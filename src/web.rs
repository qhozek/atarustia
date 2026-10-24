use crate::auth::Login;
use anyhow::{Context as _, Result};
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const SESSION_TTL: Duration = Duration::from_secs(30 * 60);
const LINK_TTL: Duration = Duration::from_secs(5 * 60);
pub fn random_token() -> String {
    let mut bytes = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
#[derive(Default)]
struct Tokens {
    links: HashMap<String, Instant>,
    sessions: HashMap<String, Instant>,
    confirmations: HashMap<String, (Instant, bool)>,
    requests: HashMap<String, Vec<Instant>>,
}
impl Tokens {
    fn purge(&mut self) {
        let now = Instant::now();
        self.links.retain(|_, t| *t > now);
        self.sessions.retain(|_, t| *t > now);
        self.confirmations.retain(|_, (expires, _)| *expires > now);
    }
    fn issue(&mut self) -> String {
        self.purge();
        if self.links.len() >= 32 {
            self.links.clear();
        }
        let t = random_token();
        self.links.insert(t.clone(), Instant::now() + LINK_TTL);
        t
    }
    fn exchange(&mut self, key: &str) -> Option<String> {
        self.purge();
        self.links.remove(key)?;
        if self.sessions.len() >= 64 {
            return None;
        }
        let session = random_token();
        self.sessions
            .insert(session.clone(), Instant::now() + SESSION_TTL);
        Some(session)
    }
    fn authorized(&mut self, s: &str) -> bool {
        self.purge();
        self.sessions.contains_key(s)
    }
}
pub struct WebState {
    tokens: Mutex<Tokens>,
    pub login: tokio::sync::RwLock<Option<Arc<Login>>>,
    accounts: Mutex<Vec<i64>>,
    pub address: SocketAddr,
    setup_pending: std::sync::atomic::AtomicBool,
    setup_value: tokio::sync::Mutex<Option<crate::config::Credentials>>,
    setup_ready: tokio::sync::Notify,
    app_configured: std::sync::atomic::AtomicBool,
    finished: tokio::sync::RwLock<Option<Value>>,
    contexts: Mutex<Vec<std::sync::Weak<crate::commands::Context>>>,
    login_view: tokio::sync::Mutex<(Value, u64)>,
    runtime: Mutex<Option<RuntimeLogin>>,
    login_operation: tokio::sync::Mutex<()>,
}
#[derive(Clone)]
struct RuntimeLogin {
    root: std::path::PathBuf,
    key: Arc<crate::session::MasterKey>,
    credentials: crate::config::Credentials,
    proxy: Option<String>,
}
impl WebState {
    pub fn new(address: SocketAddr) -> Arc<Self> {
        Arc::new(Self {
            tokens: Mutex::new(Tokens::default()),
            login: tokio::sync::RwLock::new(None),
            accounts: Mutex::new(Vec::new()),
            address,
            setup_pending: std::sync::atomic::AtomicBool::new(false),
            setup_value: tokio::sync::Mutex::new(None),
            setup_ready: tokio::sync::Notify::new(),
            app_configured: false.into(),
            finished: Default::default(),
            contexts: Default::default(),
            login_view: tokio::sync::Mutex::new((Value::Null, 0)),
            runtime: Default::default(),
            login_operation: Default::default(),
        })
    }
    pub fn link(&self) -> String {
        format!(
            "http://{}/#key={}",
            self.address,
            self.tokens.lock().expect("token lock").issue()
        )
    }
    pub fn account_ready(&self, owner: i64) {
        let mut a = self.accounts.lock().expect("accounts lock");
        if !a.contains(&owner) {
            a.push(owner);
        }
    }
    pub fn set_app_configured(&self) {
        self.app_configured
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn configure_runtime(
        &self,
        root: std::path::PathBuf,
        key: Arc<crate::session::MasterKey>,
        credentials: crate::config::Credentials,
        proxy: Option<String>,
    ) {
        *self.runtime.lock().unwrap() = Some(RuntimeLogin {
            root,
            key,
            credentials,
            proxy,
        });
    }
    async fn ensure_login(self: &Arc<Self>) -> std::result::Result<(), ApiError> {
        if self.login.read().await.is_some() {
            return Ok(());
        }
        // The primary account's Connection is owned by runtime during first start.
        if self.accounts.lock().unwrap().is_empty() {
            return Ok(());
        }
        let _lock = self.login_operation.lock().await;
        if self.login.read().await.is_some() {
            return Ok(());
        }
        let Some(runtime) = self.runtime.lock().unwrap().clone() else {
            return Ok(());
        };
        let slot = format!("web_{}", &random_token()[..12]);
        let mut connection = crate::telegram::Connection::open_with_proxy(
            &runtime.root,
            &format!("user-{slot}"),
            &runtime.credentials,
            runtime.key,
            runtime.proxy.as_deref(),
        )
        .map_err(|_| ApiError(StatusCode::BAD_GATEWAY, "telegram"))?;
        let login = Arc::new(Login::new(
            connection.client.clone(),
            connection.session.clone(),
            runtime.credentials.clone(),
        ));
        *self.login.write().await = Some(login.clone());
        *self.finished.write().await = None;
        let state = self.clone();
        tokio::spawn(async move {
            let result=async {
                let deadline=tokio::time::sleep(Duration::from_secs(10*60));tokio::pin!(deadline);
                loop {tokio::select! {
                    _=login.ready.notified()=>break,
                    _=&mut deadline=>anyhow::bail!("login expired"),
                    update=connection.updates.as_mut().unwrap().recv()=>{
                        let update=update.context("login connection closed")?;login.observe(update).await?;
                    }
                }}
                let user=connection.client.get_me().await?;
                connection.session.flush()?;
                let mut config=crate::config::Config::load(&runtime.root)?;
                anyhow::ensure!(!config.accounts.values().any(|a|a.owner_id==Some(user.id().bare_id_unchecked())),"account already added");
                config.accounts.insert(slot,crate::config::AccountConfig{owner_id:Some(user.id().bare_id_unchecked()),bot_token:None,needs_login:false});config.save(&runtime.root)?;
                state.finish_login(&login).await;
                tokio::time::sleep(Duration::from_secs(1)).await;
                let contexts:Vec<_>=state.contexts.lock().unwrap().iter().filter_map(std::sync::Weak::upgrade).collect();
                for context in contexts {context.db.set("RustRuntime","restart",true)?;context.stop.cancel();}
                Ok::<_,anyhow::Error>(())
            }.await;
            if result.is_err() {
                *state.finished.write().await =
                    Some(json!({"stage":"failed","error":"telegram","version":1}));
            }
            *state.login.write().await = None;
            let _ = connection.close().await;
        });
        Ok(())
    }
    pub fn add_context(&self, context: &Arc<crate::commands::Context>) {
        self.contexts.lock().unwrap().push(Arc::downgrade(context));
    }
    pub async fn finish_login(&self, login: &Login) {
        *self.finished.write().await = Some(serde_json::to_value(login.view().await).unwrap());
    }
    pub fn confirm(&self, token: &str) -> bool {
        if let Some((expires, confirmed)) = self.tokens.lock().unwrap().confirmations.get_mut(token)
            && *expires > Instant::now()
        {
            *confirmed = true;
            return true;
        }
        false
    }
    pub fn begin_setup(&self) {
        self.setup_pending
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub async fn take_setup(&self) -> crate::config::Credentials {
        loop {
            let notified = self.setup_ready.notified();
            if let Some(value) = self.setup_value.lock().await.take() {
                return value;
            }
            notified.await;
        }
    }
    fn authorized(&self, headers: &HeaderMap) -> bool {
        if self.accounts.lock().unwrap().is_empty() {
            return true;
        }
        let Some(cookie) = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(';').map(str::trim).find_map(|p| {
                    p.strip_prefix("ataraxia_session=")
                        .or_else(|| p.strip_prefix("atarustia_session="))
                })
            })
        else {
            return false;
        };
        self.tokens.lock().expect("token lock").authorized(cookie)
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
struct ApiError {
    status: StatusCode,
    code: &'static str,
    seconds: Option<u32>,
}
#[allow(non_snake_case)]
fn ApiError(status: StatusCode, code: &'static str) -> ApiError {
    ApiError {
        status,
        code,
        seconds: None,
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut value = json!({"error":self.code});
        if let Some(seconds) = self.seconds {
            value["seconds"] = json!(seconds);
        }
        (self.status, Json(value)).into_response()
    }
}
fn check(state: &WebState, headers: &HeaderMap) -> std::result::Result<(), ApiError> {
    if !state.authorized(headers) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "auth"));
    }
    Ok(())
}
fn json_request(headers: &HeaderMap) -> std::result::Result<(), ApiError> {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next() != Some("application/json"))
    {
        return Err(ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "bad_request"));
    }
    Ok(())
}
pub fn router(state: Arc<WebState>) -> Router {
    Router::new().route("/",get(||async { Html(include_str!("../web-resources/index.html")) }))
        .route("/static/app.js",get(||async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../web-resources/static/app.js")) }))
        .route("/static/app.css",get(||async { ([(header::CONTENT_TYPE,"text/css; charset=utf-8")],include_str!("../web-resources/static/app.css")) }))
        .route("/static/metal.js",get(||async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../web-resources/static/metal.js")) }))
        .route("/static/wordmark.png",get(||async { ([(header::CONTENT_TYPE,"image/png")],include_bytes!("../web-resources/static/wordmark.png").as_slice()) }))
        .route("/api/auth",post(authorize)).route("/api/state",get(state_view))
        .route("/api/setup",post(setup_app))
        .route("/api/app",post(setup_app)).route("/api/login",get(poll)).route("/api/cancel",post(cancel))
        .route("/api/phone",post(phone)).route("/api/code",post(code)).route("/api/password",post(password)).route("/api/qr",post(qr))
        .route("/api/logout",post(logout)).layer(DefaultBodyLimit::max(4096))
        .layer(axum::middleware::from_fn(|request:axum::extract::Request,next:axum::middleware::Next|async move {
            let mut response=next.run(request).await; let h=response.headers_mut();
            h.insert(header::CACHE_CONTROL,"no-store".parse().unwrap());
            h.insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'".parse().unwrap());
            h.insert(header::X_FRAME_OPTIONS,"DENY".parse().unwrap());
            h.insert(header::X_CONTENT_TYPE_OPTIONS,"nosniff".parse().unwrap());
            h.insert(header::REFERRER_POLICY,"no-referrer".parse().unwrap()); response
        })).with_state(state)
}
#[derive(Deserialize)]
struct AuthRequest {
    #[serde(default)]
    key: String,
}
async fn authorize(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    connection: std::result::Result<
        ConnectInfo<SocketAddr>,
        axum::extract::rejection::ExtensionRejection,
    >,
    Json(body): Json<AuthRequest>,
) -> Response {
    if json_request(&headers).is_err() {
        return ApiError(StatusCode::UNSUPPORTED_MEDIA_TYPE, "bad_request").into_response();
    }
    if state.authorized(&headers) && body.key.is_empty() {
        return Json(json!({"ok":true})).into_response();
    }
    let key = if body.key.is_empty() {
        let remote = connection
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|_| "local".into());
        match confirm_owners(&state, &headers, &remote).await {
            Ok(key) => key,
            Err(error) => return error.into_response(),
        }
    } else {
        body.key
    };
    let token = state.tokens.lock().expect("token lock").exchange(&key);
    match token {
        None => ApiError(StatusCode::FORBIDDEN, "link_invalid").into_response(),
        Some(token) => (
            [(
                header::SET_COOKIE,
                format!(
                    "ataraxia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=1800"
                ),
            )],
            Json(json!({"ok":true})),
        )
            .into_response(),
    }
}
async fn state_view(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    let authorized = state.authorized(&headers);
    let login = if authorized {
        view(&state).await?
    } else {
        Value::Null
    };
    let command = state
        .contexts
        .lock()
        .unwrap()
        .iter()
        .find_map(std::sync::Weak::upgrade)
        .map(|c| format!("{}weburl", c.db.prefix()))
        .unwrap_or_else(|| ".weburl".into());
    Ok(Json(
        json!({"version":"Rise","setup":state.accounts.lock().unwrap().is_empty(),"authorized":authorized,"can_add":can_add(&state),"app":state.app_configured.load(std::sync::atomic::Ordering::SeqCst),"apps":[{"key":"tdesktop","title":"Telegram Desktop","api_id":611335}],"command":command,"login":login}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppSetup {
    #[serde(default)]
    preset: String,
    #[serde(default)]
    api_id: Option<String>,
    #[serde(default)]
    api_hash: Option<String>,
}
async fn setup_app(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<AppSetup>,
) -> ApiResult {
    check(&state, &headers)?;
    json_request(&headers)?;
    if !state
        .setup_pending
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return Err(ApiError(StatusCode::CONFLICT, "app_invalid"));
    }
    let credentials = match body.preset.as_str() {
        "tdesktop" => crate::config::desktop_credentials(),
        "custom" | "" => {
            let secret = zeroize::Zeroizing::new(body.api_hash.unwrap_or_default());
            crate::setup::parse_app(body.api_id.as_deref().unwrap_or(""), Some(secret.as_str()))
                .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "app_invalid"))?
        }
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "app_invalid")),
    };
    let mut slot = state.setup_value.lock().await;
    if !state
        .setup_pending
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(ApiError(StatusCode::CONFLICT, "app_invalid"));
    }
    *slot = Some(credentials);
    state.set_app_configured();
    state.setup_ready.notify_one();
    Ok(Json(json!({"ok":true})))
}
async fn current(
    state: &WebState,
    headers: &HeaderMap,
) -> std::result::Result<Arc<Login>, ApiError> {
    check(state, headers)?;
    json_request(headers)?;
    state
        .login
        .read()
        .await
        .clone()
        .ok_or(ApiError(StatusCode::CONFLICT, "no_login"))
}
fn login_result(result: Result<()>) -> std::result::Result<(), ApiError> {
    match result {
        Ok(()) => Ok(()),
        Err(error) => Err(map_login_error(&error)),
    }
}
#[derive(Deserialize)]
struct Phone {
    phone: String,
}
async fn phone(State(s): State<Arc<WebState>>, h: HeaderMap, Json(b): Json<Phone>) -> ApiResult {
    let l = await_login(&s, &h).await?;
    login_result(l.phone(&b.phone).await)?;
    Ok(Json(view(&s).await?))
}
#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn code(State(s): State<Arc<WebState>>, h: HeaderMap, Json(b): Json<Code>) -> ApiResult {
    let l = current(&s, &h).await?;
    let secret = zeroize::Zeroizing::new(b.code);
    login_result(l.code(&secret).await)?;
    Ok(Json(view(&s).await?))
}
#[derive(Deserialize)]
struct Password {
    password: String,
}
async fn password(
    State(s): State<Arc<WebState>>,
    h: HeaderMap,
    Json(b): Json<Password>,
) -> ApiResult {
    let l = current(&s, &h).await?;
    let secret = zeroize::Zeroizing::new(b.password);
    login_result(l.password(&secret).await)?;
    Ok(Json(view(&s).await?))
}
async fn qr(State(s): State<Arc<WebState>>, h: HeaderMap) -> ApiResult {
    let l = await_login(&s, &h).await?;
    login_result(l.start_qr().await)?;
    Ok(Json(view(&s).await?))
}
fn can_add(state: &WebState) -> bool {
    std::env::var_os("HYSTERIA").is_none() || state.accounts.lock().unwrap().is_empty()
}
async fn await_login(
    state: &Arc<WebState>,
    headers: &HeaderMap,
) -> std::result::Result<Arc<Login>, ApiError> {
    check(state, headers)?;
    json_request(headers)?;
    if !can_add(state) {
        return Err(ApiError(StatusCode::FORBIDDEN, "limit"));
    }
    state.ensure_login().await?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(login) = state.login.read().await.clone() {
            return Ok(login);
        }
        if Instant::now() >= deadline {
            return Err(ApiError(StatusCode::BAD_GATEWAY, "telegram"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
async fn view(state: &WebState) -> std::result::Result<Value, ApiError> {
    let login = state.login.read().await.clone();
    let mut value = if let Some(login) = login {
        if let Err(error) = login.poll_qr().await {
            return Err(map_login_error(&error));
        }
        let view = login.view().await;
        if view.stage == "phone" {
            Value::Null
        } else {
            serde_json::to_value(view).unwrap()
        }
    } else {
        state.finished.read().await.clone().unwrap_or(Value::Null)
    };
    if value.is_object() {
        let mut previous = state.login_view.lock().await;
        if previous.0 != value {
            previous.1 += 1;
            previous.0 = value.clone();
        }
        value
            .as_object_mut()
            .unwrap()
            .insert("version".into(), json!(previous.1));
    }
    Ok(value)
}
async fn poll(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> ApiResult {
    if !state.authorized(&headers) && state.finished.read().await.is_none() {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "auth"));
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        let value = if state.authorized(&headers) {
            view(&state).await?
        } else {
            state.finished.read().await.clone().unwrap_or(Value::Null)
        };
        if value.is_null()
            || value.get("version").map(Value::to_string).as_ref() != query.get("v")
            || Instant::now() >= deadline
        {
            return Ok(Json(value));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn cancel(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    check(&state, &headers)?;
    json_request(&headers)?;
    if let Some(login) = state.login.read().await.clone() {
        login_result(login.cancel().await)?;
    }
    *state.finished.write().await = None;
    Ok(Json(json!({"ok":true})))
}
fn map_login_error(error: &anyhow::Error) -> ApiError {
    use grammers_client::{InvocationError, SignInError};
    let text = error.to_string();
    if error
        .downcast_ref::<tokio::time::error::Elapsed>()
        .is_some()
    {
        return ApiError(StatusCode::GATEWAY_TIMEOUT, "telegram");
    }
    if let Some(SignInError::InvalidPassword(_)) = error.downcast_ref::<SignInError>() {
        return ApiError(StatusCode::FORBIDDEN, "password_invalid");
    }
    let rpc = error
        .downcast_ref::<InvocationError>()
        .and_then(|error| match error {
            InvocationError::Rpc(rpc) => Some(rpc),
            _ => None,
        });
    if let Some(rpc) = rpc {
        return match rpc.name.as_str() {
            "PHONE_NUMBER_INVALID" => ApiError(StatusCode::BAD_REQUEST, "phone_invalid"),
            "PHONE_NUMBER_UNOCCUPIED" => ApiError(StatusCode::NOT_FOUND, "phone_unoccupied"),
            "PHONE_CODE_INVALID" => ApiError(StatusCode::FORBIDDEN, "code_invalid"),
            "PHONE_CODE_EXPIRED" | "PHONE_CODE_HASH_EMPTY" => {
                ApiError(StatusCode::GONE, "code_expired")
            }
            "PASSWORD_HASH_INVALID" => ApiError(StatusCode::FORBIDDEN, "password_invalid"),
            name if name.starts_with("FLOOD_WAIT") => ApiError {
                status: StatusCode::TOO_MANY_REQUESTS,
                code: "flood",
                seconds: rpc.value,
            },
            name if name.starts_with("RECAPTCHA_CHECK") => {
                ApiError(StatusCode::FORBIDDEN, "captcha")
            }
            _ => ApiError(StatusCode::BAD_GATEWAY, "telegram"),
        };
    }
    match text.as_str() {
        "invalid phone" => ApiError(StatusCode::BAD_REQUEST, "phone_invalid"),
        "invalid code" | "code_invalid" => ApiError(StatusCode::BAD_REQUEST, "code_invalid"),
        "invalid password" => ApiError(StatusCode::FORBIDDEN, "password_invalid"),
        "payment" => ApiError(StatusCode::PAYMENT_REQUIRED, "payment"),
        "phone_unoccupied" => ApiError(StatusCode::NOT_FOUND, "phone_unoccupied"),
        _ => ApiError(StatusCode::BAD_GATEWAY, "telegram"),
    }
}
async fn confirm_owners(
    state: &WebState,
    headers: &HeaderMap,
    remote: &str,
) -> std::result::Result<String, ApiError> {
    {
        let mut tokens = state.tokens.lock().unwrap();
        tokens.requests.retain(|_, times| {
            times.retain(|t| t.elapsed() < Duration::from_secs(180));
            !times.is_empty()
        });
        let requests = tokens.requests.entry(remote.into()).or_default();
        if requests.len() >= 3 {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "ratelimit"));
        }
        requests.push(Instant::now());
    }
    let token = random_token()[..8].to_owned();
    state.tokens.lock().unwrap().confirmations.insert(
        token.clone(),
        (Instant::now() + Duration::from_secs(300), false),
    );
    let contexts: Vec<_> = state
        .contexts
        .lock()
        .unwrap()
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .collect();
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("?");
    let mut sent = vec![];
    for context in contexts {
        let Some(bot) = context.bot.read().await.clone() else {
            continue;
        };
        let target = grammers_session::types::PeerId::from_bot_api_dialog_id(context.owner)
            .unwrap()
            .to_ambient_ref();
        let english = context.db.language().starts_with("en");
        let text = crate::original::text(english, "$web", "confirm")
            .replace("{ip}", &crate::output::escape(remote))
            .replace("{agent}", &crate::output::escape(&browser(agent)));
        let message = grammers_client::message::InputMessage::new()
            .html(text)
            .reply_markup(grammers_client::message::ReplyMarkup::from_buttons(&[
                vec![grammers_client::message::Button::data(
                    crate::original::text(english, "$web", "confirm_btn"),
                    format!("authorize_web_{token}").into_bytes(),
                )],
            ]));
        if let Ok(Ok(message)) = tokio::time::timeout(
            Duration::from_secs(15),
            bot.bot.client.send_message(target, message),
        )
        .await
        {
            sent.push(message);
        }
    }
    if sent.is_empty() {
        state.tokens.lock().unwrap().confirmations.remove(&token);
        return Err(ApiError(StatusCode::SERVICE_UNAVAILABLE, "bot"));
    }
    let deadline = Instant::now() + Duration::from_secs(300);
    let confirmed = loop {
        if state
            .tokens
            .lock()
            .unwrap()
            .confirmations
            .get(&token)
            .is_some_and(|(_, yes)| *yes)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    state.tokens.lock().unwrap().confirmations.remove(&token);
    for message in sent {
        let _ = tokio::time::timeout(Duration::from_secs(5), message.delete()).await;
    }
    if !confirmed {
        return Err(ApiError(StatusCode::REQUEST_TIMEOUT, "timeout"));
    }
    Ok(state.tokens.lock().unwrap().issue())
}
fn browser(agent: &str) -> String {
    let name = [
        ("Edg/", "Edge"),
        ("OPR/", "Opera"),
        ("YaBrowser/", "Yandex Browser"),
        ("Firefox/", "Firefox"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .into_iter()
    .find(|(key, _)| agent.contains(key))
    .map(|(_, name)| name);
    let os = [
        ("Android", "Android"),
        ("iPhone", "iOS"),
        ("iPad", "iPadOS"),
        ("Mac OS X", "macOS"),
        ("Windows", "Windows"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find(|(key, _)| agent.contains(key))
    .map(|(_, name)| name);
    let value = [name, os]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    if value.is_empty() { "?".into() } else { value }
}
async fn logout(State(s): State<Arc<WebState>>, h: HeaderMap) -> ApiResult {
    check(&s, &h)?;
    json_request(&h)?;
    if let Some(cookie) = h.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie.split(';') {
            if let Some(token) = part.trim().strip_prefix("ataraxia_session=") {
                s.tokens.lock().unwrap().sessions.remove(token);
            }
        }
    }
    Ok(Json(json!({"ok":true})))
}
pub async fn serve(state: Arc<WebState>, stop: CancellationToken) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(state.address).await?;
    serve_listener(state, listener, stop).await
}
pub async fn serve_listener(
    state: Arc<WebState>,
    listener: tokio::net::TcpListener,
    stop: CancellationToken,
) -> Result<()> {
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stop.cancelled_owned())
    .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn api_setup_validates_and_can_only_finish_once() {
        use axum::{body::Body, http::Request};
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
        state.begin_setup();
        let link = state.link();
        let key = link.split("#key=").nth(1).unwrap();
        let app = router(state.clone());
        let post = |cookie: Option<&str>, value: Value| {
            let mut r = Request::builder()
                .method("POST")
                .uri("/api/app")
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(cookie) = cookie {
                r = r.header(header::COOKIE, cookie);
            }
            r.body(Body::from(value.to_string())).unwrap()
        };
        let auth = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json!({"key":key}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let cookie = auth.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(
            app.clone()
                .oneshot(post(
                    Some(&cookie),
                    json!({"preset":"custom","api_id":"0","api_hash":"bad"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            app.clone()
                .oneshot(post(
                    Some(&cookie),
                    json!({"preset":"custom","api_id":"123","api_hash":"A".repeat(32)})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let value = state.take_setup().await;
        assert_eq!(value.api_id, 123);
        assert_eq!(value.api_hash, "a".repeat(32));
        assert_eq!(
            app.oneshot(post(Some(&cookie), json!({"preset":"tdesktop"})))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
    #[test]
    fn links_single_use_and_expire() {
        let mut t = Tokens::default();
        let key = t.issue();
        let s = t.exchange(&key).unwrap();
        assert!(t.exchange(&key).is_none());
        assert!(t.authorized(&s));
        t.sessions
            .insert(s.clone(), Instant::now() - Duration::from_secs(1));
        assert!(!t.authorized(&s));
        let k = t.issue();
        t.links
            .insert(k.clone(), Instant::now() - Duration::from_secs(1));
        assert!(t.exchange(&k).is_none());
    }
    #[tokio::test]
    async fn initial_state_matches_original_open_setup_contract() {
        let app = router(WebState::new("127.0.0.1:8080".parse().unwrap()));
        let r = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/api/state")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert!(r.headers().contains_key(header::CONTENT_SECURITY_POLICY));
        let bytes = axum::body::to_bytes(r.into_body(), 4096).await.unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["setup"], true);
        assert_eq!(value["authorized"], true);
        assert_eq!(value["apps"][0]["key"], "tdesktop");
        assert_eq!(value["command"], ".weburl");
    }
    #[tokio::test]
    async fn cookie_exchange_and_logout() {
        use axum::{body::Body, http::Request};
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
        state.account_ready(1);
        let link = state.link();
        let key = link.split("#key=").nth(1).unwrap();
        let app = router(state);
        let auth = || {
            Request::builder()
                .method("POST")
                .uri("/api/auth")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"key":key}).to_string()))
                .unwrap()
        };
        let response = app.clone().oneshot(auth()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert_eq!(
            app.clone().oneshot(auth()).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let cookie = cookie.split(';').next().unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/logout")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["authorized"], false);
    }
    #[tokio::test]
    async fn unauthorized_posts_never_enter_login() {
        use axum::{body::Body, http::Request};
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
        state.account_ready(1);
        let app = router(state);
        for route in ["phone", "code", "password", "qr"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/{route}"))
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(
                            r#"{"phone":"+12345678","code":"12345","password":"test"}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }
    #[tokio::test]
    async fn assets_are_local_and_have_strict_headers() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        let app = router(WebState::new("127.0.0.1:8080".parse().unwrap()));
        for route in [
            "/",
            "/static/app.js",
            "/static/app.css",
            "/static/metal.js",
            "/static/wordmark.png",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            assert!(!bytes.is_empty());
        }
    }
    #[tokio::test]
    async fn original_page_and_server_assets_are_byte_identical() {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        let app = router(WebState::new("127.0.0.1:8080".parse().unwrap()));
        for (path, bytes) in [
            (
                "/",
                include_bytes!("../web-resources/index.html").as_slice(),
            ),
            (
                "/static/app.js",
                include_bytes!("../web-resources/static/app.js").as_slice(),
            ),
            (
                "/static/app.css",
                include_bytes!("../web-resources/static/app.css").as_slice(),
            ),
            (
                "/static/metal.js",
                include_bytes!("../web-resources/static/metal.js").as_slice(),
            ),
            (
                "/static/wordmark.png",
                include_bytes!("../web-resources/static/wordmark.png").as_slice(),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
            assert_eq!(
                to_bytes(response.into_body(), 64 * 1024)
                    .await
                    .unwrap()
                    .as_ref(),
                bytes
            );
        }
    }
    #[test]
    fn browser_names_and_confirmation_expiry_follow_original() {
        assert_eq!(
            browser(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Version/17.0 Safari/604.1"
            ),
            "Safari · iOS"
        );
        assert_eq!(browser("curl/8"), "?");
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
        state.tokens.lock().unwrap().confirmations.insert(
            "test".into(),
            (Instant::now() + Duration::from_secs(60), false),
        );
        assert!(state.confirm("test"));
        assert!(!state.confirm("missing"));
        state.tokens.lock().unwrap().confirmations.insert(
            "expired".into(),
            (Instant::now() - Duration::from_secs(1), false),
        );
        assert!(!state.confirm("expired"));
    }
}
