use crate::auth::Login;
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
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
}
impl Tokens {
    fn purge(&mut self) {
        let now = Instant::now();
        self.links.retain(|_, t| *t > now);
        self.sessions.retain(|_, t| *t > now);
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
        let Some(cookie) = headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| {
                s.split(';')
                    .map(str::trim)
                    .find_map(|p| p.strip_prefix("atarustia_session="))
            })
        else {
            return false;
        };
        self.tokens.lock().expect("token lock").authorized(cookie)
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;
struct ApiError(StatusCode, &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
fn check(state: &WebState, headers: &HeaderMap) -> std::result::Result<(), ApiError> {
    if !state.authorized(headers) {
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "Open a new one-time link from the terminal or .weburl",
        ));
    }
    Ok(())
}
fn json_request(headers: &HeaderMap) -> std::result::Result<(), ApiError> {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next() != Some("application/json"))
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "JSON required",
        ));
    }
    Ok(())
}
pub fn router(state: Arc<WebState>) -> Router {
    Router::new().route("/",get(||async { Html(include_str!("../web/index.html")) }))
        .route("/app.js",get(||async { ([(header::CONTENT_TYPE,"text/javascript; charset=utf-8")],include_str!("../web/app.js")) }))
        .route("/style.css",get(||async { ([(header::CONTENT_TYPE,"text/css; charset=utf-8")],include_str!("../web/style.css")) }))
        .route("/api/auth",post(authorize)).route("/api/state",get(state_view))
        .route("/api/setup",post(setup_app))
        .route("/api/phone",post(phone)).route("/api/code",post(code)).route("/api/password",post(password)).route("/api/qr",post(qr))
        .route("/api/logout",post(logout)).layer(DefaultBodyLimit::max(4096))
        .layer(axum::middleware::from_fn(|request:axum::extract::Request,next:axum::middleware::Next|async move {
            let mut response=next.run(request).await; let h=response.headers_mut();
            h.insert(header::CACHE_CONTROL,"no-store".parse().unwrap());
            h.insert(header::CONTENT_SECURITY_POLICY,"default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".parse().unwrap());
            h.insert(header::X_CONTENT_TYPE_OPTIONS,"nosniff".parse().unwrap());
            h.insert(header::REFERRER_POLICY,"no-referrer".parse().unwrap()); response
        })).with_state(state)
}
#[derive(Deserialize)]
struct AuthRequest {
    key: String,
}
async fn authorize(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(body): Json<AuthRequest>,
) -> Response {
    if json_request(&headers).is_err() || body.key.len() != 64 {
        return ApiError(StatusCode::FORBIDDEN, "Invalid link").into_response();
    }
    let token = state.tokens.lock().expect("token lock").exchange(&body.key);
    match token {
        None => ApiError(StatusCode::FORBIDDEN, "Link expired or already used").into_response(),
        Some(token) => (
            [(
                header::SET_COOKIE,
                format!(
                    "atarustia_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=1800"
                ),
            )],
            Json(json!({"ok":true})),
        )
            .into_response(),
    }
}
async fn state_view(State(state): State<Arc<WebState>>, headers: HeaderMap) -> ApiResult {
    check(&state, &headers)?;
    let login = state.login.read().await.clone();
    let login = if let Some(l) = login {
        let _ = l.poll_qr().await;
        serde_json::to_value(l.view().await).unwrap()
    } else if state
        .setup_pending
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        json!({"stage":"setup"})
    } else if state.accounts.lock().expect("accounts lock").is_empty() {
        json!({"stage":"connecting"})
    } else {
        json!({"stage":"ready"})
    };
    Ok(Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"accounts":state.accounts.lock().unwrap().clone(),"login":login}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppSetup {
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
        return Err(ApiError(StatusCode::CONFLICT, "App is already configured"));
    }
    let credentials = match body.preset.as_str() {
        "tdesktop" => crate::config::desktop_credentials(),
        "custom" => {
            let secret = zeroize::Zeroizing::new(body.api_hash.unwrap_or_default());
            crate::setup::parse_app(body.api_id.as_deref().unwrap_or(""), Some(secret.as_str()))
                .map_err(|_| {
                    ApiError(
                        StatusCode::BAD_REQUEST,
                        "API ID must be positive; hash must be 32 hexadecimal characters",
                    )
                })?
        }
        _ => return Err(ApiError(StatusCode::BAD_REQUEST, "Choose an app")),
    };
    let mut slot = state.setup_value.lock().await;
    if !state
        .setup_pending
        .swap(false, std::sync::atomic::Ordering::SeqCst)
    {
        return Err(ApiError(StatusCode::CONFLICT, "App is already configured"));
    }
    *slot = Some(credentials);
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
        .ok_or(ApiError(StatusCode::CONFLICT, "No login in progress"))
}
fn login_result(result: Result<()>) -> ApiResult {
    match result {
        Ok(()) => Ok(Json(json!({"ok":true}))),
        Err(_) => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Telegram login failed or timed out. Check the input and try again; request a new code if necessary.",
        )),
    }
}
#[derive(Deserialize)]
struct Phone {
    phone: String,
}
async fn phone(State(s): State<Arc<WebState>>, h: HeaderMap, Json(b): Json<Phone>) -> ApiResult {
    let l = current(&s, &h).await?;
    login_result(l.phone(&b.phone).await)
}
#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn code(State(s): State<Arc<WebState>>, h: HeaderMap, Json(b): Json<Code>) -> ApiResult {
    let l = current(&s, &h).await?;
    let secret = zeroize::Zeroizing::new(b.code);
    login_result(l.code(&secret).await)
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
    login_result(l.password(&secret).await)
}
async fn qr(State(s): State<Arc<WebState>>, h: HeaderMap) -> ApiResult {
    let l = current(&s, &h).await?;
    login_result(l.start_qr().await)
}
async fn logout(State(s): State<Arc<WebState>>, h: HeaderMap) -> ApiResult {
    check(&s, &h)?;
    json_request(&h)?;
    if let Some(cookie) = h.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie.split(';') {
            if let Some(token) = part.trim().strip_prefix("atarustia_session=") {
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
    axum::serve(listener, router(state))
        .with_graceful_shutdown(stop.cancelled_owned())
        .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn api_setup_requires_cookie_validates_and_can_only_finish_once() {
        use axum::{body::Body, http::Request};
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
        state.begin_setup();
        let link = state.link();
        let key = link.split("#key=").nth(1).unwrap();
        let app = router(state.clone());
        let post = |cookie: Option<&str>, value: Value| {
            let mut r = Request::builder()
                .method("POST")
                .uri("/api/setup")
                .header(header::CONTENT_TYPE, "application/json");
            if let Some(cookie) = cookie {
                r = r.header(header::COOKIE, cookie);
            }
            r.body(Body::from(value.to_string())).unwrap()
        };
        assert_eq!(
            app.clone()
                .oneshot(post(None, json!({"preset":"tdesktop"})))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
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
    async fn setup_is_not_public() {
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
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        assert!(r.headers().contains_key(header::CONTENT_SECURITY_POLICY));
    }
    #[tokio::test]
    async fn cookie_exchange_and_logout() {
        use axum::{body::Body, http::Request};
        let state = WebState::new("127.0.0.1:8080".parse().unwrap());
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
        assert_eq!(
            app.oneshot(
                Request::builder()
                    .uri("/api/state")
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn unauthorized_posts_never_enter_login() {
        use axum::{body::Body, http::Request};
        let app = router(WebState::new("127.0.0.1:8080".parse().unwrap()));
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
        for route in ["/", "/app.js", "/style.css"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
            let body = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(!body.contains("https://"));
            assert!(!body.contains("http://"));
        }
    }
}
