use crate::{
    auth, backup,
    commands::Context,
    config::{Config, Credentials, Options},
    database::Database,
    modules::Registry,
    session::MasterKey,
    telegram::Connection,
    web,
};
use anyhow::{Context as _, Result, ensure};
use grammers_client::update::Update;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

pub async fn run(options: Options, logs: crate::logs::LogBuffer) -> Result<bool> {
    run_with_modules(options, logs, Arc::new(crate::sdk::Registry::default())).await
}
pub async fn run_with_modules(
    options: Options,
    logs: crate::logs::LogBuffer,
    native: Arc<crate::sdk::Registry>,
) -> Result<bool> {
    crate::config::validate_account(&options.account)?;
    if let Some(proxy) = &options.proxy {
        crate::telegram::validate_proxy(proxy)?;
        logs.add_secret(proxy.clone());
        if let Ok(url) = reqwest::Url::parse(proxy) {
            for (name, value) in url.query_pairs() {
                if name == "secret" {
                    logs.add_secret(value.into_owned());
                }
            }
        }
    }
    if let Some(source) = options.import_python {
        // The importer checks separation before acquiring its destination lock.
        // In particular --data-root == --import-python must not touch the source.
        let count = crate::migration::import_directory(&source, &options.data_root)?;
        println!(
            "Imported {count} encrypted account sessions. Source files were not modified. Start again without --import-python."
        );
        return Ok(false);
    }
    let _lock = crate::storage::DataLock::acquire(&options.data_root)?;
    let root = options.data_root.canonicalize()?;
    if let Some(source) = options.import_backup {
        let owner = options.import_owner.context("--import-owner is required")?;
        ensure!(
            std::fs::metadata(&source)?.len() <= 16 * 1024 * 1024,
            "legacy backup exceeds 16 MiB"
        );
        let bytes = std::fs::read(source)?;
        let staged = crate::legacy_backup::stage(&root, &bytes, owner)?;
        println!(
            "Legacy backup staged for review at {}. No account settings were applied and no Python code or requirements were executed.",
            staged.directory.display()
        );
        return Ok(false);
    }
    let mut config = Config::load(&root)?;
    let mut web_bootstrap = None;
    if !options.no_web
        && !options.no_auth
        && options.api_preset == "ask"
        && options.api_id.is_none()
        && options.api_hash.is_none()
        && config.credentials.is_none()
    {
        let mut address = options.listen;
        if let Some(port) = options.port {
            address.set_port(port);
        }
        ensure!(
            address.ip().is_loopback(),
            "web setup must bind to loopback; use SSH forwarding"
        );
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .context("cannot bind setup listener")?;
        let web = web::WebState::new(listener.local_addr()?);
        web.begin_setup();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(web::serve_listener(web.clone(), listener, cancel.clone()));
        println!("Open the setup link: {}", web.link());
        let value =
            tokio::select! {value=web.take_setup()=>Some(value),_=tokio::signal::ctrl_c()=>None};
        let Some(value) = value else {
            cancel.cancel();
            task.await??;
            return Ok(false);
        };
        config.credentials = Some(value);
        web_bootstrap = Some((web, cancel, task));
    }
    let credentials = match (options.api_id, options.api_hash) {
        (Some(api_id), Some(api_hash)) => Credentials { api_id, api_hash },
        (None, None) => {
            if let Some(c) = config.credentials.clone() {
                c
            } else {
                ensure!(!options.no_auth, "API credentials missing (--no-auth)");
                crate::setup::terminal_app(&options.api_preset).await?
            }
        }
        _ => anyhow::bail!("provide both --api-id and --api-hash"),
    };
    credentials.validate()?;
    logs.add_secret(credentials.api_hash.clone());
    config.credentials = Some(credentials.clone());
    // Starting the imported installation without --account must not create a new
    // empty default slot and force an unrelated login before imported accounts.
    let selected_account = if options.account == "default"
        && !config.accounts.is_empty()
        && !config.accounts.contains_key("default")
    {
        config.accounts.keys().next().cloned().unwrap()
    } else {
        options.account.clone()
    };
    let account = config.accounts.entry(selected_account.clone()).or_default();
    if let Some(token) = options.bot_token {
        crate::config::validate_bot_token(&token)?;
        account.bot_token = Some(token);
    }
    config.save(&root)?;
    let key = Arc::new(MasterKey::load(&root)?);
    let stop = CancellationToken::new();
    let _cancel_on_error = stop.clone().drop_guard();
    let web = if let Some((web, _, _)) = &web_bootstrap {
        Some(web.clone())
    } else if options.no_web {
        None
    } else {
        // No plaintext login over an exposed listener. Use SSH forwarding.
        ensure!(
            options.listen.ip().is_loopback(),
            "web login binds only to loopback; use SSH -L or a TLS reverse proxy"
        );
        let mut listen = options.listen;
        if let Some(port) = options.port {
            listen.set_port(port);
        }
        Some(web::WebState::new(listen))
    };
    let mut web_task = if let Some((_, cancel, task)) = web_bootstrap {
        let runtime_stop = stop.clone();
        tokio::spawn(async move {
            runtime_stop.cancelled().await;
            cancel.cancel();
        });
        Some(task)
    } else if let Some(web) = &web {
        let listener = tokio::net::TcpListener::bind(web.address)
            .await
            .context("cannot bind web listener")?;
        Some(tokio::spawn(web::serve_listener(
            web.clone(),
            listener,
            stop.clone(),
        )))
    } else {
        None
    };
    let signal = stop.clone();
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("SIGTERM");
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        signal.cancel();
    });
    let mut names: Vec<_> = config.accounts.keys().cloned().collect();
    names.sort_by_key(|n| n != &selected_account);
    let mut workers = tokio::task::JoinSet::new();
    let mut owner_ids = std::collections::HashSet::new();
    for name in names {
        crate::config::validate_account(&name)?;
        let mut connection = Connection::open_with_proxy(
            &root,
            &format!("user-{name}"),
            &credentials,
            key.clone(),
            options.proxy.as_deref(),
        )?;
        if !tokio::time::timeout(Duration::from_secs(30), connection.client.is_authorized())
            .await??
        {
            if options.no_auth && !config.accounts[&name].needs_login {
                connection.close().await?;
                tracing::warn!(account=%name,"not authorized; skipped");
                continue;
            }
            if let Some(web) = &web {
                let login = Arc::new(auth::Login::new(
                    connection.client.clone(),
                    connection.session.clone(),
                    credentials.clone(),
                ));
                *web.login.write().await = Some(login.clone());
                println!("Open the one-time login link: {}", web.link());
                loop {
                    tokio::select! {
                        _=stop.cancelled()=>break,
                        _=login.ready.notified()=>break,
                        update=connection.updates.as_mut().expect("login updates").recv()=>{
                            if let Some(update)=update {if let Err(e)=login.observe(update).await{tracing::warn!(error=%e,"QR login update failed");}}
                            else{anyhow::bail!("login connection closed");}
                        }
                    }
                }
                if stop.is_cancelled() {
                    connection.close().await?;
                    break;
                }
                *web.login.write().await = None;
            } else {
                tokio::select! {_=stop.cancelled()=>{connection.close().await?;break;},r=auth::terminal_login(&connection.client,&credentials,options.phone.as_deref(),Some(connection.session.clone()),connection.updates.as_mut())=>{r?;}}
            }
        }
        let me = connection.client.get_me().await?;
        ensure!(
            !me.is_bot(),
            "user session must be a personal account, not a bot"
        );
        let owner = me.id().bare_id_unchecked();
        ensure!(
            owner_ids.insert(owner),
            "Telegram account {owner} is configured in multiple slots; remove the duplicate slot from config.json"
        );
        if let Some(expected) = config.accounts[&name].owner_id {
            ensure!(owner == expected, "account identity changed");
        }
        config.accounts.get_mut(&name).unwrap().owner_id = Some(owner);
        config.accounts.get_mut(&name).unwrap().needs_login = false;
        config.save(&root)?;
        connection.session.flush()?;
        if let Some(w) = &web {
            w.account_ready(owner);
        }
        let db = Arc::new(Database::open(&root, owner)?);
        db.set("RustRuntime", "restart", false)?;
        let modules = Registry::load(&root, owner)?;
        for module in modules.list() {
            for command in module.commands {
                ensure!(
                    native.command(&command.name).is_none(),
                    "installed package conflicts with a native command: {}",
                    command.name
                );
            }
        }
        let mut token = config.accounts[&name].bot_token.clone();
        if token.is_none() && !options.no_bot {
            let username = format!("atarustia_{}_{}_bot", owner, rand::random::<u16>());
            match tokio::time::timeout(
                Duration::from_secs(150),
                crate::botfather::provision(&connection.client, &username),
            )
            .await
            {
                Ok(Ok(t)) => {
                    config.accounts.get_mut(&name).unwrap().bot_token = Some(t.clone());
                    config.save(&root)?;
                    token = Some(t);
                }
                _ => tracing::warn!(
                    "inline bot setup failed; use .ch_atarustia_bot in Saved Messages to retry"
                ),
            }
        }
        let mut secrets = vec![
            credentials.api_hash.clone(),
            me.phone().unwrap_or_default().to_owned(),
        ];
        if let Some(t) = &token {
            secrets.push(t.clone());
        }
        for secret in &secrets {
            logs.add_secret(secret.clone());
        }
        let context = Arc::new(Context {
            client: connection.client.clone(),
            db,
            modules: Mutex::new(modules),
            owner,
            username: me.username().map(str::to_owned),
            root: root.clone(),
            start: Instant::now(),
            allow_shell: options.allow_shell,
            stop: stop.clone(),
            web: web.clone(),
            secrets,
            rate: Mutex::new(Default::default()),
            seen: Mutex::new(Default::default()),
            logs: logs.clone(),
            access: Default::default(),
            events: Default::default(),
            maintenance: Default::default(),
            account: name.clone(),
            forms: Default::default(),
            bot: Default::default(),
            jobs: Default::default(),
            api_limits: Default::default(),
            native: native.clone(),
            semaphore: Arc::new(tokio::sync::Semaphore::new(32)),
        });
        tracing::info!(owner,account=%name,"account ready; send .help to Saved Messages");
        context.native.ready(&context.services()).await?;
        if let Some(token) = token.filter(|_| !options.no_bot) {
            let bot_id = token.split(':').next().context("invalid bot token")?;
            let bot = Connection::open_with_proxy(
                &root,
                &format!("bot-{name}-{bot_id}"),
                &credentials,
                key.clone(),
                options.proxy.as_deref(),
            )?;
            let ctx = context.clone();
            let hash = credentials.api_hash.clone();
            workers.spawn(async move {
                if let Err(e) = crate::inline::run(bot, ctx, token, hash).await {
                    tracing::error!(error=%e,"inline bot stopped; user account remains available");
                }
                Ok::<_, anyhow::Error>(())
            });
        } else {
            tracing::info!(
                "inline bot disabled; set ATARUSTIA_BOT_TOKEN or --bot-token to enable it"
            );
        }
        let ctx = context.clone();
        let setup = context.clone();
        workers.spawn(async move{if !setup.db.get("RustSetup","ready").as_bool().unwrap_or(false){match tokio::time::timeout(Duration::from_secs(120),crate::content::setup(&setup.client,&setup.db)).await{Ok(Ok(_))=>{setup.db.set("RustSetup","ready",true)?;let _=setup.client.send_message(grammers_session::types::PeerId::self_user().to_ambient_ref(),format!("⛈ atarustia Rust\n{}help — commands\n{}config — module settings\n{}set_backup_period 24 — daily backups",setup.db.prefix(),setup.db.prefix(),setup.db.prefix())).await;},_=>tracing::warn!("content setup failed; use .setup to retry")}}Ok::<_,anyhow::Error>(())});
        workers.spawn(async move { account_loop(connection, ctx).await });
    }
    if workers.is_empty() {
        stop.cancel();
    }
    let mut failure = None;
    loop {
        tokio::select! {
            _=stop.cancelled()=>break,
            result=workers.join_next(),if !workers.is_empty()=>match result {
                Some(Ok(Ok(())))=>{},Some(Ok(Err(e)))=>{tracing::error!(error=%e,"account worker stopped");failure=Some(e);},Some(Err(e))=>{failure=Some(e.into());},None=>{}
            },
            result=async{web_task.as_mut().unwrap().await},if web_task.is_some()=>{
                web_task=None;match result{Ok(Ok(()))=>{},Ok(Err(e))=>{failure=Some(e);stop.cancel();},Err(e)=>{failure=Some(e.into());stop.cancel();}}
            }
        }
        if workers.is_empty() {
            stop.cancel();
        }
    }
    stop.cancel();
    let drain = async {
        while let Some(r) = workers.join_next().await {
            if let Ok(Err(e)) = r {
                tracing::error!(error=%e,"shutdown error");
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(10), drain)
        .await
        .is_err()
    {
        workers.abort_all();
    }
    if let Some(task) = web_task {
        let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
    }
    let mut restart = false;
    for account in config.accounts.values() {
        if let Some(owner) = account.owner_id {
            let db = Database::open(&root, owner)?;
            restart |= db.get("RustRuntime", "restart").as_bool().unwrap_or(false);
        }
    }
    if let Some(e) = failure {
        return Err(e);
    }
    Ok(restart)
}
async fn account_loop(mut connection: Connection, context: Arc<Context>) -> Result<()> {
    let mut updates = connection
        .client
        .stream_updates(
            connection
                .updates
                .take()
                .context("update receiver missing")?,
            grammers_client::sender::UpdatesConfiguration { catch_up: false },
        )
        .await
        .map_err(anyhow::Error::from_boxed)?;
    let mut flush = tokio::time::interval(Duration::from_secs(5));
    let mut backups = tokio::time::interval(Duration::from_secs(30));
    let mut module_updates = tokio::time::interval(Duration::from_secs(1800));
    let mut logs_tick = tokio::time::interval(Duration::from_secs(30));
    let mut log_hash = String::new();
    let mut tasks = tokio::task::JoinSet::new();
    let mut loop_tick = tokio::time::interval(Duration::from_secs(1));
    let mut scheduler = crate::scheduler::Scheduler::default();
    loop {
        tokio::select! {
            _=context.stop.cancelled()=>break,
            _=logs_tick.tick()=>{if context.db.get("RustLogs","telegram").as_bool().unwrap_or(false){let bytes=context.logs.snapshot();let text=context.logs.redact(String::from_utf8_lossy(&bytes).to_string());let errors=text.lines().filter(|l|l.contains("ERROR")||l.contains("WARN")).collect::<Vec<_>>().join("\n");let hash=crate::modules::digest(errors.as_bytes());if !errors.is_empty()&&hash!=log_hash{log_hash=hash;let ctx=context.clone();tasks.spawn(async move{if let Ok(Some((peer,topic)))=crate::content::topic_destination(&ctx.client,&ctx.db,"Logs").await{let tail:String=errors.chars().rev().take(3000).collect::<String>().chars().rev().collect();let _=ctx.client.send_message(peer,grammers_client::message::InputMessage::new().text(tail).reply_to(Some(topic))).await;}});}}},
            _=module_updates.tick()=>{let packages=context.modules.lock().unwrap().export();let ctx=context.clone();tasks.spawn(async move{for (name,url,package) in Registry::updates(packages).await{let hash=match ctx.modules.lock().unwrap().stage(package){Ok(hash)=>hash,Err(_)=>continue};let notified=ctx.db.get("RustModuleUpdates",&name);if notified.as_str()==Some(&hash){continue;}let text=format!("Module update: {name}\n{url}\nReview the new source. Install exact revision with:\n{}approve {hash}",ctx.db.prefix());if ctx.client.send_message(grammers_session::types::PeerId::self_user().to_ambient_ref(),text).await.is_ok(){let _=ctx.db.set("RustModuleUpdates",&name,hash);}}});},
            _=loop_tick.tick()=>{let packages=context.modules.lock().unwrap().export();for (package,task) in scheduler.due(&packages,&context.db){if let Ok(permit)=context.semaphore.clone().try_acquire_owned(){let ctx=context.clone();tasks.spawn(async move{let _permit=permit;if let Err(e)=ctx.run_loop(package,task).await{tracing::warn!(error=%e,"module loop failed");}});}}},
            _=flush.tick()=>{connection.session.flush()?;},
            _=backups.tick()=>{
                let hours=context.db.get("RustBackup","hours").as_u64().unwrap_or(0);
                let last=context.db.get("RustBackup","last").as_u64().unwrap_or(crate::security::now());
                if hours>0 && crate::security::now().saturating_sub(last)>=hours.saturating_mul(3600){
                    let bytes={let modules=context.modules.lock().unwrap();backup::create(&context.db,&modules,context.owner)?};
                    backup::local(&context.root,&bytes,context.owner)?;
                    context.db.set("RustBackup","last",crate::security::now())?;
                    let ctx=context.clone();tasks.spawn(async move{if let Err(e)=crate::telegram::send_backup(&ctx.client,&ctx.db,bytes,"Scheduled confidential backup").await{tracing::warn!(error=%e,"backup upload failed; local copy saved");}});
                }
            },
            update=updates.next()=>{
                let update=update?;
                for module in context.native.list(){if let Ok(permit)=context.semaphore.clone().try_acquire_owned(){let services=context.services();let update=update.clone();tasks.spawn(async move{let _permit=permit;if let Err(e)=module.update(&services,&update).await{tracing::warn!(module=module.name(),error=%e,"native update failed");}});}}
                let (message,edited)=match update {Update::NewMessage(m)=>(Some(m),false),Update::MessageEdited(m)=>(Some(m),true),Update::Raw(raw)=>{let ctx=context.clone();let event=crate::modules::Event{text:format!("{:?}",raw.raw).chars().take(16384).collect(),chat:ctx.owner,sender:Some(ctx.owner),outgoing:false,command:false,edited:false,id:0,kind:"raw".into(),data:serde_json::Value::Null};if let Ok(permit)=ctx.semaphore.clone().try_acquire_owned(){tasks.spawn(async move{let _permit=permit;if let Err(e)=ctx.handle_event(event).await{tracing::warn!(error=%e,"raw watcher failed");}});}(None,false)},_=>(None,false)};
                if let Some(m)=message && let Ok(permit)=context.semaphore.clone().try_acquire_owned(){let ctx=context.clone();tasks.spawn(async move{
                    let _permit=permit;let m=m.into_inner();
                    let event=crate::modules::Event{text:m.text().into(),chat:m.peer_id().bot_api_dialog_id_unchecked(),sender:m.sender_id().filter(|s|s.kind()==grammers_session::types::PeerKind::User).map(|s|s.bare_id_unchecked()),outgoing:m.outgoing(),command:matches!(crate::dispatcher::parse(m.text(),&ctx.db.prefix(),ctx.username.as_deref(),m.outgoing()),crate::dispatcher::Parsed::Command{..}),edited,id:m.id(),kind:"message".into(),data:serde_json::to_value(crate::botapi::Message::from_native(&m)).unwrap_or_default()};
                    if let Err(e)=ctx.handle_event(event).await{tracing::warn!(error=%e,"watcher failed");}
                    if let Err(e)=ctx.handle(m,edited).await {tracing::warn!(error=%crate::output::censor(e.to_string(),&ctx.secrets),"dispatch failed");}
                });}
            },
            Some(result)=tasks.join_next(),if !tasks.is_empty()=>{if let Err(e)=result{tracing::error!(error=%e,"handler panicked");}}
        }
    }
    tasks.abort_all();
    context.native.unload(&context.services()).await;
    context.jobs.cancel_all();
    while tasks.join_next().await.is_some() {}
    updates
        .sync_update_state()
        .await
        .map_err(anyhow::Error::from_boxed)?;
    drop(updates);
    connection.close().await
}
