//! Inline account login. Secrets never enter module watchers, callback data or persisted forms.
use crate::{
    auth::Login,
    commands::Context,
    config::{AccountConfig, Config, Credentials},
    forms::{Action, Key, Screen},
    session::MasterKey,
    telegram::Connection,
};
use anyhow::{Context as _, Result, ensure};
use grammers_client::{
    message::Message,
    peer::Peer,
    tl,
    update::{Article, CallbackQuery, Update},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const TTL: Duration = Duration::from_secs(600);
const INPUT_PREFIX: &str = "account-input:";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Confirm,
    Retry,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Confirm,
    Phone,
    Code,
    Password,
    Retry,
    Failed,
    Done,
}

/// Independent login client with an in-memory session; only successful login is exported.
struct Attempt {
    connection: Connection,
    login: Arc<Login>,
    phone: Zeroizing<String>,
}
struct Entry {
    target: i64,
    unit: Option<String>,
    step: Step,
    input: String,
    expires: Instant,
    attempt: Option<Attempt>,
}
impl Entry {
    fn allows(&self, user: i64) -> bool {
        self.target == user && self.expires > Instant::now()
    }
    fn rotate(&mut self, step: Step) {
        self.step = step;
        self.input = format!("{INPUT_PREFIX}{}", crate::web::random_token());
    }
}
#[derive(Default)]
struct State {
    entries: HashMap<String, Arc<tokio::sync::Mutex<Entry>>>,
    targets: HashMap<i64, String>,
}
struct Environment {
    credentials: Credentials,
    key: Arc<MasterKey>,
    proxy: Option<String>,
}
#[derive(Default)]
pub struct Accounts {
    state: Arc<Mutex<State>>,
    environment: Option<Environment>,
}
impl Accounts {
    pub fn new(credentials: Credentials, key: Arc<MasterKey>, proxy: Option<String>) -> Self {
        Self {
            state: Default::default(),
            environment: Some(Environment {
                credentials,
                key,
                proxy,
            }),
        }
    }
    fn get(&self, ticket: &str) -> Option<Arc<tokio::sync::Mutex<Entry>>> {
        self.state.lock().unwrap().entries.get(ticket).cloned()
    }
    fn remove(&self, ticket: &str) {
        let mut state = self.state.lock().unwrap();
        state.entries.remove(ticket);
        state.targets.retain(|_, value| value != ticket);
    }
    pub async fn shutdown(&self) {
        let entries = {
            let mut state = self.state.lock().unwrap();
            state.targets.clear();
            std::mem::take(&mut state.entries)
        };
        for entry in entries.into_values() {
            entry.lock().await.attempt = None;
        }
    }
    fn create(
        &self,
        target: i64,
        confirmed: bool,
        stop: CancellationToken,
    ) -> Result<(String, Arc<tokio::sync::Mutex<Entry>>)> {
        let ticket = crate::web::random_token();
        let mut entry = Entry {
            target,
            unit: None,
            step: Step::Confirm,
            input: String::new(),
            expires: Instant::now() + TTL,
            attempt: None,
        };
        if confirmed {
            entry.rotate(Step::Phone);
        }
        let entry = Arc::new(tokio::sync::Mutex::new(entry));
        {
            let mut state = self.state.lock().unwrap();
            if let Some(old) = state.targets.remove(&target) {
                state.entries.remove(&old);
            }
            ensure!(state.entries.len() < 32, "too many account login attempts");
            state.entries.insert(ticket.clone(), entry.clone());
            state.targets.insert(target, ticket.clone());
        }
        // Expiry does not rely on an input arriving or a cleaner tick. A stale
        // in-flight request cannot persist a session after its entry is removed.
        let state = Arc::downgrade(&self.state);
        let identity = ticket.clone();
        tokio::spawn(async move {
            tokio::select! { _ = stop.cancelled() => {}, _ = tokio::time::sleep(TTL) => {} }
            let removed = state.upgrade().and_then(|state| {
                let mut state = state.lock().unwrap();
                state.targets.retain(|_, value| value != &identity);
                state.entries.remove(&identity)
            });
            if let Some(entry) = removed {
                entry.lock().await.attempt = None;
            }
        });
        Ok((ticket, entry))
    }
}

