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
    ("tglogs", "on|off — отправка WARN/ERROR в тему Logs"),
    ("loops", "[module function on|off] — фоновые задачи"),
    ("dllangpack", "<https-url> — JSON/YAML языковой пакет"),
    (
        "ch_atarustia_bot",
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
        "setup",
        "создать/восстановить контент-форум Assets / Backups / Logs",
    ),
    (
        "restoremods",
        "в ответ на backup JSON — подготовить модули к одобрению",
    ),
    ("clearmodules", "confirm — удалить все внешние модули"),
    ("settings", "настройки аккаунта"),
    (
        "help",
        "[module|command] [-f|-c|-l] — каталог и карточки модулей",
    ),
    ("info", "информация о сборке"),
    ("atarustia", "информация о платформе"),
    ("ping", "проверить отклик"),
    ("setprefix", "<prefix> — сменить префикс"),
    ("setlang", "ru|en — язык"),
    ("aliases", "список алиасов"),
    ("addalias", "<alias> <command> — добавить алиас"),
    ("delalias", "<alias> — удалить алиас"),
    ("config", "[module] — конфигурация"),
    ("fconfig", "<module> <key> <json> — изменить параметр"),
    ("modules", "список модулей"),
    ("dlm", "<https-url> — скачать пакет для проверки"),
    ("lm", "в ответ на JSON-пакет — подготовить модуль"),
    ("approve", "<sha256> — одобрить подготовленный модуль"),
    ("review", "<sha256> — исходники подготовленной ревизии"),
    (
        "importbackup",
        "в ответ на Python backup — проверить и сохранить для ручного переноса",
    ),
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
        let prefix = self.db.prefix();
        let mut text = self.tr(
            "help_title",
            &[("version", env!("CARGO_PKG_VERSION").into())],
        ) + "\n\n";
        for (name, doc) in BUILTINS
            .iter()
            .chain(crate::security_commands::COMMANDS)
            .chain(crate::preferences::COMMANDS)
        {
            text.push_str(&format!("{prefix}{name} — {doc}\n"));
        }
        for m in self.modules.lock().expect("modules lock").list() {
            if crate::preferences::hidden(&self.db, &m.name) {
                continue;
            }
            text.push_str(&format!("\n[{}] {}\n", m.name, m.description));
            for c in m.commands {
                text.push_str(&format!("{prefix}{} — {}\n", c.name, c.description));
            }
        }
        for module in self.native.list() {
            text.push_str(&format!("\n[{}] {}\n", module.name(), module.description()));
            for command in module.commands() {
                text.push_str(&format!(
                    "{prefix}{} — {}\n",
                    command.name, command.description
                ));
            }
        }
        text
    }
    pub async fn handle(self: &Arc<Self>, message: Message, edited: bool) -> Result<()> {
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
        let (args, grep) = crate::dispatcher::grep(&args);
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
        let result = if let Some((native, _)) = native {
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
            let config = self.db.get(&package.manifest.name, "__config__");
            let config = if config.is_null() {
                package.manifest.config.clone()
            } else {
                config
            };
            let state = self.db.get(&module_name, "state");
            match tokio::task::spawn_blocking(move || {
                package.run_command(&name, &args, config, state)
            })
            .await?
            {
                Err(e) => Err(e),
                Ok(effect) => {
                    if let Some(state) = effect.state {
                        self.db.set(&module_name, "state", state)?;
                    }
                    if let Some(media) = &effect.media {
                        let caption = self.logs.redact(effect.text.clone().unwrap_or_default());
                        let peer = telegram::peer(&message).await?;
                        let input = grammers_client::message::InputMessage::new()
                            .text(caption)
                            .reply_to(Some(message.id()))
                            .media(media.prepare(&self.client).await?);
                        self.client.send_message(peer, input).await?;
                        return Ok(());
                    }
                    if !effect.album.is_empty() {
                        let peer = telegram::peer(&message).await?;
                        let mut items = vec![];
                        for (i, media) in effect.album.iter().enumerate() {
                            items.push(
                                grammers_client::media::InputMedia::new()
                                    .caption(if i == 0 {
                                        self.logs.redact(effect.text.clone().unwrap_or_default())
                                    } else {
                                        String::new()
                                    })
                                    .reply_to(Some(message.id()))
                                    .media(media.prepare(&self.client).await?),
                            );
                        }
                        self.client.send_album(peer, items).await?;
                        return Ok(());
                    }
                    if let Some(screen) = effect.screen {
                        screen.restrict_module(&module_name)?;
                        let fallback = screen.text.clone();
                        if let Some(bot) = self.bot.read().await.clone() {
                            match bot.form(self, &message, screen).await {
                                Ok(()) => Ok(None),
                                Err(_) => Ok(Some(fallback)),
                            }
                        } else {
                            Ok(Some(fallback))
                        }
                    } else {
                        Ok(effect.text)
                    }
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
                telegram::answer(
                    &message,
                    &self.tr(
                        "error",
                        &[(
                            "error",
                            crate::output::error_message(&e.to_string(), &self.secrets),
                        )],
                    ),
                )
                .await?;
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
        if matches!(name, "info" | "atarustia") {
            crate::info::send(self, m).await?;
            return Ok(None);
        }
        if matches!(name, "config" | "settings") {
            let screen = crate::screens::config(self, args);
            if let (Ok(screen), Some(bot)) = (screen, self.bot.read().await.clone())
                && bot.form(self, m, screen).await.is_ok()
            {
                return Ok(None);
            }
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
                telegram::answer(m, &self.tr("restarting", &[])).await?;
                self.db.set("RustRuntime", "restart", true)?;
                self.stop.cancel();
                return Ok(None);
            }
            "ch_atarustia_bot" => {
                ensure!(
                    m.peer_id().bot_api_dialog_id_unchecked() == self.owner,
                    "bot setup only in Saved Messages"
                );
                let _guard = self.maintenance.lock().await;
                let username = if args.is_empty() {
                    format!("atarustia_{}_{}_bot", self.owner, rand::random::<u16>())
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
                telegram::answer(m, &self.tr("restarting", &[])).await?;
                self.db.set("RustRuntime", "restart", true)?;
                self.stop.cancel();
                return Ok(None);
            }
            "dllangpack" => {
                crate::i18n::install(&self.db, args).await?;
                self.tr("saved", &[])
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
                    "atarustia.log",
                    self.logs.redact(text).into_bytes(),
                    "Confidential runtime logs",
                )
                .await?;
                return Ok(None);
            }
            "clearlogs" => {
                self.logs.clear();
                self.tr("logs_cleared", &[])
            }
            "setup" => {
                let id = tokio::time::timeout(
                    Duration::from_secs(120),
                    crate::content::setup(&self.client, &self.db),
                )
                .await??;
                format!("Контент-форум готов: {id}")
            }
            "ping" => format!("Pong 🦀 · uptime {}s", self.start.elapsed().as_secs()),
            "source" => self.tr("source", &[]),
            "setprefix" => {
                let parts: Vec<_> = args.split_whitespace().collect();
                ensure!(
                    !parts.is_empty() && parts.len() <= 2,
                    "usage: setprefix <prefix> [user]"
                );
                let prefix = parts[0];
                ensure!(
                    !prefix.is_empty() && prefix.chars().count() <= 8,
                    "prefix must be 1–8 non-whitespace characters"
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
                self.tr("prefix_saved", &[("prefix", prefix.into())])
            }
            "setlang" => {
                ensure!(matches!(args, "ru" | "en"), "use ru or en");
                self.db.set("atarustia.translations", "lang", args)?;
                self.tr("language_saved", &[("language", args.into())])
            }
            "aliases" => serde_json::to_string_pretty(&self.db.get("Settings", "aliases"))?,
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
                self.tr("alias_saved", &[])
            }
            "delalias" => {
                let mut a: HashMap<String, String> =
                    serde_json::from_value(self.db.get("Settings", "aliases")).unwrap_or_default();
                ensure!(a.remove(args).is_some(), "alias not found");
                self.db
                    .set("Settings", "aliases", serde_json::to_value(a)?)?;
                self.tr("alias_deleted", &[])
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
                let mut parts = args.splitn(3, char::is_whitespace);
                let module = parts.next().context("module missing")?;
                let key = parts.next().context("key missing")?;
                let value: Value =
                    serde_json::from_str(parts.next().context("JSON value missing")?)?;
                crate::screens::set_config(self, module, key, value)?;
                self.tr("config_saved", &[])
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
                let p = crate::modules::download(args).await?;
                let manifest = serde_json::to_string_pretty(&p.manifest)?;
                let hash = self.modules.lock().unwrap().stage(p)?;
                format!(
                    "Пакет подготовлен, но НЕ установлен. Проверь исходники и права.\n{manifest}\n\nОдобрить точный пакет:\n{}approve {hash}",
                    self.db.prefix()
                )
            }
            "lm" => {
                let bytes = self.reply_bytes(m, 512 * 1024).await?;
                let p: crate::modules::Package = serde_json::from_slice(&bytes)
                    .context("expected a .module.json package, not Python")?;
                let hash = self.modules.lock().unwrap().stage(p)?;
                format!(
                    "Проверь пакет. Для установки: {}approve {hash}",
                    self.db.prefix()
                )
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
                self.modules.lock().unwrap().approve(args)?;
                // Installed revisions may have changed the implementation behind existing buttons.
                self.forms.lock().unwrap().clear();
                self.tr("module_installed", &[])
            }
            "ulm" => {
                self.modules.lock().unwrap().remove(args)?;
                self.forms.lock().unwrap().invalidate_module(args);
                self.tr("module_removed", &[])
            }
            "clearmodules" => {
                ensure!(args == "confirm", "usage: clearmodules confirm");
                let mut r = self.modules.lock().unwrap();
                let names = r.list();
                for m in names {
                    self.forms.lock().unwrap().invalidate_module(&m.name);
                }
                r.clear()?;
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
                self.web
                    .as_ref()
                    .context("web disabled; restart without --no-web")?
                    .link()
            }
            _ => bail!("unknown command"),
        };
        Ok(Some(text))
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
                let config = self.db.get(&package.manifest.name, "__config__");
                let config = if config.is_null() {
                    package.manifest.config.clone()
                } else {
                    config
                };
                let state = self.db.get(&package.manifest.name, "state");
                let p = package.clone();
                let data = serde_json::to_value(&event)?;
                let effect = tokio::task::spawn_blocking(move || {
                    p.run_event(&watcher.function, data, config, state)
                })
                .await??;
                if let Some(state) = effect.state {
                    self.db.set(&package.manifest.name, "state", state)?;
                }
                if let Some(text) = effect.text.filter(|t| !t.is_empty()) {
                    let user = event.sender.unwrap_or(0);
                    if !self.rate.lock().unwrap().accept(user, event.chat) {
                        continue;
                    }
                    let id = grammers_session::types::PeerId::from_bot_api_dialog_id(event.chat)
                        .context("invalid watcher chat")?;
                    let sender = if event.kind == "bot_message" {
                        self.bot
                            .read()
                            .await
                            .as_ref()
                            .map(|b| b.bot.client.clone())
                            .unwrap_or_else(|| self.client.clone())
                    } else {
                        self.client.clone()
                    };
                    let peer = sender
                        .resolve_peer(id.to_ambient_ref())
                        .await?
                        .to_ref()
                        .await
                        .map_err(anyhow::Error::from_boxed)?
                        .context("watcher peer missing")?;
                    for page in crate::output::pages(&crate::output::censor(text, &self.secrets))
                        .into_iter()
                        .take(3)
                    {
                        sender
                            .send_message(
                                peer,
                                grammers_client::message::InputMessage::new()
                                    .text(page)
                                    .reply_to((event.id > 0).then_some(event.id)),
                            )
                            .await?;
                    }
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
        // A module removed while the timer was waiting must not run one final time.
        if !self
            .modules
            .lock()
            .unwrap()
            .export()
            .iter()
            .any(|p| p.manifest.name == package.manifest.name && p.source == package.source)
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
        let config = self.db.get(&package.manifest.name, "__config__");
        let config = if config.is_null() {
            package.manifest.config.clone()
        } else {
            config
        };
        let state = self.db.get(&package.manifest.name, "state");
        let name = package.manifest.name.clone();
        let effect = tokio::task::spawn_blocking(move || {
            package.run_event(
                &task.function,
                json!({"kind":"loop","time":security::now()}),
                config,
                state,
            )
        })
        .await??;
        if let Some(state) = effect.state {
            self.db.set(&name, "state", state)?;
        }
        if let Some(text) = effect.text.filter(|t| !t.is_empty()) {
            for page in crate::output::pages(&crate::output::censor(text, &self.secrets))
                .into_iter()
                .take(3)
            {
                self.client
                    .send_message(
                        grammers_session::types::PeerId::self_user().to_ambient_ref(),
                        page,
                    )
                    .await?;
            }
        }
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
