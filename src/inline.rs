use crate::{commands::Context, security, telegram::Connection};
use anyhow::{Context as _, Result};
use grammers_client::{
    message::{Button, InputMessage, ReplyMarkup},
    update::{Article, Update},
};
use std::{sync::Arc, time::Duration};
#[derive(Clone)]
pub struct BotHandle {
    pub bot: crate::bot::Bot,
}
impl BotHandle {
    pub async fn form(
        &self,
        context: &Context,
        message: &grammers_client::message::Message,
        screen: crate::forms::Screen,
    ) -> Result<()> {
        let owner = message
            .sender_id()
            .map(|s| s.bare_id_unchecked())
            .unwrap_or(context.owner);
        let (id, _body) = context.forms.lock().unwrap().create(
            screen,
            owner,
            security::owners(&context.db, context.owner),
        )?;
        if let crate::dispatcher::Parsed::Command { name, args } = crate::dispatcher::parse(
            message.text(),
            &context.db.prefix_for(Some(owner), context.owner),
            context.username.as_deref(),
            message.outgoing(),
        ) {
            let aliases =
                serde_json::from_value(context.db.get("Settings", "aliases")).unwrap_or_default();
            if let Ok((name, _)) = crate::dispatcher::resolve(&name, &args, &aliases) {
                let module = context.modules.lock().unwrap().command(&name);
                let module_name = module.as_ref().map(|(p, _)| p.manifest.name.clone());
                let permission = module
                    .map(|(_, c)| c.permission)
                    .unwrap_or_else(|| context.permission(&name));
                context.forms.lock().unwrap().set_gate(
                    &id,
                    crate::forms::Gate {
                        message: message.clone(),
                        command: name,
                        module: module_name,
                        permission,
                    },
                )?;
            }
        }
        // Owner starts an inline query; its update must be handled concurrently by the bot loop.
        let bot = context
            .client
            .resolve_username(&self.bot.username)
            .await?
            .context("inline bot not found")?
            .to_ref()
            .await
            .map_err(anyhow::Error::from_boxed)?
            .context("bot peer missing")?;
        let result = async {
            loop {
                let outcome = async {
                    let peer = crate::telegram::peer(message).await?;
                    let results = context
                        .client
                        .invoke(
                            &grammers_client::tl::functions::messages::GetInlineBotResults {
                                bot: bot.into(),
                                peer: peer.into(),
                                geo_point: None,
                                query: format!("unit:{id}"),
                                offset: String::new(),
                            },
                        )
                        .await?;
                    let grammers_client::tl::enums::messages::BotResults::Results(results) =
                        results;
                    let result = results
                        .results
                        .first()
                        .context("inline form returned no result")?;
                    let result_id = match result {
                        grammers_client::tl::enums::BotInlineResult::Result(r) => r.id.clone(),
                        grammers_client::tl::enums::BotInlineResult::BotInlineMediaResult(r) => {
                            r.id.clone()
                        }
                    };
                    let random_id = rand::random();
                    let sent = context
                        .client
                        .invoke(
                            &grammers_client::tl::functions::messages::SendInlineBotResult {
                                silent: false,
                                background: false,
                                clear_draft: false,
                                hide_via: false,
                                peer: peer.into(),
                                reply_to: Some(
                                    grammers_client::tl::types::InputReplyToMessage {
                                        reply_to_msg_id: message.id(),
                                        top_msg_id: crate::telegram::topic(message),
                                        reply_to_peer_id: None,
                                        quote_text: None,
                                        quote_entities: None,
                                        quote_offset: None,
                                        monoforum_peer_id: None,
                                        todo_item_id: None,
                                        poll_option: None,
                                    }
                                    .into(),
                                ),
                                random_id,
                                query_id: results.query_id,
                                id: result_id,
                                schedule_date: None,
                                send_as: None,
                                quick_reply_shortcut: None,
                                allow_paid_stars: None,
                            },
                        )
                        .await?;
                    // Do not bind an unrelated message carried by the same Updates envelope.
                    if let Ok(sent) = crate::bot_response::sent(&sent, peer, random_id, "", None) {
                        context.forms.lock().unwrap().bind_message(
                            &id,
                            sent.chat_id,
                            sent.message_id,
                        );
                    }
                    Ok::<_, anyhow::Error>(())
                }
                .await;
                match outcome {
                    Ok(()) => break,
                    Err(_) if context.forms.lock().unwrap().plain_fallback(&id) => continue,
                    Err(e) => return Err(e),
                }
            }
            Ok::<_, anyhow::Error>(())
        };
        match tokio::time::timeout(Duration::from_secs(15), result).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                context.forms.lock().unwrap().remove(&id);
                return Err(e);
            }
            Err(_) => {
                context.forms.lock().unwrap().remove(&id);
                anyhow::bail!("inline form timed out");
            }
        }
        if message.outgoing() && context.forms.lock().unwrap().configuration(&id) {
            // Config/settings replace the owner's command, as the original form does.
            let _ = message.delete().await;
        }
        Ok(())
    }
}