fn tr(ctx: &Context, key: &str) -> String {
    crate::original::text(
        ctx.db.language().starts_with("en"),
        "ataraxia_accounts",
        key,
    )
}
fn permitted_host() -> bool {
    !std::env::var("HYSTERIA").is_ok_and(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "no" | "off"
        )
    })
}
fn key(text: String, action: Action) -> Key {
    Key {
        text,
        action: Some(action),
        url: None,
    }
}
fn screen(ctx: &Context, ticket: &str, entry: &Entry, text: &str) -> Screen {
    render(ctx.db.language().starts_with("en"), ticket, entry, text)
}
fn render(english: bool, ticket: &str, entry: &Entry, text: &str) -> Screen {
    let tr = |key| crate::original::text(english, "ataraxia_accounts", key);
    let button = match entry.step {
        Step::Phone => Some("enter_number"),
        Step::Code => Some("enter_code"),
        Step::Password => Some("enter_2fa"),
        _ => None,
    };
    let buttons = if let Some(label) = button {
        vec![vec![key(
            tr(label),
            Action::SwitchInline {
                query: format!("{} ", entry.input),
                same_chat: true,
            },
        )]]
    } else if entry.step == Step::Retry {
        vec![vec![key(
            tr("request_code"),
            Action::Account {
                ticket: ticket.into(),
                operation: Operation::Retry,
            },
        )]]
    } else if entry.step == Step::Failed {
        vec![vec![key(
            tr("btn_no"),
            Action::Account {
                ticket: ticket.into(),
                operation: Operation::Close,
            },
        )]]
    } else if entry.step == Step::Confirm {
        vec![vec![
            key(
                tr("btn_yes"),
                Action::Account {
                    ticket: ticket.into(),
                    operation: Operation::Confirm,
                },
            ),
            key(
                tr("btn_no"),
                Action::Account {
                    ticket: ticket.into(),
                    operation: Operation::Close,
                },
            ),
        ]]
    } else {
        vec![]
    };
    Screen {
        text: text.into(),
        html: true,
        buttons,
        ttl_seconds: 600,
        close_button: false,
        link_preview: false,
        ..Default::default()
    }
}
pub async fn command(ctx: &Context, message: &Message, args: &str) -> Result<()> {
    if !permitted_host() {
        return ctx.answer_html(message, &tr(ctx, "one_account")).await;
    }
    let target =
        crate::security_commands::resolve_user(ctx, message, args.split_whitespace().next()).await;
    let user = if let Ok(target) = target {
        let peer = grammers_session::types::PeerId::user(target).context("invalid user")?;
        ctx.client.resolve_peer(peer.to_ambient_ref()).await.ok()
    } else {
        None
    };
    let Some(Peer::User(user)) =
        user.filter(|peer| matches!(peer, Peer::User(user) if !user.is_bot()))
    else {
        return ctx.answer_html(message, &tr(ctx, "invalid_target")).await;
    };
    let target = user.id().bare_id_unchecked();
    let confirmed = target == ctx.owner || args.to_lowercase().contains("force_insecure");
    let (ticket, entry) = ctx.accounts.create(target, confirmed, ctx.stop.clone())?;
    let text = if confirmed {
        tr(ctx, "enter_number_format")
    } else {
        crate::original::format(
            &tr(ctx, "add_user_confirm"),
            &[
                crate::output::escape(user.first_name().unwrap_or("")),
                target.to_string(),
            ],
            &[],
        )
    };
    let body = screen(ctx, &ticket, &*entry.lock().await, &text);
    let bot = ctx.bot.read().await.clone();
    let result = if let Some(bot) = bot {
        bot.form_with_allow(
            ctx,
            message,
            body,
            if confirmed { vec![target] } else { vec![] },
        )
        .await
    } else {
        Err(anyhow::anyhow!("inline unavailable"))
    };
    match result {
        Ok(unit) => entry.lock().await.unit = Some(unit),
        Err(_) => {
            ctx.accounts.remove(&ticket);
            let text = if confirmed {
                tr(ctx, "login_timeout")
            } else {
                crate::original::format(
                    &tr(ctx, "add_user_insecure"),
                    &[
                        crate::output::escape(user.first_name().unwrap_or("")),
                        target.to_string(),
                        crate::output::escape(&ctx.db.prefix()),
                        target.to_string(),
                    ],
                    &[],
                )
            };
            ctx.answer_html(message, &text).await?;
        }
    }
    Ok(())
}
pub async fn callback(
    ctx: &Context,
    query: &CallbackQuery,
    unit: &str,
    ticket: &str,
    operation: Operation,
) -> Result<()> {
    let owners = crate::security::owners(&ctx.db, ctx.owner);
    let Some(entry) = ctx.accounts.get(ticket) else {
        query
            .answer()
            .alert(tr(ctx, "login_expired"))
            .send()
            .await?;
        return Ok(());
    };
    let mut entry = entry.lock().await;
    let user = query.sender_id().bare_id_unchecked();
    ensure!(
        owners.contains(&user) || operation != Operation::Confirm && entry.allows(user),
        "only an owner may confirm account login"
    );
    ensure!(entry.expires > Instant::now(), "login expired");
    ensure!(entry.unit.as_deref() == Some(unit), "account form mismatch");
    if operation == Operation::Close {
        entry.attempt = None;
        ctx.accounts.remove(ticket);
        ctx.forms.lock().unwrap().remove(unit);
        query
            .answer()
            .edit(grammers_client::message::InputMessage::new().text("✕"))
            .await?;
        return Ok(());
    }
    if !permitted_host() {
        query.answer().alert(tr(ctx, "one_account")).send().await?;
        return Ok(());
    }
    if operation == Operation::Retry {
        entry.attempt = None;
    }
    entry.rotate(Step::Phone);
    ctx.forms.lock().unwrap().allow_user(unit, entry.target)?;
    let body = ctx.forms.lock().unwrap().replace(
        unit,
        screen(ctx, ticket, &entry, &tr(ctx, "enter_number_format")),
    )?;
    if let Some(bot) = ctx.bot.read().await.clone() {
        bot.bot.edit_callback(query, body).await?;
    }
    query.answer().send().await?;
    Ok(())
}

