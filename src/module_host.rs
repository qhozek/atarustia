//! Capability-checked asynchronous I/O for installed Rhai packages.
//!
//! Rhai runs once on the blocking pool. A bounded channel suspends only that
//! worker while Tokio performs I/O; no replay of code or side effects is needed.
use crate::{commands::Context, modules::Package};
use anyhow::{Context as _, Result, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use grammers_client::{message::InputMessage, tl};
use grammers_session::types::{PeerId, PeerRef};
use rhai::{Dynamic, Engine, EvalAltResult, Map};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot};

pub const MAX_CALLS: usize = 16;
pub const MAX_VALUE: usize = 128 * 1024;
pub const MAX_DOWNLOAD: usize = 64 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(15);
const INVOCATION_TIMEOUT: Duration = Duration::from_secs(60);
const OPERATIONS: &[&str] = &[
    "get_me",
    "get_chat",
    "get_messages",
    "get_reply",
    "history",
    "download_file",
    "send_message",
    "edit_message",
    "delete_messages",
    "send_media",
    "send_file",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Capabilities {
    /// Exact HTTPS origins, including a non-default port if needed. No wildcards.
    pub http: Vec<String>,
    pub telegram: TelegramCapabilities,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelegramCapabilities {
    pub operations: Vec<String>,
    /// `current`, `me`, signed Bot API dialog IDs or exact @usernames.
    pub chats: Vec<String>,
    /// Whether the package may also act through the inline bot.
    pub bot: bool,
}
impl Capabilities {
    pub fn is_empty(&self) -> bool {
        self.http.is_empty()
            && self.telegram.operations.is_empty()
            && self.telegram.chats.is_empty()
            && !self.telegram.bot
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.http.len() <= 16, "too many HTTP origins");
        for origin in &self.http {
            let url = https_url(origin)?;
            ensure!(
                url.path() == "/" && url.query().is_none(),
                "HTTP capability must be an origin, not a path or query"
            );
        }
        ensure!(
            self.telegram.operations.len() <= OPERATIONS.len(),
            "too many Telegram operations"
        );
        for operation in &self.telegram.operations {
            ensure!(
                OPERATIONS.contains(&operation.as_str()),
                "unknown Telegram operation: {operation}"
            );
        }
        ensure!(
            self.telegram.chats.len() <= 64,
            "too many Telegram chat grants"
        );
        for chat in &self.telegram.chats {
            normalize_chat(chat)?;
        }
        Ok(())
    }
    pub fn authorize(&self, method: &str, params: &Value, mode: &Mode) -> Result<()> {
        if method == "http.request" {
            let request: HttpRequest = serde_json::from_value(params.clone())?;
            let url = https_url(&request.url)?;
            ensure!(
                self.http.iter().any(|origin| https_url(origin)
                    .is_ok_and(|allowed| allowed.origin() == url.origin())),
                "HTTP origin is not granted by this package"
            );
            ensure!(
                !mode.preview || request.method.eq_ignore_ascii_case("GET"),
                "inline previews may only perform HTTP GET"
            );
            request.validate()?;
            return Ok(());
        }
        let operation = method
            .strip_prefix("telegram.")
            .context("unknown host method")?;
        ensure!(
            OPERATIONS.contains(&operation),
            "unknown Telegram operation"
        );
        validate_telegram_params(operation, params)?;
        ensure!(
            self.telegram.operations.iter().any(|v| v == operation),
            "Telegram operation is not granted by this package"
        );
        ensure!(
            !mode.preview || !is_write(operation),
            "Telegram writes are forbidden during inline preview"
        );
        if params.get("bot").and_then(Value::as_bool).unwrap_or(false) {
            ensure!(
                self.telegram.bot,
                "acting as the bot is not granted by this package"
            );
        }
        if operation != "get_me" {
            let target = chat_param(params)?;
            let normalized = normalize_chat(&target)?;
            let current = mode.chat.map(|id| id.to_string());
            let allowed = self.telegram.chats.iter().any(|grant| {
                let Ok(grant) = normalize_chat(grant) else {
                    return false;
                };
                grant == normalized
                    || grant == "current" && current.as_deref() == Some(normalized.as_str())
            });
            ensure!(allowed, "Telegram chat is not granted by this package");
            if normalized == "current" {
                ensure!(mode.chat.is_some(), "no current chat in this invocation");
            }
        }
        Ok(())
    }
}
fn is_write(operation: &str) -> bool {
    matches!(
        operation,
        "send_message" | "edit_message" | "delete_messages" | "send_media" | "send_file"
    )
}
fn https_url(value: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(value)?;
    ensure!(
        value.len() <= 8192
            && url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "host HTTP requires HTTPS without credentials or a fragment"
    );
    Ok(url)
}
fn normalize_chat(value: &str) -> Result<String> {
    if matches!(value, "current" | "me") {
        return Ok(value.into());
    }
    if let Ok(id) = value.parse::<i64>() {
        ensure!(
            PeerId::from_bot_api_dialog_id(id).is_some(),
            "invalid chat ID"
        );
        return Ok(id.to_string());
    }
    let name = value
        .strip_prefix('@')
        .context("chat must be current, me, a dialog ID or @username")?;
    ensure!(
        (1..=64).contains(&name.len())
            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
        "invalid chat username"
    );
    Ok(format!("@{}", name.to_ascii_lowercase()))
}
fn chat_param(params: &Value) -> Result<String> {
    match params.get("chat") {
        Some(Value::String(chat)) => Ok(chat.clone()),
        Some(Value::Number(chat)) if chat.is_i64() => Ok(chat.to_string()),
        None => Ok("current".into()),
        _ => bail!("chat must be a string or signed dialog ID"),
    }
}

#[derive(Clone, Debug, Default)]
pub struct Mode {
    pub chat: Option<i64>,
    pub message_id: Option<i32>,
    pub preview: bool,
}
struct Request {
    method: String,
    params: Value,
    response: oneshot::Sender<std::result::Result<Value, String>>,
}
#[derive(Clone)]
pub(crate) struct Bridge {
    sender: mpsc::Sender<Request>,
    mode: Mode,
}
impl Bridge {
    fn call(&self, method: &str, params: Map) -> std::result::Result<Dynamic, Box<EvalAltResult>> {
        let run = || -> Result<Dynamic> {
            let params = rhai::serde::from_dynamic::<Value>(&Dynamic::from_map(params))
                .map_err(|_| anyhow::anyhow!("host parameters must be JSON values"))?;
            ensure!(
                serde_json::to_vec(&params)?.len() <= MAX_VALUE,
                "host request too large"
            );
            let (response, receiver) = oneshot::channel();
            self.sender
                .blocking_send(Request {
                    method: method.into(),
                    params,
                    response,
                })
                .map_err(|_| anyhow::anyhow!("host invocation ended"))?;
            let value = receiver
                .blocking_recv()
                .map_err(|_| anyhow::anyhow!("host invocation cancelled"))?
                .map_err(anyhow::Error::msg)?;
            rhai::serde::to_dynamic(value).map_err(|_| anyhow::anyhow!("invalid host response"))
        };
        run().map_err(|error| error.to_string().into())
    }
    pub(crate) fn register(&self, engine: &mut Engine) {
        let sender = self.sender.clone();
        engine.on_progress(move |_| {
            sender
                .is_closed()
                .then(|| Dynamic::from("host invocation ended"))
        });
        let context = json!({"chat":self.mode.chat,"message_id":self.mode.message_id,"preview":self.mode.preview});
        engine.register_fn("invocation", move || {
            rhai::serde::to_dynamic(&context).expect("invocation context is JSON")
        });
        let bridge = self.clone();
        engine.register_fn("host_call", move |method: &str, params: Map| {
            bridge.call(method, params)
        });
        let bridge = self.clone();
        engine.register_fn("http_get", move |url: &str| {
            let mut params = Map::new();
            params.insert("url".into(), url.into());
            bridge.call("http.request", params)
        });
        engine.register_fn(
            "json_parse",
            |text: &str| -> std::result::Result<Dynamic, Box<EvalAltResult>> {
                let value: Value = serde_json::from_str(text)
                    .map_err(|_| Box::<EvalAltResult>::from("invalid JSON"))?;
                rhai::serde::to_dynamic(value).map_err(|_| "invalid JSON value".into())
            },
        );
        engine.register_fn(
            "json_stringify",
            |value: Dynamic| -> std::result::Result<String, Box<EvalAltResult>> {
                let value: Value = rhai::serde::from_dynamic(&value)
                    .map_err(|_| Box::<EvalAltResult>::from("not a JSON value"))?;
                serde_json::to_string(&value).map_err(|_| "not a JSON value".into())
            },
        );
        engine.register_fn("base64_encode", |text: &str| {
            STANDARD.encode(text.as_bytes())
        });
        engine.register_fn(
            "base64_text",
            |text: &str| -> std::result::Result<String, Box<EvalAltResult>> {
                let bytes = STANDARD
                    .decode(text)
                    .map_err(|_| Box::<EvalAltResult>::from("invalid base64"))?;
                String::from_utf8(bytes).map_err(|_| "file is not UTF-8 text".into())
            },
        );
    }
}

pub type HostFuture<'a> = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'a>>;
/// Transport seam used by deterministic tests as well as the live account host.
pub trait Transport: Send + Sync + 'static {
    fn call<'a>(&'a self, method: &'a str, params: Value) -> HostFuture<'a>;
    fn redact(&self, _error: String) -> String {
        "host operation failed (see logs)".into()
    }
}
pub async fn run_command(
    context: &Context,
    package: Package,
    command: String,
    args: String,
    mode: Mode,
) -> Result<crate::modules::Effect> {
    let (config, state, language) = inputs(context, &package);
    let transport = Arc::new(AccountTransport::new(context, mode.clone()).await?);
    let capabilities = package.manifest.capabilities.clone();
    let module = package.manifest.name.clone();
    let result = execute(
        capabilities,
        mode,
        context.stop.clone(),
        transport,
        move |bridge| {
            package.run_command_with_host(&command, &args, config, state, &language, Some(bridge))
        },
    )
    .await;
    if let Err(error) = &result {
        crate::module_lifecycle::report(context, &module, "command", error);
    }
    result
}
pub async fn run_event(
    context: &Context,
    package: Package,
    function: String,
    event: Value,
    mode: Mode,
) -> Result<crate::modules::Effect> {
    run_event_with_stop(
        context,
        package,
        function,
        event,
        mode,
        context.stop.clone(),
    )
    .await
}
pub(crate) async fn run_event_with_stop(
    context: &Context,
    package: Package,
    function: String,
    event: Value,
    mode: Mode,
    stop: tokio_util::sync::CancellationToken,
) -> Result<crate::modules::Effect> {
    let (config, state, language) = inputs(context, &package);
    let transport = Arc::new(AccountTransport::new(context, mode.clone()).await?);
    let capabilities = package.manifest.capabilities.clone();
    execute(capabilities, mode, stop, transport, move |bridge| {
        package.run_event_with_host(&function, event, config, state, &language, Some(bridge))
    })
    .await
}
fn inputs(context: &Context, package: &Package) -> (Value, Value, String) {
    let name = &package.manifest.name;
    let config = context.db.get(name, "__config__");
    (
        if config.is_null() {
            package.manifest.config.clone()
        } else {
            config
        },
        context.db.get(name, "state"),
        context.db.language(),
    )
}
pub(crate) async fn execute<F>(
    capabilities: Capabilities,
    mode: Mode,
    stop: tokio_util::sync::CancellationToken,
    transport: Arc<dyn Transport>,
    script: F,
) -> Result<crate::modules::Effect>
where
    F: FnOnce(Bridge) -> Result<crate::modules::Effect> + Send + 'static,
{
    capabilities.validate()?;
    let (sender, mut receiver) = mpsc::channel::<Request>(1);
    let worker_mode = mode.clone();
    let mut worker = tokio::task::spawn_blocking(move || {
        script(Bridge {
            sender,
            mode: worker_mode,
        })
    });
    let deadline = tokio::time::sleep(INVOCATION_TIMEOUT);
    tokio::pin!(deadline);
    let mut calls = 0;
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => bail!("module invocation cancelled"),
            _ = &mut deadline => bail!("module invocation timed out"),
            result = &mut worker => return result?,
            request = receiver.recv() => {
                let Some(request) = request else { return worker.await? };
                calls += 1;
                // Authorization errors are deliberately specific; remote errors are redacted.
                let permission = capabilities.authorize(&request.method, &request.params, &mode);
                let result = if calls > MAX_CALLS {
                    Err("host call budget exceeded".into())
                } else if let Err(error) = permission {
                    Err(error.to_string())
                } else {
                    let result = tokio::select! {
                        _ = stop.cancelled() => bail!("module invocation cancelled"),
                        _ = &mut deadline => bail!("module invocation timed out"),
                        value = tokio::time::timeout(CALL_TIMEOUT, transport.call(&request.method, request.params)) => value,
                    };
                    match result {
                        Ok(Ok(value)) if serde_json::to_vec(&value)?.len() <= MAX_VALUE => Ok(value),
                        Ok(Ok(_)) => Err("host response too large".into()),
                        Ok(Err(error)) => Err(transport.redact(error.to_string())),
                        Err(_) => Err("host operation timed out".into()),
                    }
                };
                let _ = request.response.send(result);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct HttpRequest {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Option<String>,
    response: String,
}
impl Default for HttpRequest {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: "GET".into(),
            headers: BTreeMap::new(),
            body: None,
            response: "text".into(),
        }
    }
}
impl HttpRequest {
    fn validate(&self) -> Result<()> {
        ensure!(
            matches!(self.response.as_str(), "text" | "base64"),
            "response must be text or base64"
        );
        ensure!(
            matches!(
                self.method.to_ascii_uppercase().as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
            ),
            "unsupported HTTP method"
        );
        ensure!(
            self.body.as_ref().is_none_or(|b| b.len() <= 64 * 1024) && self.headers.len() <= 16,
            "HTTP request exceeds limits"
        );
        for (key, value) in &self.headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
            ensure!(
                !matches!(
                    name.as_str(),
                    "host"
                        | "content-length"
                        | "connection"
                        | "transfer-encoding"
                        | "proxy-authorization"
                ),
                "reserved HTTP header"
            );
            ensure!(value.len() <= 8192, "HTTP header too long");
            reqwest::header::HeaderValue::from_str(value)?;
        }
        Ok(())
    }
}
async fn http(client: &reqwest::Client, params: Value) -> Result<Value> {
    let request: HttpRequest = serde_json::from_value(params)?;
    request.validate()?;
    let mut builder = client.request(
        reqwest::Method::from_bytes(request.method.to_ascii_uppercase().as_bytes())?,
        https_url(&request.url)?,
    );
    for (key, value) in request.headers {
        builder = builder.header(key, value);
    }
    if let Some(body) = request.body {
        builder = builder.body(body);
    }
    let response = builder.send().await.map_err(reqwest::Error::without_url)?;
    read_http_response(response, &request.response).await
}
async fn read_http_response(mut response: reqwest::Response, encoding: &str) -> Result<Value> {
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= 64 * 1024),
        "HTTP response exceeds 64 KiB"
    );
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        ensure!(
            bytes.len() + chunk.len() <= 64 * 1024,
            "HTTP response exceeds 64 KiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    let body = if encoding == "base64" {
        STANDARD.encode(bytes)
    } else {
        String::from_utf8(bytes)?
    };
    Ok(
        json!({"status": status, "body": body, "encoding":encoding, "ok": (200..300).contains(&status)}),
    )
}