async fn screen(context: &Context, user: i64, page: usize) -> Result<InputMessage> {
    let help = crate::help::build(
        context,
        user,
        None,
        &crate::help::State {
            page,
            ..Default::default()
        },
    )
    .await?;
    let mut forms = context.forms.lock().unwrap();
    let (id, _) = forms.create(help, user, vec![context.owner])?;
    forms.plain_fallback(&id);
    let input = forms.render(&id, 0)?;
    Ok(input)
}
pub async fn run(
    connection: Connection,
    context: Arc<Context>,
    token: String,
    hash: String,
) -> Result<()> {
    let result = run_inner(connection, context.clone(), token, hash).await;
    *context.bot.write().await = None;
    result
}
async fn run_inner(
    mut connection: Connection,
    context: Arc<Context>,
    token: String,
    hash: String,
) -> Result<()> {
    if !connection.client.is_authorized().await? {
        tokio::time::timeout(
            Duration::from_secs(30),
            connection.client.bot_sign_in(&token, &hash),
        )
        .await??;
    }
    let me = connection.client.get_me().await?;
    let expected = token
        .split(':')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .context("invalid bot token")?;
    anyhow::ensure!(
        me.id().bare_id_unchecked() == expected,
        "bot session belongs to another bot; use a different account slot or remove its session after stopping"
    );
    connection.session.flush()?;
    *context.bot.write().await = Some(BotHandle {
        bot: crate::bot::Bot {
            client: connection.client.clone(),
            id: expected,
            username: me.username().context("bot username missing")?.into(),
            session: Some(connection.handle.session.clone()),
        },
    });
    tracing::info!(
        bot = me.username().unwrap_or("unknown"),
        "inline bot connected over MTProto"
    );
    if let Some(bot) = context.bot.read().await.clone() {
        let greet = async {
            let peer = context
                .client
                .resolve_username(&bot.bot.username)
                .await?
                .context("inline bot not found")?
                .to_ref()
                .await
                .map_err(anyhow::Error::from_boxed)?
                .context("bot peer missing")?;
            let init = context
                .client
                .send_message(peer, "/start ataraxia init")
                .await?;
            let _ = init.delete().await;
            crate::presentation::greet(&context, &bot.bot).await
        };
        if !matches!(
            tokio::time::timeout(Duration::from_secs(30), greet).await,
            Ok(Ok(()))
        ) {
            tracing::warn!("quickstart message deferred");
        }
    }
    if let Err(error) = crate::update_flow::complete_for(&context, true).await {
        tracing::warn!(error=%error,"restart report deferred");
    }
    let mut updates = connection
        .client
        .stream_updates(connection.updates.take().unwrap(), Default::default())
        .await
        .map_err(anyhow::Error::from_boxed)?;
    let mut flush = tokio::time::interval(Duration::from_secs(5));
    let mut handlers = tokio::task::JoinSet::new();
    let permits = Arc::new(tokio::sync::Semaphore::new(32));
    loop {
        tokio::select! {
            _=context.stop.cancelled()=>break,
            _=flush.tick()=>{connection.session.flush()?;context.forms.lock().unwrap().clean();},
            update=updates.next()=>{
                let update=update?;
                if let Ok(permit)=permits.clone().try_acquire_owned(){let ctx=context.clone();handlers.spawn(async move{let _permit=permit;if let Err(e)=handle(&ctx,update).await {tracing::warn!(error=%crate::output::censor(e.to_string(),&ctx.secrets),"inline handler failed");}});}
            }
            Some(result)=handlers.join_next(),if !handlers.is_empty()=>{if let Err(e)=result{tracing::warn!(error=%e,"inline task failed");}}
        }
    }
    *context.bot.write().await = None;
    handlers.abort_all();
    while handlers.join_next().await.is_some() {}
    updates
        .sync_update_state()
        .await
        .map_err(anyhow::Error::from_boxed)?;
    drop(updates);
    connection.close().await
}
async fn handle(context: &Context, update: Update) -> Result<()> {
    if crate::config_ui::handle_bot_update(context, &update).await? {
        return Ok(());
    }
    if let Update::CallbackQuery(query) = &update
        && let Some(lang) = std::str::from_utf8(query.data())
            .ok()
            .and_then(|d| d.strip_prefix("ataraxia/lang/"))
    {
        if security::owners(&context.db, context.owner)
            .contains(&query.sender_id().bare_id_unchecked())
            && matches!(lang, "en" | "ru")
        {
            context.db.set("atarustia.translations", "lang", lang)?;
            query
                .answer()
                .text(crate::original::text(
                    lang == "en",
                    "quickstart",
                    "language_saved",
                ))
                .send()
                .await?;
            let body = crate::presentation::quickstart(
                lang == "en",
                context
                    .db
                    .get("RustRuntime", "premium")
                    .as_bool()
                    .unwrap_or(false),
            )?;
            if let Some(bot) = context.bot.read().await.clone() {
                bot.bot.edit_callback(query, body).await?;
            }
        } else {
            query
                .answer()
                .alert(crate::original::text(
                    context.db.language().starts_with("en"),
                    "$inline",
                    "button403",
                ))
                .send()
                .await?;
        }
        return Ok(());
    }
    if let Update::CallbackQuery(query) = &update
        && let Some(token) = std::str::from_utf8(query.data())
            .ok()
            .and_then(|data| data.strip_prefix("authorize_web_"))
    {
        let owner = security::owners(&context.db, context.owner)
            .contains(&query.sender_id().bare_id_unchecked());
        if owner && let Some(web) = &context.web {
            web.confirm(token);
            query.answer().send().await?;
        } else {
            query.answer().alert("403").send().await?;
        }
        return Ok(());
    }
    // Form delivery waits for its inline query while the originating module owns
    // the events lock. Route this internal query before any module watcher.
    if let Update::InlineQuery(query) = &update
        && let Some(unit) = query.text().strip_prefix("unit:")
    {
        let owners = security::owners(&context.db, context.owner);
        let results = if owners.contains(&query.sender_id().bare_id_unchecked()) {
            context
                .forms
                .lock()
                .unwrap()
                .inline_result(unit)
                .map(|r| vec![r])
                .unwrap_or_default()
        } else {
            vec![]
        };
        query.answer(results).private().cache_time(0).send().await?;
        return Ok(());
    }
    for module in context.native.list() {
        if let Err(e) = module.update(&context.services(), &update).await {
            tracing::warn!(module=module.name(),error=%e,"native bot event failed");
        }
    }
    let owners = security::owners(&context.db, context.owner);
    let bot_id = context
        .bot
        .read()
        .await
        .as_ref()
        .map(|b| b.bot.id)
        .unwrap_or(0);
    if let Some(event) =
        crate::bot_events::from_update(&update, bot_id).filter(|event| event.name != "bot_message")
    {
        let data = event.data;
        let mut event = crate::modules::Event {
            text: data
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            chat: event.chat.unwrap_or(context.owner),
            sender: event.sender,
            outgoing: false,
            command: false,
            edited: event.name.starts_with("edited_"),
            id: event.message_id.unwrap_or(0),
            kind: event.name,
            data,
            ..Default::default()
        };
        if let Ok(converted) = serde_json::from_value::<crate::botapi::Message>(event.data.clone())
        {
            event.enrich_from_botapi(&converted);
        }
        context.handle_event(event).await?;
    }
    if let Update::NewMessage(message) = &update {
        if let Some(reply) = message.reply_to_message_id() {
            let chat = message.peer_id().bot_api_dialog_id_unchecked();
            let user = message
                .sender_id()
                .map(|s| s.bare_id_unchecked())
                .unwrap_or(0);
            let prompt = context.forms.lock().unwrap().take_input(chat, reply, user);
            if let Some(prompt) = prompt {
                anyhow::ensure!(owners.contains(&user), "configuration permission revoked");
                if message.text() == "/cancel" {
                    message.reply("Cancelled").await?;
                    return Ok(());
                }
                let value = serde_json::from_str(message.text())
                    .unwrap_or_else(|_| serde_json::Value::String(message.text().into()));
                match crate::screens::set_config(context, &prompt.module, &prompt.key, value) {
                    Ok(()) => {
                        message.reply(context.tr("saved", &[])).await?;
                    }
                    Err(e) => {
                        message
                            .reply(format!(
                                "Invalid value: {}. Press the edit button again to retry.",
                                crate::output::error_message(&e.to_string(), &context.secrets)
                            ))
                            .await?;
                    }
                }
                crate::module_lifecycle::config_changes(context).await;
                if let Ok(screen) =
                    crate::screens::config_for_unit(context, &prompt.module, &prompt.unit)
                {
                    let body = context.forms.lock().unwrap().replace(&prompt.unit, screen);
                    if let Ok(body) = body {
                        message.respond(body).await?;
                    }
                }
                let _ = message.delete().await;
                return Ok(());
            }
        }
        let converted = crate::botapi::Message::from_native(message);
        let mut event = crate::modules::Event {
            text: message.text().into(),
            chat: message.peer_id().bot_api_dialog_id_unchecked(),
            sender: message
                .sender_id()
                .filter(|s| s.kind() == grammers_session::types::PeerKind::User)
                .map(|s| s.bare_id_unchecked()),
            outgoing: message.outgoing(),
            command: message.text().starts_with('/'),
            edited: false,
            id: message.id(),
            kind: "bot_message".into(),
            data: serde_json::to_value(&converted)?,
            ..Default::default()
        };
        event.enrich_from_botapi(&converted);
        if !matches!(
            message.peer(),
            Some(grammers_client::peer::Peer::Channel(_))
        ) && let Err(e) = context.handle_event(event).await
        {
            tracing::warn!(error=%e,"bot watcher failed");
        }
    }
    match update {
        Update::NewMessage(m) if !m.outgoing() => {
            if m.text() == "/start" {
                let bot = context
                    .bot
                    .read()
                    .await
                    .clone()
                    .context("inline bot unavailable")?;
                let bytes = include_bytes!("../assets/ataraxia.png").to_vec();
                let media = crate::media::Media {
                    kind: crate::file_id::FileKind::Photo,
                    source: crate::media::Source::Bytes {
                        name: "ataraxia.png".into(),
                        data: bytes,
                    },
                    ..Default::default()
                };
                bot.bot
                    .send_media(
                        crate::telegram::peer(&m).await?,
                        &media,
                        crate::presentation::start_card(context.db.language().starts_with("en"))?,
                    )
                    .await?;
            } else if m.text() == "/help"
                && owners.contains(&m.sender_id().map(|s| s.bare_id_unchecked()).unwrap_or(0))
            {
                let screen = crate::help::build(
                    context,
                    m.sender_id()
                        .map(|id| id.bare_id_unchecked())
                        .unwrap_or(context.owner),
                    None,
                    &Default::default(),
                )
                .await?;
                let (id, _) = context.forms.lock().unwrap().create(
                    screen,
                    m.sender_id()
                        .map(|s| s.bare_id_unchecked())
                        .unwrap_or(context.owner),
                    vec![context.owner],
                )?;
                let body = {
                    let mut forms = context.forms.lock().unwrap();
                    forms.plain_fallback(&id);
                    forms.render(&id, 0)?
                };
                m.respond(body).await?;
            }
        }
        Update::CallbackQuery(q) => {
            let data = std::str::from_utf8(q.data()).unwrap_or_default();
            if data.starts_with("f:") {
                let click = context
                    .forms
                    .lock()
                    .unwrap()
                    .click(data, q.sender_id().bare_id_unchecked());
                let click = match click {
                    Ok(c) => c,
                    Err(_) => {
                        q.answer()
                            .alert("Form expired or access denied")
                            .send()
                            .await?;
                        return Ok(());
                    }
                };
                if let grammers_client::tl::enums::Update::InlineBotCallbackQuery(raw) = &q.raw {
                    context
                        .forms
                        .lock()
                        .unwrap()
                        .bind_inline(&click.unit, crate::file_id::encode_inline_id(&raw.msg_id));
                }
                use crate::forms::Action;
                if matches!(
                    click.action,
                    Action::Configure { .. }
                        | Action::Settings { .. }
                        | Action::SettingsMaintenance { .. }
                ) && !owners.contains(&q.sender_id().bare_id_unchecked())
                {
                    q.answer()
                        .alert(crate::config_ui::ui(context).text("owner_only"))
                        .send()
                        .await?;
                    return Ok(());
                }
                if !owners.contains(&q.sender_id().bare_id_unchecked()) {
                    let gate = context.forms.lock().unwrap().gate(&click.unit);
                    let inline_command = context.forms.lock().unwrap().inline_command(&click.unit);
                    anyhow::ensure!(
                        gate.is_some()
                            || inline_command.as_ref().is_some_and(|command| {
                                crate::security_commands::inline_allowed(
                                    &context.db,
                                    context.owner,
                                    q.sender_id().bare_id_unchecked(),
                                    command,
                                )
                            })
                            || inline_command.is_none()
                                && matches!(
                                    &click.action,
                                    crate::forms::Action::HelpNavigate { .. }
                                        | crate::forms::Action::Help { .. }
                                        | crate::forms::Action::Close
                                        | crate::forms::Action::Page { .. }
                                )
                                && crate::security_commands::inline_allowed(
                                    &context.db,
                                    context.owner,
                                    q.sender_id().bare_id_unchecked(),
                                    "help"
                                ),
                        "delegated form has no originating permission gate"
                    );
                    if let Some(gate) = gate {
                        anyhow::ensure!(
                            gate.message
                                .sender_id()
                                .is_some_and(|id| id.bare_id_unchecked()
                                    == q.sender_id().bare_id_unchecked())
                                && context
                                    .access
                                    .check(
                                        &context.client,
                                        &context.db,
                                        context.owner,
                                        &gate.message,
                                        &gate.command,
                                        gate.module.as_deref(),
                                        context.permission_with_default(
                                            &gate.command,
                                            gate.permission
                                        )
                                    )
                                    .await?,
                            "form command access revoked"
                        );
                    }
                }
                let message = match click.action {
                    Action::Configure { state, operation } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "{}",
                            crate::config_ui::ui(context).text("owner_only")
                        );
                        let (screen, toast) =
                            if matches!(operation, crate::config_ui::Operation::Navigate) {
                                (
                                    crate::config_ui::build(context, &state, Some(&click.unit))?,
                                    None,
                                )
                            } else {
                                crate::config_ui::changed_screen(
                                    context,
                                    &state,
                                    &operation,
                                    None,
                                    &click.unit,
                                )?
                            };
                        crate::module_lifecycle::config_changes(context).await;
                        if let Some(toast) = toast {
                            q.answer().text(toast).send().await?;
                        } else {
                            q.answer().send().await?;
                        }
                        context.forms.lock().unwrap().replace(&click.unit, screen)?
                    }
                    Action::Settings { key, value } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "owner only settings"
                        );
                        if let Some(key) = key {
                            anyhow::ensure!(
                                matches!(
                                    key.as_str(),
                                    "no_nickname" | "grep" | "inlinelogs" | "suggest_subscribe"
                                ),
                                "unknown setting"
                            );
                            context.db.set("atarustia.main", &key, value)?;
                            if key == "no_nickname" && value && context.db.prefix() == "." {
                                q.answer()
                                    .alert(crate::original::text(
                                        crate::config_ui::ui(context).english,
                                        "settings",
                                        "nonick_warning",
                                    ))
                                    .send()
                                    .await?;
                            } else {
                                q.answer().text("Configuration value saved!").send().await?;
                            }
                        }
                        context
                            .forms
                            .lock()
                            .unwrap()
                            .replace(&click.unit, crate::config_ui::settings(context))?
                    }
                    Action::SettingsMaintenance { restart, confirmed } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "owner only settings"
                        );
                        if confirmed {
                            let action = if restart {
                                Action::Restart
                            } else {
                                let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
                                let changes = crate::updater::check(root).await?;
                                Action::Update {
                                    current: changes.current,
                                    incoming: changes.incoming,
                                    retry: false,
                                }
                            };
                            return crate::update_flow::callback(context, &q, click.unit, action)
                                .await;
                        }
                        q.answer().send().await?;
                        context.forms.lock().unwrap().replace(
                            &click.unit,
                            crate::config_ui::settings_confirmation(
                                crate::config_ui::ui(context).english,
                                restart,
                            ),
                        )?
                    }
                    action @ (Action::Update { .. } | Action::Restart | Action::UpdateDismiss) => {
                        return crate::update_flow::callback(context, &q, click.unit, action).await;
                    }
                    Action::Copy { .. } | Action::SwitchInline { .. } | Action::Disabled => {
                        q.answer().send().await?;
                        return Ok(());
                    }
                    Action::Page { page } => {
                        let state = context.forms.lock().unwrap().help_state(&click.unit);
                        if let Some(state) = state {
                            let gate = context.forms.lock().unwrap().gate(&click.unit);
                            let screen = crate::help::build(
                                context,
                                q.sender_id().bare_id_unchecked(),
                                gate.as_ref().map(|g| &g.message),
                                &state,
                            )
                            .await?;
                            context.forms.lock().unwrap().replace(&click.unit, screen)?;
                        }
                        context.forms.lock().unwrap().render(&click.unit, page)?
                    }
                    Action::Close => {
                        let target = {
                            let forms = context.forms.lock().unwrap();
                            forms
                                .configuration(&click.unit)
                                .then(|| forms.destination(&click.unit))
                                .flatten()
                        };
                        if let Some((chat, id)) = target {
                            let peer =
                                grammers_session::types::PeerId::from_bot_api_dialog_id(chat)
                                    .context("invalid form chat")?;
                            context
                                .client
                                .delete_messages(peer.to_ambient_ref(), &[id])
                                .await?;
                            context.forms.lock().unwrap().remove(&click.unit);
                            q.answer().send().await?;
                            return Ok(());
                        }
                        context.forms.lock().unwrap().remove(&click.unit);
                        InputMessage::new()
                            .text("✕")
                            .reply_markup(ReplyMarkup::from_buttons(&[]))
                    }
                    action @ (Action::Help { .. }
                    | Action::HelpNavigate { .. }
                    | Action::HelpToggle { .. }
                    | Action::HelpUnhideAll { .. }
                    | Action::HelpSettings { .. }) => {
                        crate::help::callback(context, &q, &click.unit, action).await?
                    }
                    Action::ConfigOpen { module } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "owner only configuration"
                        );
                        let s = crate::screens::config_for_unit(context, &module, &click.unit)?;
                        context.forms.lock().unwrap().replace(&click.unit, s)?
                    }
                    Action::ConfigSet { module, key, value } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "owner only configuration"
                        );
                        crate::screens::set_config(context, &module, &key, value)?;
                        crate::module_lifecycle::config_changes(context).await;
                        let screen =
                            crate::screens::config_for_unit(context, &module, &click.unit)?;
                        context.forms.lock().unwrap().replace(&click.unit, screen)?
                    }
                    Action::ConfigInput { module, key } => {
                        let user = q.sender_id().bare_id_unchecked();
                        anyhow::ensure!(owners.contains(&user), "owner only configuration");
                        let bot = context
                            .bot
                            .read()
                            .await
                            .clone()
                            .context("bot disconnected")?;
                        let prompt=bot.bot.client.send_message(q.sender_id().to_ambient_ref(),format!("Reply to this message with the JSON value for {module}.{key}. For text you may use a plain string. /cancel cancels. The input expires in 5 minutes.")).await?;
                        context.forms.lock().unwrap().input(
                            user,
                            prompt.id(),
                            crate::forms::Prompt {
                                unit: click.unit.clone(),
                                user,
                                module,
                                key,
                                expires: std::time::Instant::now() + Duration::from_secs(300),
                            },
                        )?;
                        q.answer()
                            .alert("Reply to the bot's private message")
                            .send()
                            .await?;
                        return Ok(());
                    }
                    Action::ConfigToggle { module, key }
                    | Action::ConfigReset { module, key }
                    | Action::ConfigStep { module, key, .. } => {
                        anyhow::ensure!(
                            owners.contains(&q.sender_id().bare_id_unchecked()),
                            "owner only configuration"
                        );
                        let action = context
                            .forms
                            .lock()
                            .unwrap()
                            .click(data, q.sender_id().bare_id_unchecked())?
                            .action;
                        let (step, reset) = match action {
                            Action::ConfigStep { step, .. } => (Some(step), false),
                            Action::ConfigReset { .. } => (None, true),
                            _ => (None, false),
                        };
                        crate::screens::apply_config(context, &module, &key, step, reset)?;
                        crate::module_lifecycle::config_changes(context).await;
                        let s = crate::screens::config_for_unit(context, &module, &click.unit)?;
                        context.forms.lock().unwrap().replace(&click.unit, s)?
                    }
                    Action::Module {
                        module,
                        function,
                        args,
                    } => {
                        // Delegated command forms cease to work when that user's rule is revoked.
                        let _lock = context.events.lock().await;
                        context
                            .forms
                            .lock()
                            .unwrap()
                            .click(data, q.sender_id().bare_id_unchecked())?;
                        if !owners.contains(&q.sender_id().bare_id_unchecked()) {
                            let command = context.forms.lock().unwrap().inline_command(&click.unit);
                            if let Some(command) = command {
                                anyhow::ensure!(
                                    crate::security_commands::inline_allowed(
                                        &context.db,
                                        context.owner,
                                        q.sender_id().bare_id_unchecked(),
                                        &command
                                    ),
                                    "inline command access revoked"
                                );
                            }
                        }
                        anyhow::ensure!(
                            crate::module_lifecycle::active(context, &module),
                            "module is suspended"
                        );
                        let p = context
                            .modules
                            .lock()
                            .unwrap()
                            .export()
                            .into_iter()
                            .find(|p| p.manifest.name == module)
                            .context("module unloaded")?;
                        let event = serde_json::json!({"kind":"callback","user":q.sender_id().bare_id_unchecked(),"args":args});
                        let chat = context
                            .forms
                            .lock()
                            .unwrap()
                            .gate(&click.unit)
                            .map(|gate| gate.message.peer_id().bot_api_dialog_id_unchecked());
                        let effect = crate::module_host::run_event(
                            context,
                            p,
                            function,
                            event,
                            crate::module_host::Mode {
                                chat,
                                message_id: None,
                                preview: false,
                            },
                        )
                        .await?;
                        let plan = crate::module_effects::Plan::new(&module, effect)?;
                        plan.require_callback_destination(chat.is_some())?;
                        let output = plan.commit(context, &module, false)?;
                        return crate::module_effects::callback(
                            context,
                            &module,
                            output,
                            &q,
                            &click.unit,
                        )
                        .await;
                    }
                };
                if let Some(bot) = context.bot.read().await.as_ref() {
                    let rich = context.forms.lock().unwrap().rich_source(&click.unit);
                    let media = context.forms.lock().unwrap().media(&click.unit);
                    if let Some(media) = media {
                        bot.bot.edit_callback_media(&q, message, &media).await?;
                    } else {
                        if rich.is_some() {
                            let result =
                                bot.bot.edit_callback_rich(&q, message.clone(), rich).await;
                            if result.is_err()
                                && context.forms.lock().unwrap().plain_fallback(&click.unit)
                            {
                                let plain = context.forms.lock().unwrap().render(&click.unit, 0)?;
                                bot.bot.edit_callback(&q, plain).await?;
                            } else {
                                result?;
                            }
                        } else {
                            bot.bot.edit_callback(&q, message).await?;
                        }
                    }
                }
                return Ok(());
            }
            if !owners.contains(&q.sender_id().bare_id_unchecked()) {
                q.answer().alert("Owner only").send().await?;
                return Ok(());
            }
            let data = std::str::from_utf8(q.data()).unwrap_or_default();
            if let Some(page) = data
                .strip_prefix("help:")
                .and_then(|s| s.parse::<usize>().ok())
            {
                q.answer()
                    .edit(screen(context, q.sender_id().bare_id_unchecked(), page).await?)
                    .await?;
            } else if data == "config" {
                q.answer().edit(InputMessage::new().text(format!("Prefix: {}\nLanguage: {}\nUse {}config in Saved Messages to edit module settings.",context.db.prefix(),context.db.language(),context.db.prefix())).reply_markup(ReplyMarkup::from_buttons(&[vec![Button::data("‹ Help",b"help:0".to_vec())]]))).await?;
            } else {
                q.answer().send().await?;
            }
        }
        Update::InlineQuery(q) => {
            let cmd = q.text().split_whitespace().next().unwrap_or("help");
            if !crate::security_commands::inline_allowed(
                &context.db,
                context.owner,
                q.sender_id().bare_id_unchecked(),
                cmd,
            ) {
                q.answer(Vec::<Article>::new()).private().send().await?;
                return Ok(());
            }
            let command = context
                .modules
                .lock()
                .unwrap()
                .command(cmd)
                .filter(|(_, c)| c.inline);
            if let Some((package, _)) = command {
                let _serial = context.events.lock().await;
                anyhow::ensure!(
                    crate::module_lifecycle::active(context, &package.manifest.name),
                    "module is suspended"
                );
                anyhow::ensure!(
                    context
                        .modules
                        .lock()
                        .unwrap()
                        .package(&package.manifest.name)
                        .is_some_and(|p| p.revision() == package.revision()),
                    "module revision changed"
                );
                let args = q
                    .text()
                    .split_once(char::is_whitespace)
                    .map(|(_, a)| a)
                    .unwrap_or("")
                    .to_owned();
                let name = cmd.to_owned();
                let module = package.manifest.name.clone();
                let effect = crate::module_host::run_command(
                    context,
                    package,
                    name,
                    args,
                    crate::module_host::Mode {
                        chat: None,
                        message_id: None,
                        preview: true,
                    },
                )
                .await?;
                let plan = crate::module_effects::Plan::new(&module, effect)?;
                plan.require_single_result()?;
                let output = plan.commit(context, &module, true)?;
                let result = crate::module_effects::inline(
                    context,
                    &module,
                    cmd,
                    output,
                    q.sender_id().bare_id_unchecked(),
                )?;
                q.answer([result]).private().cache_time(0).send().await?;
                return Ok(());
            }
            q.answer(
                crate::presentation::inline_catalog(context, q.sender_id().bare_id_unchecked())
                    .await?,
            )
            .private()
            .cache_time(0)
            .send()
            .await?;
        }
        Update::GuestChatQuery(q) => {
            if !owners.contains(
                &q.message
                    .sender_id()
                    .map(|s| s.bare_id_unchecked())
                    .unwrap_or(0),
            ) {
                return Ok(());
            }
            let text = q
                .message
                .text()
                .split_whitespace()
                .skip_while(|s| s.starts_with('@'))
                .collect::<Vec<_>>()
                .join(" ");
            {
                let (name, args) = text.split_once(char::is_whitespace).unwrap_or((&text, ""));
                let package = context
                    .modules
                    .lock()
                    .unwrap()
                    .command(name)
                    .filter(|(_, c)| c.guest);
                if let Some((package, _)) = package {
                    let module = package.manifest.name.clone();
                    let name = name.to_owned();
                    let args = args.to_owned();
                    let _serial = context.events.lock().await;
                    anyhow::ensure!(
                        crate::module_lifecycle::active(context, &module),
                        "module is suspended"
                    );
                    anyhow::ensure!(
                        context
                            .modules
                            .lock()
                            .unwrap()
                            .package(&module)
                            .is_some_and(|p| p.revision() == package.revision()),
                        "module revision changed"
                    );
                    let effect = crate::module_host::run_command(
                        context,
                        package,
                        name,
                        args,
                        crate::module_host::Mode {
                            chat: Some(q.message.peer_id().bot_api_dialog_id_unchecked()),
                            message_id: Some(q.message.id()),
                            preview: false,
                        },
                    )
                    .await?;
                    let plan = crate::module_effects::Plan::new(&module, effect)?;
                    plan.require_single_result()?;
                    let output = plan.commit(context, &module, false)?;
                    let user = q
                        .message
                        .sender_id()
                        .map(|id| id.bare_id_unchecked())
                        .unwrap_or(context.owner);
                    let result = crate::module_effects::inline(
                        context,
                        &module,
                        text.split_whitespace().next().unwrap_or(""),
                        output,
                        user,
                    )?;
                    let bot = context
                        .bot
                        .read()
                        .await
                        .clone()
                        .context("inline bot disconnected")?;
                    let grammers_client::tl::enums::Update::BotGuestChatQuery(raw) = &q.raw else {
                        anyhow::bail!("not a guest query")
                    };
                    bot.bot
                        .client
                        .invoke(
                            &grammers_client::tl::functions::messages::SetBotGuestChatResult {
                                query_id: raw.query_id,
                                result,
                            },
                        )
                        .await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