fn normalize_code(value: &str, length: Option<i32>) -> std::result::Result<String, &'static str> {
    let code: String = value.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    if code.is_empty() || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err("code_digits_only");
    }
    if length.is_some_and(|n| n as usize != code.len()) {
        return Err("invalid_code");
    }
    Ok(code)
}
fn error_key(error: &anyhow::Error, step: Step) -> (&'static str, Option<u64>) {
    if let Some(grammers_client::InvocationError::Rpc(rpc)) =
        error.downcast_ref::<grammers_client::InvocationError>()
    {
        if rpc.name.starts_with("FLOOD_WAIT") {
            return ("floodwait_error", rpc.value.map(|n| n as u64));
        }
        return (
            match rpc.name.as_str() {
                "PHONE_NUMBER_INVALID" => "incorrect_number",
                "PHONE_CODE_EXPIRED" | "PHONE_CODE_HASH_EMPTY" => "code_expired",
                "PHONE_CODE_INVALID" => "invalid_code",
                "PASSWORD_HASH_INVALID" => "invalid_password",
                _ => "login_timeout",
            },
            None,
        );
    }
    if step == Step::Password && error.to_string() == "invalid password" {
        ("invalid_password", None)
    } else {
        ("login_timeout", None)
    }
}

async fn input(ctx: &Context, ticket: &str, entry: &mut Entry, value: &str) -> Result<String> {
    ensure!(
        entry.expires > Instant::now() && ctx.accounts.get(ticket).is_some(),
        "login expired"
    );
    if !permitted_host() {
        entry.attempt = None;
        return Ok(tr(ctx, "one_account"));
    }
    match entry.step {
        Step::Phone => {
            entry.attempt = None;
            let phone = value
                .chars()
                .filter(|c| !matches!(c, ' ' | '(' | ')' | '-'))
                .collect::<String>();
            if phone.len() > 24
                || phone.trim_start_matches('+').len() < 7
                || !phone
                    .trim_start_matches('+')
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            {
                entry.rotate(Step::Phone);
                return Ok(format!(
                    "{}{}",
                    tr(ctx, "incorrect_number"),
                    tr(ctx, "enter_number_format")
                ));
            }
            let env = ctx
                .accounts
                .environment
                .as_ref()
                .context("login services unavailable")?;
            let connection =
                Connection::temporary(&env.credentials, env.key.clone(), env.proxy.as_deref())?;
            let login = Arc::new(Login::new(
                connection.client.clone(),
                connection.session.clone(),
                env.credentials.clone(),
            ));
            entry.attempt = Some(Attempt {
                connection,
                login: login.clone(),
                phone: Zeroizing::new(phone.clone()),
            });
            login.phone(&phone).await?;
        }
        Step::Code => {
            let attempt = entry.attempt.as_ref().context("login expired")?;
            let length = attempt.login.view().await.length;
            let code = match normalize_code(value, length) {
                Ok(code) => Zeroizing::new(code),
                Err(key) => return Ok(tr(ctx, key)),
            };
            attempt.login.code(&code).await?;
        }
        Step::Password => {
            if value.is_empty() {
                return Ok(tr(ctx, "invalid_password"));
            }
            entry
                .attempt
                .as_ref()
                .context("login expired")?
                .login
                .password(value)
                .await?;
        }
        _ => anyhow::bail!("login input is not active"),
    }
    ensure!(
        entry.expires > Instant::now()
            && ctx.accounts.get(ticket).is_some()
            && !ctx.stop.is_cancelled(),
        "login expired"
    );
    let attempt = entry.attempt.as_ref().context("login expired")?;
    match attempt.login.view().await.stage {
        "code" => {
            entry.rotate(Step::Code);
            Ok(tr(ctx, "code_sent"))
        }
        "password" => {
            entry.rotate(Step::Password);
            Ok(tr(ctx, "2fa_enabled"))
        }
        "done" => {
            let user = attempt.connection.client.get_me().await?;
            ensure!(!user.is_bot(), "not a personal account");
            let owner = user.id().bare_id_unchecked();
            let _lock = crate::updater::MAINTENANCE.lock().await;
            let mut config = Config::load(&ctx.root)?;
            if config
                .accounts
                .values()
                .any(|account| account.owner_id == Some(owner))
            {
                // This is a fresh independent authorization, not the configured session.
                let _ = attempt.connection.client.sign_out().await;
                entry.attempt = None;
                entry.rotate(Step::Phone);
                return Ok(tr(ctx, "invalid_target"));
            }
            ensure!(
                permitted_host()
                    && entry.expires > Instant::now()
                    && ctx.accounts.get(ticket).is_some(),
                "login expired"
            );
            ctx.logs.add_secret(attempt.phone.to_string());
            {
                // Replacement/expiry and publication are mutually exclusive. A
                // request that completed after a new attempt must not add an account.
                let registry = ctx.accounts.state.lock().unwrap();
                ensure!(
                    registry.entries.contains_key(ticket)
                        && entry.expires > Instant::now()
                        && !ctx.stop.is_cancelled(),
                    "login expired"
                );
                publish(&ctx.root, &mut config, owner, &attempt.connection.session)?;
            }
            entry.rotate(Step::Done);
            entry.attempt = None;
            ctx.db.set("RustRuntime", "restart", true)?;
            Ok(tr(ctx, "login_successful"))
        }
        _ => {
            entry.rotate(Step::Phone);
            Ok(tr(ctx, "code_expired"))
        }
    }
}