struct AccountTransport {
    user: crate::native_bot::NativeBot,
    bot: Option<crate::native_bot::NativeBot>,
    http: reqwest::Client,
    mode: Mode,
    logs: crate::logs::LogBuffer,
}
impl AccountTransport {
    async fn new(context: &Context, mode: Mode) -> Result<Self> {
        Ok(Self {
            user: crate::native_bot::NativeBot {
                client: context.client.clone(),
                id: context.owner,
                username: context.username.clone().unwrap_or_default(),
                session: None,
            },
            bot: context.bot.read().await.as_ref().map(|b| b.bot.native()),
            http: http_client()?,
            mode,
            logs: context.logs.clone(),
        })
    }
    async fn peer(&self, actor: &crate::native_bot::NativeBot, params: &Value) -> Result<PeerRef> {
        let chat = normalize_chat(&chat_param(params)?)?;
        let chat = if chat == "current" {
            self.mode.chat.context("no current chat")?.to_string()
        } else {
            chat
        };
        if chat == "me" {
            if actor.id == self.user.id {
                return Ok(PeerId::self_user().to_ambient_ref());
            }
            let owner = PeerId::from_bot_api_dialog_id(self.user.id).context("invalid owner ID")?;
            return actor
                .client
                .resolve_peer(owner.to_ambient_ref())
                .await?
                .to_ref()
                .await
                .map_err(anyhow::Error::from_boxed)?
                .context("owner is not known to the bot");
        }
        if let Ok(id) = chat.parse::<i64>() {
            let id = PeerId::from_bot_api_dialog_id(id).context("invalid chat ID")?;
            return actor
                .client
                .resolve_peer(id.to_ambient_ref())
                .await?
                .to_ref()
                .await
                .map_err(anyhow::Error::from_boxed)?
                .context("chat is not cached; use @username");
        }
        actor.resolve_chat(&chat).await
    }
    async fn telegram(&self, operation: &str, params: Value) -> Result<Value> {
        let actor = if params.get("bot").and_then(Value::as_bool).unwrap_or(false) {
            self.bot.as_ref().context("inline bot is not connected")?
        } else {
            &self.user
        };
        if operation == "get_me" {
            return Ok(serde_json::to_value(actor.get_me().await?)?);
        }
        let peer = self.peer(actor, &params).await?;
        match operation {
            "get_chat" => Ok(serde_json::to_value(actor.get_chat(peer).await?)?),
            "get_messages" => {
                let ids = ids(&params)?;
                let messages = actor.client.get_messages_by_id(peer, &ids).await?;
                ensure!(
                    messages.iter().flatten().all(|m| m.peer_id() == peer.id),
                    "message belongs to another chat"
                );
                Ok(serde_json::to_value(
                    messages
                        .into_iter()
                        .map(|m| m.map(|m| crate::botapi::Message::from_native(&m)))
                        .collect::<Vec<_>>(),
                )?)
            }
            "history" => {
                let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10);
                ensure!((1..=50).contains(&limit), "history limit must be 1–50");
                let mut history = actor.client.iter_messages(peer).limit(limit as usize);
                let mut messages = vec![];
                while let Some(message) = history.next().await? {
                    messages.push(crate::botapi::Message::from_native(&message));
                }
                Ok(serde_json::to_value(messages)?)
            }
            "get_reply" => {
                let id = params
                    .get("message_id")
                    .and_then(Value::as_i64)
                    .map(|id| i32::try_from(id).context("invalid message ID"))
                    .transpose()?
                    .or(self.mode.message_id)
                    .context("get_reply needs a message_id outside message handlers")?;
                ensure!(id > 0, "invalid message ID");
                let messages = actor.client.get_messages_by_id(peer, &[id]).await?;
                let message = messages
                    .into_iter()
                    .flatten()
                    .next()
                    .context("message not found")?;
                ensure!(
                    message.peer_id() == peer.id,
                    "message belongs to another chat"
                );
                let reply = message.get_reply().await?;
                ensure!(
                    reply.as_ref().is_none_or(|m| m.peer_id() == peer.id),
                    "reply belongs to another chat"
                );
                Ok(serde_json::to_value(
                    reply.map(|m| crate::botapi::Message::from_native(&m)),
                )?)
            }
            "send_message" => {
                let message = actor.client.send_message(peer, input(&params)?).await?;
                Ok(serde_json::to_value(crate::botapi::Message::from_native(
                    &message,
                ))?)
            }
            "edit_message" => {
                let id = message_id(&params)?;
                let messages = actor.client.get_messages_by_id(peer, &[id]).await?;
                ensure!(
                    messages.iter().flatten().any(|m| m.peer_id() == peer.id),
                    "message not found in granted chat"
                );
                actor.edit_message_text(peer, id, input(&params)?).await?;
                Ok(json!(true))
            }
            "delete_messages" => {
                let ids = ids(&params)?;
                // Fetch in the approved peer first: non-channel deletion RPCs are account-wide.
                let messages = actor.client.get_messages_by_id(peer, &ids).await?;
                ensure!(
                    messages.iter().all(Option::is_some),
                    "message not found in granted chat"
                );
                ensure!(
                    messages.iter().flatten().all(|m| m.peer_id() == peer.id),
                    "message belongs to another chat"
                );
                actor.delete_messages(peer, &ids).await?;
                Ok(json!(true))
            }
            "download_file" => {
                let messages = actor
                    .client
                    .get_messages_by_id(peer, &[message_id(&params)?])
                    .await?;
                let message = messages
                    .into_iter()
                    .flatten()
                    .next()
                    .context("message not found")?;
                ensure!(
                    message.peer_id() == peer.id,
                    "message belongs to another chat"
                );
                let media = message.media().context("message has no file")?;
                let mut download = actor.client.iter_download(&media);
                let mut bytes = vec![];
                while let Some(chunk) = download.next().await? {
                    ensure!(
                        bytes.len() + chunk.len() <= MAX_DOWNLOAD,
                        "script download exceeds 64 KiB"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                Ok(json!({"base64":STANDARD.encode(&bytes),"size":bytes.len()}))
            }
            "send_media" => {
                let media: crate::media::Media =
                    serde_json::from_value(params.get("media").context("media missing")?.clone())?;
                media.validate()?;
                // The script supplies bytes or a Telegram file ID, never a local path.
                ensure!(
                    !matches!(media.source, crate::media::Source::Url { .. }),
                    "host send_media requires bytes or file_id; fetch approved HTTP data explicitly"
                );
                let message = actor.send_media(peer, &media, input(&params)?).await?;
                Ok(serde_json::to_value(crate::botapi::Message::from_native(
                    &message,
                ))?)
            }
            "send_file" => {
                let bytes = STANDARD.decode(
                    params
                        .get("base64")
                        .and_then(Value::as_str)
                        .context("base64 missing")?,
                )?;
                ensure!(
                    !bytes.is_empty() && bytes.len() <= MAX_DOWNLOAD,
                    "script file must contain 1 byte–64 KiB"
                );
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .context("filename missing")?;
                let media = crate::media::Media {
                    source: crate::media::Source::Bytes {
                        name: name.into(),
                        data: bytes,
                    },
                    ..Default::default()
                };
                media.validate()?;
                let message = actor.send_media(peer, &media, input(&params)?).await?;
                Ok(serde_json::to_value(crate::botapi::Message::from_native(
                    &message,
                ))?)
            }
            _ => bail!("unknown Telegram operation"),
        }
    }
}
fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(CALL_TIMEOUT)
        .build()?)
}
impl Transport for AccountTransport {
    fn call<'a>(&'a self, method: &'a str, params: Value) -> HostFuture<'a> {
        Box::pin(async move {
            if method == "http.request" {
                http(&self.http, params).await
            } else {
                self.telegram(
                    method
                        .strip_prefix("telegram.")
                        .context("unknown host method")?,
                    params,
                )
                .await
            }
        })
    }
    fn redact(&self, error: String) -> String {
        tracing::warn!(error=%self.logs.redact(error), "module host operation failed");
        "host operation failed (see logs)".into()
    }
}
fn message_id(params: &Value) -> Result<i32> {
    let id = params
        .get("message_id")
        .and_then(Value::as_i64)
        .context("message_id missing")?;
    ensure!(id > 0 && id <= i32::MAX as i64, "invalid message_id");
    Ok(id as i32)
}
fn validate_telegram_params(operation: &str, params: &Value) -> Result<()> {
    let object = params
        .as_object()
        .context("Telegram parameters must be an object")?;
    if let Some(bot) = object.get("bot") {
        ensure!(bot.is_boolean(), "bot must be boolean");
    }
    let fields: &[&str] = match operation {
        "get_me" | "get_chat" => &[],
        "history" => &["limit"],
        "get_messages" | "delete_messages" => &["ids"],
        "get_reply" | "download_file" => &["message_id"],
        "send_message" => &["text", "html", "reply_to"],
        "edit_message" => &["message_id", "text", "html", "reply_to"],
        "send_media" => &["media", "text", "html", "reply_to"],
        "send_file" => &["name", "base64", "text", "html", "reply_to"],
        _ => bail!("unknown Telegram operation"),
    };
    ensure!(
        object
            .keys()
            .all(|key| matches!(key.as_str(), "chat" | "bot") || fields.contains(&key.as_str())),
        "unknown Telegram parameter"
    );
    if let Some(limit) = params.get("limit") {
        ensure!(
            limit.as_u64().is_some_and(|n| (1..=50).contains(&n)),
            "history limit must be 1–50"
        );
    }
    if let Some(text) = params.get("text") {
        ensure!(text.is_string(), "text must be a string");
    }
    if let Some(html) = params.get("html") {
        ensure!(html.is_boolean(), "html must be boolean");
    }
    if params.get("message_id").is_some() {
        message_id(params)?;
    }
    if params.get("ids").is_some() {
        ids(params)?;
    }
    match operation {
        "get_messages" | "delete_messages" => {
            ids(params)?;
        }
        "download_file" | "edit_message" => {
            message_id(params)?;
        }
        _ => {}
    }
    if matches!(operation, "send_message" | "edit_message") {
        ensure!(
            params
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.is_empty()),
            "text missing"
        );
    }
    Ok(())
}
fn ids(params: &Value) -> Result<Vec<i32>> {
    let ids: Vec<i32> = serde_json::from_value(params.get("ids").context("ids missing")?.clone())?;
    ensure!(
        !ids.is_empty() && ids.len() <= 50 && ids.iter().all(|id| *id > 0),
        "ids must contain 1–50 positive IDs"
    );
    Ok(ids)
}
fn input(params: &Value) -> Result<InputMessage> {
    let text = params
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let html = params.get("html").and_then(Value::as_bool).unwrap_or(false);
    let input = if html {
        InputMessage::new().html(text)
    } else {
        InputMessage::new().text(text)
    };
    let article = grammers_client::update::Article::new("", input.clone());
    if let tl::enums::InputBotInlineMessage::Text(t) = article.raw.send_message {
        ensure!(
            t.message.encode_utf16().count() <= 4096,
            "message exceeds Telegram text limit"
        );
    }
    let reply = params
        .get("reply_to")
        .and_then(Value::as_i64)
        .map(|id| i32::try_from(id).context("invalid reply ID"))
        .transpose()?;
    ensure!(reply.is_none_or(|id| id > 0), "invalid reply ID");
    Ok(input.reply_to(reply))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockHost {
        calls: Mutex<Vec<(String, Value)>>,
    }
    impl Transport for MockHost {
        fn call<'a>(&'a self, method: &'a str, params: Value) -> HostFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.into(), params));
                match method {
                    "http.request" => {
                        Ok(json!({"status":200,"ok":true,"body":"{\"temperature\":21}"}))
                    }
                    "telegram.send_message" => Ok(json!({"message_id":42})),
                    _ => Ok(json!(true)),
                }
            })
        }
    }
    fn package(source: &str) -> Package {
        let mut package: Package =
            serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
        package.source = source.into();
        package.manifest.capabilities = Capabilities {
            http: vec!["https://weather.example".into()],
            telegram: TelegramCapabilities {
                operations: vec!["send_message".into(), "history".into()],
                chats: vec!["current".into(), "me".into()],
                bot: false,
            },
        };
        package
    }
    async fn run(
        package: Package,
        mode: Mode,
        host: Arc<dyn Transport>,
    ) -> Result<crate::modules::Effect> {
        let capabilities = package.manifest.capabilities.clone();
        execute(
            capabilities,
            mode,
            tokio_util::sync::CancellationToken::new(),
            host,
            move |bridge| {
                package.run_command_with_host(
                    "hello",
                    "Moscow",
                    json!({}),
                    json!({}),
                    "en",
                    Some(bridge),
                )
            },
        )
        .await
    }
    #[tokio::test]
    async fn http_result_feeds_telegram_without_replaying_script() {
        let host = Arc::new(MockHost::default());
        let p = package(
            r#"
            fn hello(args, config) {
                let response = http_get("https://weather.example/current?city=" + args);
                let weather = json_parse(response.body);
                let sent = host_call("telegram.send_message", #{text: "Temperature: " + weather.temperature});
                #{text: sent.message_id.to_string(), state: #{last: weather.temperature}}
            }
        "#,
        );
        p.validate().unwrap();
        let effect = run(
            p,
            Mode {
                chat: Some(-123),
                message_id: Some(7),
                preview: false,
            },
            host.clone(),
        )
        .await
        .unwrap();
        assert_eq!(effect.text.as_deref(), Some("42"));
        assert_eq!(effect.state.unwrap()["last"], 21);
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].1["text"], "Temperature: 21");
    }
    #[tokio::test]
    async fn denied_capabilities_never_reach_transport() {
        for (source, mode) in [
            (
                r#"fn hello(a,c) { http_get("https://other.example/"); }"#,
                Mode::default(),
            ),
            (
                r#"fn hello(a,c) { host_call("telegram.send_message", #{chat:"me",text:"bad"}); }"#,
                Mode {
                    preview: true,
                    chat: None,
                    message_id: None,
                },
            ),
            (
                r#"fn hello(a,c) { host_call("telegram.send_message", #{chat:123,text:"bad"}); }"#,
                Mode {
                    chat: Some(456),
                    preview: false,
                    message_id: None,
                },
            ),
            (
                r#"fn hello(a,c) { host_call("telegram.send_message", #{chat:"me",bot:true,text:"bad"}); }"#,
                Mode::default(),
            ),
            (
                r#"fn hello(a,c) { host_call("telegram.invoke", #{chat:"me"}); }"#,
                Mode::default(),
            ),
            (
                r#"fn hello(a,c) { host_call("telegram.history", #{}); }"#,
                Mode::default(),
            ),
            (
                r#"fn hello(a,c) { host_call("http.request", #{url:"https://weather.example/",method:"POST"}); }"#,
                Mode {
                    preview: true,
                    chat: None,
                    message_id: None,
                },
            ),
        ] {
            let host = Arc::new(MockHost::default());
            assert!(run(package(source), mode, host.clone()).await.is_err());
            assert!(host.calls.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn preview_allows_http_get_and_read_only_telegram() {
        let host = Arc::new(MockHost::default());
        let p = package(
            r#"fn hello(a,c) { http_get("https://weather.example/"); host_call("telegram.history", #{chat:"me",limit:1}); "ok" }"#,
        );
        assert_eq!(
            run(
                p,
                Mode {
                    preview: true,
                    chat: None,
                    message_id: None,
                },
                host.clone()
            )
            .await
            .unwrap()
            .text
            .as_deref(),
            Some("ok")
        );
        assert_eq!(host.calls.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn budget_counts_even_if_script_catches_errors() {
        let host = Arc::new(MockHost::default());
        let p = package(
            r#"fn hello(a,c) { for n in 0..20 { try { http_get("https://weather.example/"); } catch(e) {} } "done" }"#,
        );
        run(p, Mode::default(), host.clone()).await.unwrap();
        assert_eq!(host.calls.lock().unwrap().len(), MAX_CALLS);
    }
    struct HangingHost(tokio::sync::Notify);
    impl Transport for HangingHost {
        fn call<'a>(&'a self, _: &'a str, _: Value) -> HostFuture<'a> {
            Box::pin(async move {
                self.0.notify_one();
                std::future::pending().await
            })
        }
    }
    #[tokio::test]
    async fn cancellation_releases_waiting_worker_without_blocking_runtime() {
        let p = package(r#"fn hello(a,c) { http_get("https://weather.example/") }"#);
        let stop = tokio_util::sync::CancellationToken::new();
        let cancel = stop.clone();
        let host = Arc::new(HangingHost(tokio::sync::Notify::new()));
        let task = tokio::spawn(execute(
            p.manifest.capabilities.clone(),
            Mode::default(),
            stop,
            host.clone(),
            move |bridge| {
                p.run_command_with_host("hello", "", json!({}), json!({}), "en", Some(bridge))
            },
        ));
        tokio::time::timeout(Duration::from_secs(2), host.0.notified())
            .await
            .unwrap();
        cancel.cancel();
        let error = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
    }
    #[test]
    fn origins_and_requests_are_exact_and_bounded() {
        let p = package("");
        for url in [
            "http://weather.example/",
            "https://weather.example.evil/",
            "https://user:pass@weather.example/",
            "https://weather.example:8443/",
            "https://weather.example/#secret",
        ] {
            assert!(
                p.manifest
                    .capabilities
                    .authorize("http.request", &json!({"url":url}), &Mode::default())
                    .is_err()
            );
        }
        assert!(
            p.manifest
                .capabilities
                .authorize(
                    "http.request",
                    &json!({"url":"https://weather.example/a","headers":{"Host":"evil"}}),
                    &Mode::default()
                )
                .is_err()
        );
        assert!(
            p.manifest
                .capabilities
                .authorize(
                    "http.request",
                    &json!({"url":"https://weather.example/a"}),
                    &Mode::default()
                )
                .is_ok()
        );
        let mut caps = p.manifest.capabilities;
        caps.http = vec!["https://weather.example/path".into()];
        assert!(caps.validate().is_err());
        caps.http.clear();
        caps.telegram.operations.push("invoke".into());
        assert!(caps.validate().is_err());
    }
    #[test]
    fn old_package_serialization_does_not_gain_empty_capabilities() {
        let p: Package =
            serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
        assert!(p.manifest.capabilities.is_empty());
        assert!(
            serde_json::to_value(p).unwrap()["manifest"]
                .get("capabilities")
                .is_none()
        );
    }
    #[tokio::test]
    async fn binary_and_text_response_limits_apply_to_actual_streamed_bodies() {
        use axum::http::Response;
        let response: reqwest::Response = Response::builder()
            .status(418)
            .body(vec![0, 255, 1])
            .unwrap()
            .into();
        let value = read_http_response(response, "base64").await.unwrap();
        assert_eq!(value["body"], "AP8B");
        assert_eq!(value["status"], 418);
        assert_eq!(value["ok"], false);
        let response: reqwest::Response = Response::builder().body(vec![255]).unwrap().into();
        assert!(read_http_response(response, "text").await.is_err());
        let response: reqwest::Response = Response::builder()
            .body(vec![42; 64 * 1024 + 1])
            .unwrap()
            .into();
        assert!(read_http_response(response, "base64").await.is_err());
    }
    #[tokio::test]
    async fn chunked_http_body_cannot_bypass_size_limit() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            let mut reply =
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                    .to_vec();
            for _ in 0..17 {
                reply.extend_from_slice(b"1000\r\n");
                reply.extend(vec![b'x'; 4096]);
                reply.extend_from_slice(b"\r\n");
            }
            reply.extend_from_slice(b"0\r\n\r\n");
            let _ = socket.write_all(&reply).await;
        });
        let response = http_client()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert!(response.content_length().is_none());
        assert!(read_http_response(response, "text").await.is_err());
        server.await.unwrap();
    }
    #[tokio::test]
    async fn production_http_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 2048];
            assert!(socket.read(&mut bytes).await.unwrap() > 0);
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{address}/unapproved\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        // Only this transport test talks HTTP to loopback; host calls require HTTPS.
        let response = http_client()
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 302);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn examples_validate_and_host_context_reaches_event_handlers() {
        for source in [
            include_str!("../examples/weather.module.json"),
            include_str!("../examples/telegram_host.module.json"),
        ] {
            let p: Package = serde_json::from_str(source).unwrap();
            p.validate().unwrap();
        }
        let p = package(
            r#"fn callback(event, config) { let ctx = invocation(); #{text: (ctx.chat + ctx.message_id).to_string()} }"#,
        );
        let effect = execute(
            p.manifest.capabilities.clone(),
            Mode {
                chat: Some(123),
                message_id: Some(4),
                preview: false,
            },
            Default::default(),
            Arc::new(MockHost::default()),
            move |bridge| {
                p.run_event_with_host(
                    "callback",
                    json!({}),
                    json!({}),
                    json!({}),
                    "en",
                    Some(bridge),
                )
            },
        )
        .await
        .unwrap();
        assert_eq!(effect.text.as_deref(), Some("127"));
    }
    #[tokio::test]
    async fn capabilities_are_part_of_exact_revision_approval() {
        let root = tempfile::tempdir().unwrap();
        let mut registry = crate::modules::Registry::load(root.path(), 1).unwrap();
        let p = package("fn hello(a,c) { \"ok\" }");
        let first = registry.stage(p.clone()).unwrap();
        let mut more = p;
        more.manifest.capabilities.telegram.bot = true;
        let second = registry.stage(more).unwrap();
        assert_ne!(first, second);
        registry.approve(&first).unwrap();
        assert!(
            !registry
                .command("hello")
                .unwrap()
                .0
                .manifest
                .capabilities
                .telegram
                .bot
        );
        registry.approve(&second).unwrap();
        assert!(
            registry
                .command("hello")
                .unwrap()
                .0
                .manifest
                .capabilities
                .telegram
                .bot
        );
    }
    struct FailingHost;
    impl Transport for FailingHost {
        fn call<'a>(&'a self, _: &'a str, _: Value) -> HostFuture<'a> {
            Box::pin(async { bail!("remote error containing SECRET") })
        }
    }
    #[tokio::test]
    async fn transport_errors_do_not_leak_secrets_to_scripts() {
        let p = package(
            r#"fn hello(a,c) { try { http_get("https://weather.example/"); } catch(e) { return e; } }"#,
        );
        let text = run(p, Mode::default(), Arc::new(FailingHost))
            .await
            .unwrap()
            .text
            .unwrap();
        assert!(text.contains("host operation failed"));
        assert!(!text.contains("SECRET"));
    }
    #[test]
    fn invalid_telegram_parameters_fail_before_transport() {
        for (method, params) in [
            ("send_message", json!({"bot":"true","text":"hi"})),
            ("send_message", json!({"text":"hi","allow_paid_stars":5})),
            ("get_messages", json!({"ids":[0]})),
            ("download_file", json!({"message_id":-1})),
            ("history", json!({"limit":-1})),
            ("edit_message", json!({"message_id":1})),
        ] {
            assert!(validate_telegram_params(method, &params).is_err());
        }
    }
    #[tokio::test]
    async fn oversize_request_never_reaches_transport_and_response_is_bounded() {
        struct LargeHost;
        impl Transport for LargeHost {
            fn call<'a>(&'a self, _: &'a str, _: Value) -> HostFuture<'a> {
                Box::pin(async { Ok(json!("x".repeat(MAX_VALUE + 1))) })
            }
        }
        let p = package(r#"fn hello(a,c) { http_get("https://weather.example/"); }"#);
        let error = run(p, Mode::default(), Arc::new(LargeHost))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("host response too large"));
        let host = Arc::new(MockHost::default());
        let p = package(
            r#"fn hello(a,c) { host_call("http.request", #{url:"https://weather.example/",body:"x".repeat(100000)}); }"#,
        );
        assert!(run(p, Mode::default(), host.clone()).await.is_err());
        assert!(host.calls.lock().unwrap().is_empty());
    }
}
