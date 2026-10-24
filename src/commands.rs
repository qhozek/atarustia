use crate::{
    backup,
    database::Database,
    modules::Registry,
    security::{self, Grant, Permission},
    telegram,
    web::WebState,
};
use anyhow::{Context as _, Result, bail, ensure};
use grammers_client::{Client, message::Message};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub const BUILTINS: &[(&str, &str)] = &[
    ("logs", "отправить буфер логов в Избранное"),
    ("clearlogs", "очистить буфер логов"),
    ("dllangpack", "<https-url> — JSON/YAML языковой пакет"),
    (
        "ch_ataraxia_bot",
        "[username] — создать/выбрать inline-бота",
    ),
    ("ch_bot_token", "<token> — заменить токен и перезапустить"),
    (
        "addacc",
        "<slot> — добавить аккаунт через защищённый веб-вход",
    ),
    (
        "update",
        "[-f|full_commit_hash] — обновление исходников с подтверждением",
    ),
    (
        "restoremods",
        "в ответ на backup JSON — подготовить модули к одобрению",
    ),
    ("clearmodules", "confirm — удалить все внешние модули"),
    ("settings", "меню NoNick, Grep, InlineLogs и обслуживания"),
    (
        "help",
        "[module|command] [-f|-c|-l] — каталог и карточки модулей",
    ),
    ("info", "информация о сборке"),
    ("ataraxia", "информация о платформе"),
    ("ping", "проверить отклик"),
    ("setprefix", "<prefix> — сменить префикс"),
    ("setlang", "ru|en — язык"),
    ("aliases", "список алиасов"),
    ("addalias", "<alias> <command> — добавить алиас"),
    ("delalias", "<alias> — удалить алиас"),
    ("config", "[модуль] [параметр] — конфигурация"),
    (
        "fconfig",
        "<модуль> <параметр> <значение> — изменить конфигурацию",
    ),
    ("dlm", "<https-url> — скачать пакет для проверки"),
    ("lm", "в ответ на JSON-пакет — подготовить модуль"),
    ("ulm", "<module> — удалить модуль"),
    ("ml", "<module> — отправить исходники"),
    (
        "e",
        "<Rhai expression> — вычислить выражение без доступа к ОС",
    ),
    ("terminal", "<shell> — команда ОС (нужен --allow-shell)"),
    ("terminate", "в ответ на команду — остановить выполнение"),
    (
        "suspend",
        "<seconds> — приостановить обработку команд аккаунта",
    ),
    (
        "api_fw_protection",
        "on|off — асинхронный ограничитель обработки команд",
    ),
    (
        "suspend_api_protect",
        "<seconds> — временно приостановить ограничитель",
    ),
    ("ec", "<C source> — gcc"),
    ("ecpp", "<C++ source> — g++"),
    ("enode", "<JavaScript> — node"),
    ("security", "<command> owner|private|group|everyone — права"),
    ("ownerlist", "список владельцев"),
    ("owneradd", "<id> — добавить владельца"),
    ("ownerrm", "<id> — удалить владельца"),
    ("tsec", "<id> <command> <seconds> — временный доступ"),
    (
        "backupall",
        "БД и модули: локальная копия + файл в Избранное",
    ),
    ("backupdb", "резервная копия БД"),
    ("backupmods", "резервная копия модулей"),
    ("restoredb", "в ответ на backup JSON — восстановить БД"),
    (
        "restoreall",
        "в ответ на backup JSON — восстановить БД и подготовить модули к одобрению",
    ),
    (
        "set_backup_period",
        "<hours|0> — период автоматических копий",
    ),
    ("weburl", "одноразовая ссылка, только в Избранном"),
    ("source", "исходный проект и версия порта"),
    ("restart", "[-f] — перезапуск с подтверждением"),
];
pub fn is_builtin(name: &str) -> bool {
    BUILTINS
        .iter()
        .chain(crate::security_commands::COMMANDS)
        .chain(crate::preferences::COMMANDS)
        .any(|(n, _)| *n == name)
}
pub struct Context {
    pub client: Client,
    pub db: Arc<Database>,
    pub modules: Mutex<Registry>,
    pub owner: i64,
    pub username: Option<String>,
    pub root: PathBuf,
    pub start: Instant,
    pub allow_shell: bool,
    pub stop: CancellationToken,
    pub web: Option<Arc<WebState>>,
    pub secrets: Vec<String>,
    pub rate: Mutex<crate::dispatcher::RateLimit>,
    pub semaphore: Arc<tokio::sync::Semaphore>,
    pub seen: Mutex<crate::dispatcher::Seen>,
    pub logs: crate::logs::LogBuffer,
    pub access: crate::access::AccessCache,
    pub events: tokio::sync::Mutex<()>,
    pub maintenance: tokio::sync::Mutex<()>,
    pub account: String,
    pub forms: Mutex<crate::forms::Forms>,
    pub bot: tokio::sync::RwLock<Option<crate::inline::BotHandle>>,
    pub jobs: crate::jobs::Jobs,
    pub api_limits: crate::api_protection::Limiter,
    pub native: Arc<crate::sdk::Registry>,
}
impl Context {
    pub fn services(&self) -> crate::sdk::Services {
        crate::sdk::Services {
            client: self.client.clone(),
            db: self.db.clone(),
            owner: self.owner,
            stop: self.stop.clone(),
        }
    }
    pub fn tr(&self, key: &str, values: &[(&str, String)]) -> String {
        crate::i18n::text(&self.db, key, values)
    }
    pub fn original(&self, module: &str, key: &str, values: &[String]) -> String {
        crate::original::message(
            self.db.language().starts_with("en"),
            module,
            key,
            values,
            &[],
        )
    }
    pub(crate) async fn answer_html(&self, message: &Message, text: &str) -> Result<()> {
        let body = crate::update_ui::user_body(
            &crate::forms::Screen {
                text: text.into(),
                ..Default::default()
            },
            self.db
                .get("RustRuntime", "premium")
                .as_bool()
                .unwrap_or(false),
        )?;
        if message.outgoing() {
            message.edit(body).await?;
        } else {
            message.reply(body).await?;
        }
        Ok(())
    }
    pub fn permission(&self, name: &str) -> Permission {
        self.permission_with_default(name, Permission::Owner)
    }
    pub fn permission_with_default(&self, name: &str, default: Permission) -> Permission {
        let permission = Permission::from_value(
            self.db
                .get("atarustia.security", "masks")
                .get(name)
                .cloned()
                .unwrap_or_else(|| serde_json::to_value(default).unwrap()),
        )
        .unwrap_or(Permission::Owner);
        security::effective(&self.db, permission)
    }
    pub fn help(&self) -> String {
        crate::screens::help(self, None)
            .map(|s| s.text)
            .unwrap_or_else(|_| self.original("ataraxia_info", "nothing", &[]))
    }
    pub async fn handle(self: &Arc<Self>, message: Message, edited: bool) -> Result<()> {
        if !edited
            && message.outgoing()
            && message
                .text()
                .contains("This message will be deleted automatically")
        {
            let bot = self.bot.read().await.as_ref().map(|b| b.bot.id);
            if bot.is_some() && message.via_bot_id() == bot {
                message.delete().await?;
                return Ok(());
            }
        }
        if !edited && crate::config_ui::handle_reply(self, &message).await? {
            return Ok(());
        }
        // Channel posts/edits do not authenticate the editor. Never trust `outgoing` there.
        if message.peer_id().kind() == grammers_session::types::PeerKind::Channel {
            if message
                .sender_id()
                .is_none_or(|s| s.kind() != grammers_session::types::PeerKind::User)
            {
                return Ok(());
            }
            if edited {
                return Ok(());
            }
        }
        if message.forward_header().is_some() || message.via_bot_id().is_some() {
            return Ok(());
        }
        let prefix = self.db.prefix_for(
            message.sender_id().map(|s| s.bare_id_unchecked()),
            self.owner,
        );
        let mut parsed = crate::dispatcher::parse(
            message.text(),
            &prefix,
            self.username.as_deref(),
            message.outgoing(),
        );
        if matches!(parsed, crate::dispatcher::Parsed::Ignore) {
            let translated = crate::dispatcher::switch_layout(message.text());
            parsed = crate::dispatcher::parse(
                &translated,
                &prefix,
                self.username.as_deref(),
                message.outgoing(),
            );
        }
        let (name, args) = match parsed {
            crate::dispatcher::Parsed::Ignore => return Ok(()),
            crate::dispatcher::Parsed::Escape(text) => {
                if !edited {
                    self.seen.lock().unwrap().insert(
                        message.peer_id().bot_api_dialog_id_unchecked(),
                        message.id(),
                    );
                    message.edit(text).await?;
                }
                return Ok(());
            }
            crate::dispatcher::Parsed::Command { name, args } => (name, args),
        };
        let aliases: HashMap<String, String> =
            serde_json::from_value(self.db.get("Settings", "aliases")).unwrap_or_default();
        let (name, args) = crate::dispatcher::resolve(&name, &args, &aliases)?;
        let (name, args) = if !is_builtin(&name)
            && self.modules.lock().unwrap().command(&name).is_none()
        {
            let switched = crate::dispatcher::switch_layout(&name);
            if is_builtin(&switched) || self.modules.lock().unwrap().command(&switched).is_some() {
                (switched, args)
            } else {
                (name, args)
            }
        } else {
            (name, args)
        };
        let (args, grep) = if self
            .db
            .get("atarustia.main", "grep")
            .as_bool()
            .unwrap_or(false)
        {
            crate::dispatcher::grep(&args)
        } else {
            (args, None)
        };
        if name != "suspend"
            && self
                .db
                .get("RustRuntime", "paused_until")
                .as_u64()
                .unwrap_or(0)
                > security::now()
        {
            return Ok(());
        }
        let module = self.modules.lock().expect("modules lock").command(&name);
        let native = self.native.command(&name);
        if !is_builtin(&name) && module.is_none() && native.is_none() {
            return Ok(());
        }
        let user = message
            .sender_id()
            .filter(|id| id.kind() == grammers_session::types::PeerKind::User)
            .map(|id| id.bare_id_unchecked());
        let chat = message.peer_id().bot_api_dialog_id_unchecked();
        let permission = if let Some((_, c)) = &module {
            Permission::from_value(
                self.db
                    .get("atarustia.security", "masks")
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| serde_json::to_value(c.permission).unwrap()),
            )
            .unwrap_or(Permission::Owner)
        } else {
            native
                .as_ref()
                .map(|(_, c)| self.permission_with_default(&name, c.permission))
                .unwrap_or_else(|| self.permission(&name))
        };
        let module_name = module
            .as_ref()
            .map(|(p, _)| p.manifest.name.as_str())
            .or_else(|| native.as_ref().map(|(m, _)| m.name()));
        if !self
            .access
            .check(
                &self.client,
                &self.db,
                self.owner,
                &message,
                &name,
                module_name,
                permission,
            )
            .await?
        {
            return Ok(());
        }
        let limits: crate::api_protection::Limits =
            serde_json::from_value(self.db.get("RustApiProtection", "limits")).unwrap_or_default();
        let delay = self.api_limits.delay(&limits);
        if !delay.is_zero() {
            tokio::select! {_=self.stop.cancelled()=>return Ok(()),_=tokio::time::sleep(delay)=>{}}
        }
        if !crate::preferences::addressed(
            &self.db,
            user.unwrap_or(0),
            chat,
            &name,
            message.text(),
            self.username.as_deref(),
            message.outgoing(),
            security::targeted(&self.db, user.unwrap_or(0), chat, &name, module_name, false),
        ) {
            return Ok(());
        }
        if !self.seen.lock().unwrap().insert(chat, message.id()) {
            return Ok(());
        }
        if !security::owners(&self.db, self.owner).contains(&user.unwrap_or(0))
            && !self.rate.lock().unwrap().accept(user.unwrap_or(0), chat)
        {
            return Ok(());
        }
        let result =
            if let Some((native, _)) = native {
                match native
                    .command(&self.services(), &message, &name, &args)
                    .await
                {
                    Ok(crate::sdk::Response::Text(text)) => Ok(Some(text)),
                    Ok(crate::sdk::Response::None) => Ok(None),
                    Err(e) => Err(e),
                    Ok(crate::sdk::Response::Screen(screen)) => {
                        let screen = *screen;
                        let fallback = screen.text.clone();
                        if let Some(bot) = self.bot.read().await.clone() {
                            match bot.form(self, &message, screen).await {
                                Ok(()) => Ok(None),
                                Err(_) => Ok(Some(fallback)),
                            }
                        } else {
                            Ok(Some(fallback))
                        }
                    }
                }
            } else if let Some((package, _)) = module {
                let _serial = self.events.lock().await;
                let module_name = package.manifest.name.clone();
                ensure!(
                    crate::module_lifecycle::active(self, &module_name),
                    "module is suspended"
                );
                ensure!(
                    self.modules
                        .lock()
                        .unwrap()
                        .package(&module_name)
                        .is_some_and(|p| p.revision() == package.revision()),
                    "module revision changed"
                );
                match crate::module_host::run_command(
                    self,
                    package,
                    name,
                    args,
                    crate::module_host::Mode {
                        chat: Some(chat),
                        message_id: Some(message.id()),
                        preview: false,
                    },
                )
                .await
                {
                    Err(e) => Err(e),
                    Ok(effect) => {
                        let output = crate::module_effects::Plan::new(&module_name, effect)?
                            .commit(self, &module_name, false)?;
                        crate::module_effects::message(self, &module_name, output, &message).await
                    }
                }
            } else {
                self.execute(&message, &name, &args).await
            };
        match result {
            Ok(Some(text)) => {
                let text = if let Some(grep) = &grep {
                    grep.apply(&text)
                } else {
                    text
                };
                let text = self.logs.redact(crate::output::censor(text, &self.secrets));
                if grep.is_none()
                    && crate::output::pages(&text).len() > 1
                    && crate::output::pages(&text).len() <= 50
                    && let Some(bot) = self.bot.read().await.clone()
                {
                    let screen = crate::forms::Screen {
                        text: text.clone(),
                        ..Default::default()
                    };
                    if bot.form(self, &message, screen).await.is_ok() {
                        return Ok(());
                    }
                }
                if crate::output::pages(&text).len() > 6 {
                    telegram::send_file(
                        &self.client,
                        telegram::peer(&message).await?,
                        "command_result.txt",
                        text.into_bytes(),
                        "Command output",
                    )
                    .await?;
                } else {
                    telegram::answer(&message, &text).await?;
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(command=%message.id(),"command failed");
                let mut text = format!(
                    "<tg-emoji emoji-id=5877477244938489129>🚫</tg-emoji><b> Call</b> <code>{}</code><b> failed!</b>",
                    crate::output::escape(message.text())
                );
                if self
                    .db
                    .get("atarustia.main", "inlinelogs")
                    .as_bool()
                    .unwrap_or(true)
                {
                    text.push_str(&format!(
                        "\n\n<b>🧾 Logs:</b>\n<pre><code class=\"language-logs\">{}</code></pre>",
                        crate::output::escape(
                            &self.logs.redact(crate::output::error_message(
                                &e.to_string(),
                                &self.secrets
                            ))
                        )
                    ));
                }
                let input = grammers_client::message::InputMessage::new()
                    .html(text)
                    .link_preview(false);
                if message.outgoing() {
                    message.edit(input).await?;
                } else {
                    message.reply(input).await?;
                }
            }
        }
        Ok(())
    }
    async fn execute(&self, m: &Message, name: &str, args: &str) -> Result<Option<String>> {
        if name == "help" {
            crate::help::send(self, m, args).await?;
            return Ok(None);
        }
        if matches!(name, "update" | "restart") {
            crate::update_flow::command(self, m, name, args).await?;
            return Ok(None);
        }
        if name == "ataraxia" {
            return crate::original::about(self, m).await;
        }
        if name == "info" {
            crate::info::send(self, m).await?;
            return Ok(None);
        }
        if name == "ping" {
            crate::presentation::ping(self, m).await?;
            return Ok(None);
        }
        if matches!(name, "config" | "settings") {
            let screen = if name == "settings" {
                crate::config_ui::settings(self)
            } else {
                crate::screens::config(self, args)?
            };
            if let Some(bot) = self.bot.read().await.clone() {
                bot.form(self, m, screen).await?;
            } else {
                let input = grammers_client::message::InputMessage::new()
                    .html(screen.text)
                    .link_preview(false);
                if m.outgoing() {
                    m.edit(input).await?;
                } else {
                    m.reply(input).await?;
                }
            }
            return Ok(None);
        }
        if name == "tsec"
            || crate::security_commands::COMMANDS
                .iter()
                .any(|(n, _)| *n == name)
        {
            return crate::security_commands::execute(self, m, name, args).await;
        }
        if crate::preferences::COMMANDS.iter().any(|(n, _)| *n == name) {
            return crate::preferences::execute(self, m, name, args).await;
        }
        let text = match name {
            "importbackup" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "import backups only in Saved Messages"
                );
                let bytes = self.reply_bytes(m, 16 * 1024 * 1024).await?;
                let root = self.root.clone();
                let owner = self.owner;
                let staged = tokio::task::spawn_blocking(move || {
                    crate::legacy_backup::stage(&root, &bytes, owner)
                })
                .await??;
                if staged.report.has_database {
                    let converted = std::fs::read(staged.directory.join("converted.backup.json"))?;
                    telegram::send_file(&self.client,telegram::peer(m).await?,"converted.backup.json",converted,"Review these converted preferences; .restoredb applies them explicitly. Old owners/security and Python code are not activated.").await?;
                }
                format!(
                    "Legacy backup reviewed. Python sources preserved as .py.txt: {}\nRequirements are review-only: {}\nReport: {}\nNo settings or code were activated.",
                    staged.report.python_sources.len(),
                    staged.report.requirements.len(),
                    staged.directory.join("report.json").display()
                )
            }
            "review" => {
                let package = self.modules.lock().unwrap().pending(args)?;
                telegram::send_file(
                    &self.client,
                    telegram::peer(m).await?,
                    "pending.module.json",
                    serde_json::to_vec_pretty(&package)?,
                    "Review this exact source before .approve",
                )
                .await?;
                return Ok(None);
            }
            "suspend" => {
                let seconds: u64 = args.parse()?;
                ensure!(seconds <= 86400, "maximum pause 86400 seconds");
                self.db
                    .set("RustRuntime", "paused_until", security::now() + seconds)?;
                format!("Command handling paused for {seconds} seconds; .suspend 0 resumes")
            }
            "tglogs" => {
                ensure!(matches!(args, "on" | "off"), "usage: tglogs on|off");
                self.db.set("RustLogs", "telegram", args == "on")?;
                "Telegram logs updated".into()
            }
            "api_fw_protection" => {
                ensure!(
                    matches!(args, "on" | "off"),
                    "usage: api_fw_protection on|off"
                );
                let mut limits: crate::api_protection::Limits =
                    serde_json::from_value(self.db.get("RustApiProtection", "limits"))
                        .unwrap_or_default();
                limits.enabled = args == "on";
                self.db
                    .set("RustApiProtection", "limits", serde_json::to_value(limits)?)?;
                "API command limiter updated".into()
            }
            "suspend_api_protect" => {
                let seconds: u64 = args.parse()?;
                ensure!(seconds <= 86400, "maximum 86400 seconds");
                self.api_limits.suspend(seconds);
                "Command limiter temporarily suspended".into()
            }
            "terminate" => {
                let reply = m
                    .get_reply()
                    .await?
                    .context("reply to a running executor command")?;
                ensure!(
                    self.jobs
                        .cancel(m.peer_id().bot_api_dialog_id_unchecked(), reply.id()),
                    "no running command for this message"
                );
                "Cancellation requested".into()
            }
            "addacc" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "addacc only in Saved Messages"
                );
                ensure!(
                    self.web.is_some(),
                    "restart without --no-web to add accounts"
                );
                crate::config::validate_account(args)?;
                let _guard = self.maintenance.lock().await;
                let mut config = crate::config::Config::load(&self.root)?;
                ensure!(
                    !config.accounts.contains_key(args),
                    "account slot already exists"
                );
                config.accounts.insert(
                    args.into(),
                    crate::config::AccountConfig {
                        needs_login: true,
                        ..Default::default()
                    },
                );
                config.save(&self.root)?;
                telegram::answer(m,"Account slot created. Restarting; open a fresh one-time link from the terminal to authorize it.").await?;
                self.db.set("RustRuntime", "restart", true)?;
                self.stop.cancel();
                return Ok(None);
            }
            "ch_bot_token" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "set tokens only in Saved Messages"
                );
                crate::config::validate_bot_token(args)?;
                let _guard = self.maintenance.lock().await;
                let mut config = crate::config::Config::load(&self.root)?;
                config
                    .accounts
                    .get_mut(&self.account)
                    .context("account slot missing")?
                    .bot_token = Some(args.into());
                config.save(&self.root)?;
                self.answer_html(m, &self.original("inline_stuff", "bot_updated", &[]))
                    .await?;
                self.db.set("RustRuntime", "restart", true)?;
                self.stop.cancel();
                return Ok(None);
            }
            "ch_ataraxia_bot" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "bot setup only in Saved Messages"
                );
                let _guard = self.maintenance.lock().await;
                let username = if args.is_empty() {
                    crate::original::bot_username()
                } else {
                    args.trim_start_matches('@').into()
                };
                let existing = self.client.resolve_username(&username).await?;
                let token = if existing.is_some() {
                    crate::botfather::owned_token(&self.client, &username).await?
                } else {
                    crate::botfather::provision(&self.client, &username).await?
                };
                let mut config = crate::config::Config::load(&self.root)?;
                config
                    .accounts
                    .get_mut(&self.account)
                    .context("account missing")?
                    .bot_token = Some(token);
                config.save(&self.root)?;
                self.answer_html(m, &self.original("inline_stuff", "bot_updated", &[]))
                    .await?;
                self.db.set("RustRuntime", "restart", true)?;
                self.stop.cancel();
                return Ok(None);
            }
            "dllangpack" => {
                crate::i18n::install(&self.db, args).await?;
                self.answer_html(m, &self.original("translations", "pack_saved", &[]))
                    .await?;
                return Ok(None);
            }
            "loops" => {
                let p: Vec<_> = args.split_whitespace().collect();
                if p.is_empty() {
                    let list:Vec<_>=self.modules.lock().unwrap().list().into_iter().filter(|m|!m.loops.is_empty()).map(|m|json!({"module":m.name,"loops":m.loops,"overrides":self.db.get(&m.name,"loops")})).collect();
                    serde_json::to_string_pretty(&list)?
                } else {
                    ensure!(
                        p.len() == 3 && matches!(p[2], "on" | "off"),
                        "usage: loops <module> <function> on|off"
                    );
                    ensure!(
                        self.modules
                            .lock()
                            .unwrap()
                            .list()
                            .iter()
                            .any(|m| m.name == p[0] && m.loops.iter().any(|l| l.function == p[1])),
                        "unknown loop"
                    );
                    let mut overrides = self
                        .db
                        .get(p[0], "loops")
                        .as_object()
                        .cloned()
                        .unwrap_or_default();
                    overrides.insert(p[1].into(), json!(p[2] == "on"));
                    self.db.set(p[0], "loops", Value::Object(overrides))?;
                    "Loop state saved".into()
                }
            }
            "logs" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "logs only in Saved Messages"
                );
                let text = crate::output::censor(
                    String::from_utf8_lossy(&self.logs.snapshot()).to_string(),
                    &self.secrets,
                );
                telegram::send_file(
                    &self.client,
                    telegram::peer(m).await?,
                    "ataraxia-logs.txt",
                    self.logs.redact(text).into_bytes(),
                    "Confidential runtime logs",
                )
                .await?;
                return Ok(None);
            }
            "clearlogs" => {
                self.logs.clear();
                self.answer_html(m, &self.original("test", "logs_cleared", &[]))
                    .await?;
                return Ok(None);
            }
            "setup" => {
                let id = tokio::time::timeout(
                    Duration::from_secs(120),
                    crate::content::setup(&self.client, &self.db),
                )
                .await??;
                format!("Контент-форум готов: {id}")
            }
            "source" => {
                self.answer_html(
                    m,
                    &self.original(
                        "updater",
                        "source",
                        &["https://git.break.wtf/xenside/ataraxia".into()],
                    ),
                )
                .await?;
                return Ok(None);
            }
            "setprefix" => {
                let parts: Vec<_> = args.split_whitespace().collect();
                ensure!(
                    !parts.is_empty() && parts.len() <= 2,
                    "usage: setprefix <prefix> [user]"
                );
                let prefix = parts[0];
                let oldprefix = if let Some(target) = parts.get(1) {
                    let id = crate::security_commands::resolve_user(self, m, Some(target)).await?;
                    self.db.prefix_for(Some(id), self.owner)
                } else {
                    self.db.prefix()
                };
                ensure!(
                    prefix != "s"
                        && (prefix.chars().count() == 1
                            || self
                                .db
                                .get("Settings", "__config__")
                                .get("allow_nonstandart_prefixes")
                                .and_then(Value::as_bool)
                                == Some(true)),
                    "{}",
                    crate::original::text(
                        self.db.language().starts_with("en"),
                        "settings",
                        "prefix_incorrect"
                    )
                );
                if let Some(target) = parts.get(1) {
                    let user =
                        crate::security_commands::resolve_user(self, m, Some(target)).await?;
                    ensure!(
                        security::owners(&self.db, self.owner).contains(&user)
                            || security::groups(&self.db)
                                .values()
                                .any(|g| g.users.contains(&user)),
                        "personal prefix requires owner/group membership"
                    );
                    let mut prefixes = self
                        .db
                        .get("atarustia.main", "command_prefixes")
                        .as_object()
                        .cloned()
                        .unwrap_or_default();
                    prefixes.insert(user.to_string(), json!(prefix));
                    self.db.set(
                        "atarustia.main",
                        "command_prefixes",
                        Value::Object(prefixes),
                    )?;
                } else {
                    self.db.set("atarustia.main", "command_prefix", prefix)?;
                }
                let personal = parts.get(1);
                let text = crate::original::message(
                    self.db.language().starts_with("en"),
                    "settings",
                    if personal.is_some() {
                        "entity_prefix_set"
                    } else {
                        "prefix_set"
                    },
                    &["<tg-emoji emoji-id=5197474765387864959>👍</tg-emoji>".into()],
                    &[
                        ("newprefix", crate::output::escape(prefix)),
                        ("oldprefix", crate::output::escape(&oldprefix)),
                        (
                            "entity_name",
                            crate::output::escape(personal.copied().unwrap_or("")),
                        ),
                        (
                            "entity_id",
                            crate::output::escape(personal.copied().unwrap_or("")),
                        ),
                    ],
                );
                self.answer_html(m, &text).await?;
                return Ok(None);
            }
            "setlang" => {
                ensure!(matches!(args, "ru" | "en"), "use ru or en");
                self.db.set("atarustia.translations", "lang", args)?;
                self.answer_html(
                    m,
                    &self.original(
                        "translations",
                        "lang_saved",
                        &[if args == "en" { "🇬🇧" } else { "🇷🇺" }.into()],
                    ),
                )
                .await?;
                return Ok(None);
            }
            "aliases" => {
                let saved = self.db.get("Settings", "aliases");
                let emoji = self
                    .db
                    .get("Settings", "__config__")
                    .get("alias_emoji")
                    .and_then(Value::as_str)
                    .unwrap_or("<tg-emoji emoji-id=4974259868996207180>▪️</tg-emoji>")
                    .to_owned();
                let lines = saved
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.iter())
                    .map(|(alias, target)| {
                        format!(
                            "{emoji} <code>{}</code> &lt;- {}",
                            crate::output::escape(alias),
                            crate::output::escape(target.as_str().unwrap_or(""))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = format!(
                    "{}<blockquote expandable>{lines}</blockquote>",
                    crate::original::text(
                        self.db.language().starts_with("en"),
                        "settings",
                        "aliases"
                    )
                );
                let body = grammers_client::message::InputMessage::new()
                    .html(text)
                    .link_preview(false);
                if m.outgoing() {
                    m.edit(body).await?;
                } else {
                    m.reply(body).await?;
                }
                return Ok(None);
            }
            "addalias" => {
                let (alias, target) = args
                    .split_once(char::is_whitespace)
                    .context("usage: addalias <alias> <command>")?;
                ensure!(
                    crate::modules::valid_name(alias) && !is_builtin(alias),
                    "invalid alias or built-in collision"
                );
                let mut a: HashMap<String, String> =
                    serde_json::from_value(self.db.get("Settings", "aliases")).unwrap_or_default();
                a.insert(alias.into(), target.trim().into());
                let (resolved, _) = crate::dispatcher::resolve(alias, "", &a)?;
                ensure!(
                    is_builtin(&resolved)
                        || self.modules.lock().unwrap().command(&resolved).is_some(),
                    "unknown target command"
                );
                self.db
                    .set("Settings", "aliases", serde_json::to_value(a)?)?;
                self.answer_html(
                    m,
                    &self.original("settings", "alias_created", &[crate::output::escape(alias)]),
                )
                .await?;
                return Ok(None);
            }
            "delalias" => {
                let mut a: HashMap<String, String> =
                    serde_json::from_value(self.db.get("Settings", "aliases")).unwrap_or_default();
                ensure!(a.remove(args).is_some(), "alias not found");
                self.db
                    .set("Settings", "aliases", serde_json::to_value(a)?)?;
                self.answer_html(
                    m,
                    &self.original("settings", "alias_removed", &[crate::output::escape(args)]),
                )
                .await?;
                return Ok(None);
            }
            "config" | "settings" => {
                if args.is_empty() {
                    serde_json::to_string_pretty(
                        &json!({"prefix":self.db.prefix(),"language":self.db.language(),"backup_hours":self.db.get("RustBackup","hours"),"shell_enabled":self.allow_shell,"modules":self.modules.lock().unwrap().list()}),
                    )?
                } else {
                    serde_json::to_string_pretty(&crate::screens::config_data(self, args)?.0)?
                }
            }
            "fconfig" => {
                let reply = m.get_reply().await?;
                let text =
                    crate::config_ui::fast_config(self, args, reply.as_ref().map(|m| m.text()))?;
                crate::module_lifecycle::config_changes(self).await;
                let input = grammers_client::message::InputMessage::new()
                    .html(text)
                    .link_preview(false);
                if m.outgoing() {
                    m.edit(input).await?;
                } else {
                    m.reply(input).await?;
                }
                return Ok(None);
            }
            "modules" => {
                let list = self.modules.lock().unwrap().list();
                if list.is_empty() {
                    "Внешние модули не установлены".into()
                } else {
                    serde_json::to_string_pretty(&list)?
                }
            }
            "dlm" => {
                let mut p = crate::modules::download(args).await?;
                crate::modules::resolve_packurl(&mut p).await?;
                self.check_scope(&p.manifest.scope).await?;
                let hash = self.modules.lock().unwrap().stage(p)?;
                crate::module_lifecycle::approve(self, &hash).await?;
                self.tr("module_installed", &[])
            }
            "lm" => {
                let bytes = self.reply_bytes(m, 512 * 1024).await?;
                let mut p: crate::modules::Package = serde_json::from_slice(&bytes)
                    .context("expected a .module.json package, not Python")?;
                crate::modules::resolve_packurl(&mut p).await?;
                self.check_scope(&p.manifest.scope).await?;
                let hash = self.modules.lock().unwrap().stage(p)?;
                crate::module_lifecycle::approve(self, &hash).await?;
                self.tr("module_installed", &[])
            }
            "approve" => {
                let pending = self.modules.lock().unwrap().pending(args)?;
                ensure!(
                    pending
                        .manifest
                        .commands
                        .iter()
                        .all(|c| self.native.command(&c.name).is_none()),
                    "package command conflicts with a native Rust module"
                );
                self.check_scope(&pending.manifest.scope).await?;
                crate::module_lifecycle::approve(self, args).await?;
                self.tr("module_installed", &[])
            }
            "ulm" => {
                crate::module_lifecycle::remove(self, args).await?;
                self.tr("module_removed", &[])
            }
            "clearmodules" => {
                ensure!(args == "confirm", "usage: clearmodules confirm");
                crate::module_lifecycle::clear(self).await?;
                "Внешние модули удалены".into()
            }
            "ml" => {
                let p = self
                    .modules
                    .lock()
                    .unwrap()
                    .export()
                    .into_iter()
                    .find(|p| p.manifest.name == args)
                    .context("unknown module")?;
                telegram::send_file(
                    &self.client,
                    telegram::peer(m).await?,
                    &format!("{args}.module.json"),
                    serde_json::to_vec_pretty(&p)?,
                    "Pinned module source",
                )
                .await?;
                return Ok(None);
            }
            "e" => {
                let source = args.to_owned();
                tokio::task::spawn_blocking(move || crate::modules::evaluate(&source)).await??
            }
            "terminal" | "ec" | "ecpp" | "enode" => {
                ensure!(
                    self.allow_shell,
                    "native execution disabled; start with --allow-shell"
                );
                let token = self.stop.child_token();
                let chat = m.peer_id().bot_api_dialog_id_unchecked();
                ensure!(
                    self.jobs.insert(chat, m.id(), token.clone()),
                    "command is already running"
                );
                let result = tokio::select! {_=token.cancelled()=>Err(anyhow::anyhow!("execution cancelled")),result=self.run_native(name,args)=>result};
                self.jobs.remove(chat, m.id());
                result?
            }
            "ownerlist" => format!("Owners: {:?}", security::owners(&self.db, self.owner)),
            "owneradd" | "ownerrm" => {
                let id: i64 = args.parse().context("numeric user ID required")?;
                ensure!(id > 0, "invalid user ID");
                let mut owners = security::owners(&self.db, self.owner);
                if name == "owneradd" {
                    if !owners.contains(&id) {
                        owners.push(id);
                    }
                } else {
                    ensure!(id != self.owner, "primary owner cannot be removed");
                    owners.retain(|x| *x != id);
                }
                self.db.set(
                    "atarustia.security",
                    "owner",
                    serde_json::to_value(&owners)?,
                )?;
                format!("Owners: {owners:?}")
            }
            "security" => {
                if let Some(spec) = args.strip_prefix("bounding ") {
                    let permission = Permission::parse(spec.trim())?;
                    self.db.set(
                        "atarustia.security",
                        "bounding_mask",
                        json!(permission.bits()),
                    )?;
                    return Ok(Some(format!("Bounding mask: {}", permission.bits())));
                }
                let (cmd, mask) = args
                    .split_once(char::is_whitespace)
                    .context("usage: security <command> owner|private|group|everyone")?;
                ensure!(
                    is_builtin(cmd) || self.modules.lock().unwrap().command(cmd).is_some(),
                    "unknown command"
                );
                ensure!(
                    !security::owner_only(cmd),
                    "this command is always owner-only"
                );
                let permission = Permission::parse(mask.trim())?;
                let mut masks = self
                    .db
                    .get("atarustia.security", "masks")
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                masks.insert(cmd.into(), serde_json::to_value(permission)?);
                self.db
                    .set("atarustia.security", "masks", Value::Object(masks))?;
                "Права сохранены".into()
            }
            "tsec" => {
                let p: Vec<_> = args.split_whitespace().collect();
                ensure!(p.len() == 3, "usage: tsec <id> <command> <seconds>");
                let user: i64 = p[0].parse()?;
                let seconds: u64 = p[2].parse()?;
                ensure!(
                    user > 0 && seconds > 0 && seconds <= 365 * 86400,
                    "invalid user or duration"
                );
                ensure!(!security::owner_only(p[1]), "cannot delegate this command");
                ensure!(
                    is_builtin(p[1]) || self.modules.lock().unwrap().command(p[1]).is_some(),
                    "unknown command"
                );
                let mut grants: Vec<Grant> =
                    serde_json::from_value(self.db.get("atarustia.security", "grants"))
                        .unwrap_or_default();
                grants.retain(|g| g.expires == 0 || g.expires > security::now());
                grants.push(Grant {
                    user: Some(user),
                    chat: None,
                    command: p[1].into(),
                    expires: security::now() + seconds,
                });
                self.db.set(
                    "atarustia.security",
                    "grants",
                    serde_json::to_value(grants)?,
                )?;
                "Временное разрешение сохранено".into()
            }
            "backupall" | "backupdb" | "backupmods" => {
                let kind = match name {
                    "backupdb" => backup::Kind::Database,
                    "backupmods" => backup::Kind::Modules,
                    _ => backup::Kind::All,
                };
                let bytes = {
                    let r = self.modules.lock().unwrap();
                    backup::create_kind(&self.db, &r, self.owner, kind)?
                };
                let path = backup::local(&self.root, &bytes, self.owner)?;
                telegram::send_backup(
                    &self.client,
                    &self.db,
                    bytes,
                    "Confidential backup: database and module sources, no sessions or master key.",
                )
                .await?;
                format!(
                    "Резервная копия отправлена в контент-форум или Избранное. Локально: {}",
                    path.display()
                )
            }
            "restoredb" | "restoreall" | "restoremods" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "restore only in Saved Messages"
                );
                let bytes = self.reply_bytes(m, 16 * 1024 * 1024).await?;
                if bytes.starts_with(b"PK") {
                    return Ok(Some(format!(
                        "This is an original Python ZIP backup. Reply with {}importbackup to review/convert it; it will not execute Python or install requirements.",
                        self.db.prefix()
                    )));
                }
                let b = backup::parse(&bytes, self.owner)?;
                let before = {
                    let r = self.modules.lock().unwrap();
                    backup::create(&self.db, &r, self.owner)?
                };
                backup::local(&self.root, &before, self.owner)?;
                if name != "restoremods" {
                    ensure!(
                        b.kind != backup::Kind::Modules,
                        "module-only backup does not contain a database"
                    );
                    self.db.restore(b.database)?;
                }
                if name != "restoredb" {
                    let mut staged = Vec::new();
                    let registry = self.modules.lock().unwrap();
                    for p in b.modules {
                        let name = p.manifest.name.clone();
                        staged.push(format!(
                            "{name}: {}approve {}",
                            self.db.prefix(),
                            registry.stage(p)?
                        ));
                    }
                    format!(
                        "Резервная копия обработана. Модули не исполнялись. Одобри каждый пакет отдельно:\n{}",
                        staged.join("\n")
                    )
                } else {
                    "БД восстановлена".into()
                }
            }
            "set_backup_period" => {
                let hours: u64 = args.parse().context("hours must be an integer")?;
                ensure!(hours <= 8760, "maximum 8760 hours");
                self.db.set("RustBackup", "hours", hours)?;
                self.db.set("RustBackup", "last", security::now())?;
                format!("Период резервного копирования: {hours} ч (0 = выключено)")
            }
            "weburl" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "weburl is only allowed in Saved Messages"
                );
                let web = self
                    .web
                    .as_ref()
                    .context("web disabled; restart without --no-web")?;
                let link = web.link();
                let url = link.split("/#").next().unwrap_or(&link);
                let text = crate::original::text(
                    self.db.language().starts_with("en"),
                    "ataraxia_web",
                    "web_link",
                )
                .replace("{link}", &crate::output::escape(&link))
                .replace("{url}", &crate::output::escape(url));
                let input = grammers_client::message::InputMessage::new().html(text);
                if m.outgoing() {
                    m.edit(input).await?;
                } else {
                    m.reply(input).await?;
                }
                return Ok(None);
            }
            _ => bail!("unknown command"),
        };
        Ok(Some(text))
    }
    /// Verifies a package's `scope` requirements before it is staged or installed.
    async fn check_scope(&self, scope: &[String]) -> Result<()> {
        for need in scope {
            match need.as_str() {
                "inline" => ensure!(
                    self.bot.read().await.is_some(),
                    "module requires the inline bot, which is not running"
                ),
                "shell" => ensure!(self.allow_shell, "module requires --allow-shell"),
                "ffmpeg" => {
                    let present = std::process::Command::new("ffmpeg")
                        .arg("-version")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success());
                    ensure!(present, "module requires ffmpeg, which is not installed");
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub async fn handle_event(&self, event: crate::modules::Event) -> Result<()> {
        if self
            .db
            .get("RustRuntime", "paused_until")
            .as_u64()
            .unwrap_or(0)
            > security::now()
        {
            return Ok(());
        }
        // Watchers only reply to the triggering message's own chat. No arbitrary remote targets.
        let _serial = self.events.lock().await;
        let packages = self.modules.lock().unwrap().export();
        for package in packages {
            if !crate::module_lifecycle::active(self, &package.manifest.name) {
                continue;
            }
            let policy = crate::preferences::watcher_policy(&self.db, &package.manifest.name);
            if !policy.accepts(event.chat, event.outgoing) {
                continue;
            }
            for watcher in package
                .manifest
                .watchers
                .clone()
                .into_iter()
                .filter(|w| w.accepts(&event))
            {
                if !crate::module_lifecycle::active(self, &package.manifest.name) {
                    break;
                }
                let result = async {
                    let p = package.clone();
                    let data = serde_json::to_value(&event)?;
                    let effect = crate::module_host::run_event(
                        self,
                        p,
                        watcher.function,
                        data,
                        crate::module_host::Mode {
                            chat: Some(event.chat),
                            message_id: (event.id > 0).then_some(event.id),
                            preview: false,
                        },
                    )
                    .await?;
                    let output = crate::module_effects::Plan::new(&package.manifest.name, effect)?
                        .commit(self, &package.manifest.name, false)?;
                    if !matches!(output, crate::module_effects::Output::None) {
                        let user = event.sender.unwrap_or(0);
                        if !self.rate.lock().unwrap().accept(user, event.chat) {
                            return Ok::<_, anyhow::Error>(());
                        }
                        crate::module_effects::background(
                            self,
                            &package.manifest.name,
                            output,
                            event.kind != "message" && event.kind != "raw",
                            event.chat,
                            (event.id > 0).then_some(event.id),
                        )
                        .await?;
                    }
                    Ok(())
                }
                .await;
                if let Err(error) = result {
                    crate::module_lifecycle::report(
                        self,
                        &package.manifest.name,
                        "watcher",
                        &error,
                    );
                }
            }
        }
        Ok(())
    }
    pub async fn run_loop(
        &self,
        package: crate::modules::Package,
        task: crate::modules::Loop,
    ) -> Result<()> {
        if self
            .db
            .get("RustRuntime", "paused_until")
            .as_u64()
            .unwrap_or(0)
            > security::now()
        {
            return Ok(());
        }
        let _serial = self.events.lock().await;
        if !crate::module_lifecycle::active(self, &package.manifest.name) {
            return Ok(());
        }
        // A module removed while the timer was waiting must not run one final time.
        if !self
            .modules
            .lock()
            .unwrap()
            .export()
            .iter()
            .any(|p| p.manifest.name == package.manifest.name && p.revision() == package.revision())
        {
            return Ok(());
        }
        let override_value = self.db.get(&package.manifest.name, "loops");
        if !override_value
            .get(&task.function)
            .and_then(Value::as_bool)
            .unwrap_or(task.autostart)
        {
            return Ok(());
        }
        if let Some(clause) = &task.stop_clause
            && self
                .db
                .get(&package.manifest.name, "state")
                .get(clause)
                .and_then(Value::as_bool)
                != Some(true)
        {
            return Ok(());
        }
        let name = package.manifest.name.clone();
        let effect = crate::module_host::run_event(
            self,
            package,
            task.function,
            json!({"kind":"loop","time":security::now()}),
            crate::module_host::Mode::default(),
        )
        .await?;
        let output = crate::module_effects::Plan::new(&name, effect)?.commit(self, &name, false)?;
        crate::module_effects::background(self, &name, output, false, self.owner, None).await?;
        Ok(())
    }
    async fn reply_bytes(&self, m: &Message, limit: usize) -> Result<Vec<u8>> {
        let reply = m.get_reply().await?.context("reply to a file")?;
        let media = reply.media().context("reply has no file")?;
        let mut download = self.client.iter_download(&media);
        let mut bytes = Vec::new();
        while let Some(chunk) =
            tokio::time::timeout(Duration::from_secs(30), download.next()).await??
        {
            ensure!(bytes.len() + chunk.len() <= limit, "file too large");
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn run_native(&self, name: &str, source: &str) -> Result<String> {
        ensure!(!source.trim().is_empty(), "source missing");
        let dir = tempfile::tempdir()?;
        match name {
            "terminal" => {
                #[cfg(windows)]
                let mut cmd = {
                    let mut c = tokio::process::Command::new("cmd");
                    c.args(["/C", source]);
                    c
                };
                #[cfg(not(windows))]
                let mut cmd = {
                    let mut c = tokio::process::Command::new("/bin/sh");
                    c.args(["-c", source]);
                    c
                };
                capture(&mut cmd, &self.root, Duration::from_secs(60)).await
            }
            "enode" => {
                tokio::fs::write(dir.path().join("code.js"), source).await?;
                capture(
                    tokio::process::Command::new("node").arg("code.js"),
                    dir.path(),
                    Duration::from_secs(10),
                )
                .await
            }
            _ => {
                let (compiler, file) = if name == "ec" {
                    ("gcc", "code.c")
                } else {
                    ("g++", "code.cpp")
                };
                tokio::fs::write(dir.path().join(file), source).await?;
                let binary = if cfg!(windows) { "code.exe" } else { "code" };
                let report = capture(
                    tokio::process::Command::new(compiler).args([file, "-o", binary]),
                    dir.path(),
                    Duration::from_secs(30),
                )
                .await?;
                if !dir.path().join(binary).exists() {
                    return Ok(report);
                }
                capture(
                    &mut tokio::process::Command::new(dir.path().join(binary)),
                    dir.path(),
                    Duration::from_secs(10),
                )
                .await
            }
        }
    }
}
/// Bounded streams and a timeout; child is killed on drop. Never inherits stdin.
pub async fn capture(
    command: &mut tokio::process::Command,
    cwd: &std::path::Path,
    duration: Duration,
) -> Result<String> {
    use tokio::io::AsyncReadExt;
    command
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let _windows_job = WindowsJob::attach(&child)?;
    let _group = ProcessGroup(child.id());
    let stdout = child.stdout.take().context("stdout missing")?;
    let stderr = child.stderr.take().context("stderr missing")?;
    let read = |stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>| async move {
        let mut bytes = Vec::new();
        stream.take(128 * 1024 + 1).read_to_end(&mut bytes).await?;
        anyhow::ensure!(bytes.len() <= 128 * 1024, "process output exceeds 128 KiB");
        Ok::<_, anyhow::Error>(bytes)
    };
    let task = async {
        let (out, err, status) =
            tokio::try_join!(read(Box::new(stdout)), read(Box::new(stderr)), async {
                Ok::<_, anyhow::Error>(child.wait().await?)
            })?;
        Ok::<_, anyhow::Error>(format!(
            "exit: {}\n{}{}",
            status,
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        ))
    };
    match tokio::time::timeout(duration, task).await {
        Ok(v) => v,
        Err(_) => {
            let _ = child.kill().await;
            bail!("execution timed out");
        }
    }
}
/// Drop also runs if the command handler is cancelled during shutdown.
pub(crate) struct ProcessGroup(pub(crate) Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0 {
            // The PID is the leader of our freshly-created private process group.
            unsafe {
                libc::kill(-(id as i32), libc::SIGKILL);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.0;
        }
    }
}

#[cfg(windows)]
pub(crate) struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl WindowsJob {
    pub(crate) fn attach(child: &tokio::process::Child) -> Result<Self> {
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        // The job owns no Rust references; the process handle remains owned by tokio::Child.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            ensure!(
                !job.is_null(),
                "cannot create process job: {}",
                std::io::Error::last_os_error()
            );
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            );
            let Some(process) = child.raw_handle() else {
                CloseHandle(job);
                bail!("child process handle missing");
            };
            if configured == 0 || AssignProcessToJobObject(job, process) == 0 {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(error.into());
            }
            Ok(Self(job))
        }
    }
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
// A Windows HANDLE is an opaque kernel reference; this guard only closes it once on drop.
#[cfg(windows)]
unsafe impl Send for WindowsJob {}

#[cfg(test)]
mod tests {
    use super::*;
    fn shell(script: &str) -> tokio::process::Command {
        #[cfg(windows)]
        {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", script]);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = tokio::process::Command::new("/bin/sh");
            c.args(["-c", script]);
            c
        }
    }
    #[tokio::test]
    async fn native_output_is_captured() {
        let dir = tempfile::tempdir().unwrap();
        let text = capture(&mut shell("echo hello"), dir.path(), Duration::from_secs(5))
            .await
            .unwrap();
        assert!(text.contains("hello"));
        assert!(text.contains("exit:"));
    }
    #[test]
    fn native_and_confidential_commands_are_locked() {
        for name in [
            "terminal",
            "e",
            "ec",
            "enode",
            "backupdb",
            "config",
            "weburl",
            "restoreall",
        ] {
            assert!(security::owner_only(name));
        }
        let mut names = std::collections::HashSet::new();
        for (name, _) in BUILTINS {
            assert!(names.insert(name));
            assert!(crate::modules::valid_name(name));
        }
    }
    #[tokio::test]
    async fn execution_timeout() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let mut c = shell("ping -n 6 127.0.0.1 > nul");
        #[cfg(not(windows))]
        let mut c = shell("sleep 5");
        assert!(
            capture(&mut c, dir.path(), Duration::from_millis(50))
                .await
                .is_err()
        );
    }
}