/// Caller serializes configuration updates. No failed/incomplete login creates an account slot.
fn publish(
    root: &std::path::Path,
    config: &mut Config,
    owner: i64,
    session: &crate::session::EncryptedSession,
) -> Result<()> {
    ensure!(
        !config
            .accounts
            .values()
            .any(|account| account.owner_id == Some(owner)),
        "account already configured"
    );
    let slot = format!("inline_{owner}");
    ensure!(!config.accounts.contains_key(&slot), "account slot exists");
    let path = root.join(format!("atarustia-user-{slot}.atsession"));
    session.export(&path)?;
    config.accounts.insert(
        slot.clone(),
        AccountConfig {
            owner_id: Some(owner),
            bot_token: None,
            needs_login: false,
        },
    );
    if let Err(error) = config.save(root) {
        config.accounts.remove(&slot);
        let _ = std::fs::remove_file(&path);
        return Err(error);
    }
    Ok(())
}

pub async fn handle_update(ctx: &Context, update: &Update) -> Result<bool> {
    let (query, user, chosen) = match update {
        Update::InlineQuery(query) if query.text().starts_with(INPUT_PREFIX) => {
            (query.text(), query.sender_id().bare_id_unchecked(), false)
        }
        _ => match update.raw() {
            tl::enums::Update::BotInlineSend(sent) if sent.query.starts_with(INPUT_PREFIX) => {
                (sent.query.as_str(), sent.user_id, true)
            }
            _ => return Ok(false),
        },
    };
    let (token, value) = query.split_once(' ').unwrap_or((query, ""));
    let candidates: Vec<_> = ctx
        .accounts
        .state
        .lock()
        .unwrap()
        .entries
        .iter()
        .map(|(ticket, entry)| (ticket.clone(), entry.clone()))
        .collect();
    for (ticket, entry) in candidates {
        let Ok(mut entry) = entry.try_lock() else {
            continue;
        };
        if entry.input != token || !entry.allows(user) {
            continue;
        }
        if !chosen {
            if let Update::InlineQuery(query) = update {
                let title = tr(
                    ctx,
                    match entry.step {
                        Step::Code => "login_code",
                        Step::Password => "your_2fa",
                        _ => "your_phone_number",
                    },
                );
                let transfer = if user == ctx.owner {
                    "🔄 <b>Transferring value to userbot...</b>\n<i>This message will be deleted automatically</i>"
                } else {
                    "🔄 <b>Transferring value to userbot...</b>"
                };
                query
                    .answer([Article::new(title, crate::emoji::input(transfer))])
                    .private()
                    .cache_time(0)
                    .send()
                    .await?;
            }
            return Ok(true);
        }
        let value = Zeroizing::new(value.to_owned());
        let previous = entry.step;
        // Consume the token before awaiting Telegram: duplicates and replay cannot repeat RPCs.
        entry.input.clear();
        let remaining = entry
            .expires
            .saturating_duration_since(Instant::now())
            .min(Duration::from_secs(65));
        let result = tokio::select! {
            _ = ctx.stop.cancelled() => Err(anyhow::anyhow!("login stopped")),
            result = tokio::time::timeout(remaining, input(ctx, &ticket, &mut entry, &value)) => result.unwrap_or_else(|_| Err(anyhow::anyhow!("login timed out"))),
        };
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                let (key, seconds) = error_key(&error, previous);
                if key == "code_expired" {
                    entry.attempt = None;
                    entry.rotate(Step::Retry);
                } else if key == "incorrect_number" {
                    entry.attempt = None;
                    entry.rotate(Step::Phone);
                } else if !matches!(key, "invalid_code" | "invalid_password") {
                    entry.attempt = None;
                    entry.rotate(Step::Failed);
                } else {
                    entry.rotate(previous);
                }
                let text = crate::original::format(
                    &tr(ctx, key),
                    &[seconds.unwrap_or(0).to_string()],
                    &[],
                );
                if key == "incorrect_number" {
                    text + tr(ctx, "enter_number_format").as_str()
                } else {
                    text
                }
            }
        };
        if entry.input.is_empty() && entry.step != Step::Done {
            entry.rotate(previous);
        }
        let done = entry.step == Step::Done;
        if let Some(unit) = &entry.unit {
            let body = screen(ctx, &ticket, &entry, &text);
            // Report failures without echoing Telegram requests, phone, code or password.
            if crate::config_ui::edit_unit(ctx, unit, body).await.is_err() {
                tracing::warn!("account login screen could not be edited");
            }
        }
        if done {
            ctx.accounts.remove(&ticket);
            tokio::time::sleep(Duration::from_secs(1)).await;
            ctx.stop.cancel();
        }
        return Ok(true);
    }
    // Reserved credential queries, including stale/forged ones, never reach watchers.
    if let Update::InlineQuery(query) = update {
        query
            .answer(Vec::<Article>::new())
            .private()
            .cache_time(0)
            .send()
            .await?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(step: Step) -> Entry {
        let mut entry = Entry {
            target: 42,
            unit: None,
            step,
            input: String::new(),
            expires: Instant::now() + TTL,
            attempt: None,
        };
        entry.rotate(step);
        entry
    }

    #[test]
    fn login_codes_accept_separators_but_not_unicode_digits_or_wrong_length() {
        assert_eq!(normalize_code("12 3-45", Some(5)).unwrap(), "12345");
        assert_eq!(
            normalize_code("１２３４５", Some(5)),
            Err("code_digits_only")
        );
        assert_eq!(normalize_code("", None), Err("code_digits_only"));
        assert_eq!(normalize_code("1234", Some(5)), Err("invalid_code"));
    }

    #[test]
    fn input_is_target_bound_expires_and_rotates_without_storing_credentials() {
        let mut entry = entry(Step::Phone);
        assert!(entry.allows(42));
        assert!(!entry.allows(1));
        let old = entry.input.clone();
        entry.rotate(Step::Code);
        assert_ne!(old, entry.input);
        entry.expires = Instant::now() - Duration::from_secs(1);
        assert!(!entry.allows(42));
    }

    #[test]
    fn original_login_screens_have_exact_labels_and_no_secret_values() {
        for english in [false, true] {
            for (step, label) in [
                (Step::Phone, "enter_number"),
                (Step::Code, "enter_code"),
                (Step::Password, "enter_2fa"),
            ] {
                let state = entry(step);
                let screen = render(english, "opaque-ticket", &state, "status");
                screen.validate().unwrap();
                assert_eq!(screen.buttons.len(), 1);
                assert_eq!(
                    screen.buttons[0][0].text,
                    crate::original::text(english, "ataraxia_accounts", label)
                );
                assert!(matches!(&screen.buttons[0][0].action,
                    Some(Action::SwitchInline {query, same_chat:true}) if query.starts_with(INPUT_PREFIX)));
                assert!(!screen.close_button);
                assert!(
                    !serde_json::to_string(&screen)
                        .unwrap()
                        .contains("password-value")
                );
                // External packages cannot fabricate native account-control actions.
                let confirm = render(english, "opaque-ticket", &entry(Step::Confirm), "warning");
                assert!(confirm.restrict_module("untrusted").is_err());
            }
        }
    }

    #[test]
    fn expired_code_offers_retry_and_transport_errors_only_offer_close() {
        let retry = render(true, "ticket", &entry(Step::Retry), "expired");
        assert!(matches!(
            retry.buttons[0][0].action,
            Some(Action::Account {
                operation: Operation::Retry,
                ..
            })
        ));
        let failed = render(true, "ticket", &entry(Step::Failed), "timeout");
        assert!(matches!(
            failed.buttons[0][0].action,
            Some(Action::Account {
                operation: Operation::Close,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn replacing_attempts_and_shutdown_invalidate_old_tickets() {
        let accounts = Accounts::default();
        let stop = CancellationToken::new();
        let (old, old_entry) = accounts.create(42, true, stop.clone()).unwrap();
        let (new, _) = accounts.create(42, true, stop.clone()).unwrap();
        assert!(accounts.get(&old).is_none());
        assert!(accounts.get(&new).is_some());
        assert_eq!(old_entry.lock().await.target, 42);
        accounts.shutdown().await;
        assert!(accounts.get(&new).is_none());
        stop.cancel();
    }

    #[tokio::test]
    async fn account_cancellation_cleans_registry_without_waiting_for_next_message() {
        let accounts = Accounts::default();
        let stop = CancellationToken::new();
        let (ticket, _) = accounts.create(42, false, stop.clone()).unwrap();
        stop.cancel();
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(accounts.get(&ticket).is_none());
    }

    #[tokio::test]
    async fn publish_adds_only_authorized_encrypted_session_and_preserves_existing_accounts() {
        use grammers_session::Session;
        let root = tempfile::tempdir().unwrap();
        let key = Arc::new(MasterKey::load(root.path()).unwrap());
        let session = crate::session::EncryptedSession::temporary(key.clone());
        session.set_home_dc_id(4).await.unwrap();
        let mut config = Config::default();
        config.accounts.insert(
            "default".into(),
            AccountConfig {
                owner_id: Some(1),
                bot_token: Some("synthetic-token".into()),
                needs_login: false,
            },
        );
        config.save(root.path()).unwrap();
        publish(root.path(), &mut config, 42, &session).unwrap();
        let path = root.path().join("atarustia-user-inline_42.atsession");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"ATRUST1\0"));
        let restored = Config::load(root.path()).unwrap();
        assert_eq!(
            restored.accounts["default"].bot_token.as_deref(),
            Some("synthetic-token")
        );
        assert_eq!(restored.accounts["inline_42"].owner_id, Some(42));
        assert!(!restored.accounts["inline_42"].needs_login);
        assert!(publish(root.path(), &mut config, 42, &session).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            crate::session::EncryptedSession::load(path, key)
                .unwrap()
                .home_dc_id()
                .unwrap(),
            4
        );
    }

    #[tokio::test]
    async fn chosen_input_is_target_bound_consumed_once_and_never_reaches_modules() {
        let root = tempfile::tempdir().unwrap();
        let key = Arc::new(MasterKey::load(root.path()).unwrap());
        let credentials = Credentials {
            api_id: 123,
            api_hash: "0".repeat(32),
        };
        let connection = Connection::temporary(&credentials, key, None).unwrap();
        let ctx = Context {
            client: connection.client.clone(),
            db: Arc::new(crate::database::Database::open(root.path(), 1).unwrap()),
            modules: Mutex::new(crate::modules::Registry::load(root.path(), 1).unwrap()),
            owner: 1,
            username: Some("owner".into()),
            root: root.path().into(),
            start: Instant::now(),
            allow_shell: false,
            stop: Default::default(),
            web: None,
            secrets: vec![],
            rate: Default::default(),
            semaphore: Arc::new(tokio::sync::Semaphore::new(32)),
            seen: Default::default(),
            logs: Default::default(),
            access: Default::default(),
            events: Default::default(),
            maintenance: Default::default(),
            account: "test".into(),
            forms: Default::default(),
            bot: Default::default(),
            jobs: Default::default(),
            api_limits: Default::default(),
            native: Default::default(),
            accounts: Default::default(),
        };
        let (ticket, entry) = ctx.accounts.create(42, true, ctx.stop.clone()).unwrap();
        let token = entry.lock().await.input.clone();
        let chosen = |user, value: &str| {
            Update::Raw(grammers_client::update::Raw {
                raw: tl::types::UpdateBotInlineSend {
                    user_id: user,
                    query: format!("{token} {value}"),
                    geo: None,
                    id: "synthetic".into(),
                    msg_id: None,
                }
                .into(),
                state: grammers_session::updates::State {
                    date: 0,
                    seq: 0,
                    message_box: None,
                },
            })
        };
        assert!(
            handle_update(&ctx, &chosen(7, "private-value"))
                .await
                .unwrap()
        );
        assert_eq!(entry.lock().await.input, token);
        assert!(
            handle_update(&ctx, &chosen(42, "invalid-phone"))
                .await
                .unwrap()
        );
        let next = entry.lock().await.input.clone();
        assert_ne!(next, token);
        assert!(entry.lock().await.attempt.is_none());
        assert!(
            handle_update(&ctx, &chosen(42, "invalid-phone"))
                .await
                .unwrap()
        );
        assert_eq!(entry.lock().await.input, next);
        assert_eq!(
            ctx.accounts.get(&ticket).unwrap().lock().await.step,
            Step::Phone
        );
        assert!(!root.path().join("config.json").exists());
        ctx.accounts.shutdown().await;
        ctx.stop.cancel();
        connection.close().await.unwrap();
    }

    #[test]
    fn publication_failure_does_not_leave_slot_or_session() {
        let root = tempfile::tempdir().unwrap();
        let key = Arc::new(MasterKey::load(root.path()).unwrap());
        let session = crate::session::EncryptedSession::temporary(key);
        std::fs::create_dir(root.path().join("config.json")).unwrap();
        let mut config = Config::default();
        assert!(publish(root.path(), &mut config, 42, &session).is_err());
        assert!(config.accounts.is_empty());
        assert!(
            !root
                .path()
                .join("atarustia-user-inline_42.atsession")
                .exists()
        );
    }
}
