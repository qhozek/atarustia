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
        let (id, _body) =
            context
                .forms
                .lock()
                .unwrap()
                .create(screen, owner, vec![context.owner])?;
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
                    context
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
                                random_id: rand::random(),
                                query_id: results.query_id,
                                id: result_id,
                                schedule_date: None,
                                send_as: None,
                                quick_reply_shortcut: None,
                                allow_paid_stars: None,
                            },
                        )
                        .await?;
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
        let event = crate::modules::Event {
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
        };
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
        let event = crate::modules::Event {
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
            data: serde_json::to_value(crate::botapi::Message::from_native(message))?,
        };
        if !matches!(
            message.peer(),
            Some(grammers_client::peer::Peer::Channel(_))
        ) && let Err(e) = context.handle_event(event).await
        {
            tracing::warn!(error=%e,"bot watcher failed");
        }
    }
    match update {
        Update::NewMessage(m)
            if !m.outgoing()
                && owners.contains(&m.sender_id().map(|s| s.bare_id_unchecked()).unwrap_or(0)) =>
        {
            if m.text() == "/start" || m.text() == "/help" {
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
                use crate::forms::Action;
                if !owners.contains(&q.sender_id().bare_id_unchecked()) {
                    let gate = context.forms.lock().unwrap().gate(&click.unit);
                    anyhow::ensure!(
                        gate.is_some()
                            || matches!(
                                &click.action,
                                crate::forms::Action::HelpNavigate { .. }
                                    | crate::forms::Action::Help { .. }
                                    | crate::forms::Action::Close
                                    | crate::forms::Action::Page { .. }
                            ) && crate::security_commands::inline_allowed(
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
                        let p = context
                            .modules
                            .lock()
                            .unwrap()
                            .export()
                            .into_iter()
                            .find(|p| p.manifest.name == module)
                            .context("module unloaded")?;
                        let saved = context.db.get(&module, "__config__");
                        let config = if saved.is_null() {
                            p.manifest.config.clone()
                        } else {
                            saved
                        };
                        let state = context.db.get(&module, "state");
                        let event = serde_json::json!({"kind":"callback","user":q.sender_id().bare_id_unchecked(),"args":args});
                        let effect = tokio::task::spawn_blocking(move || {
                            p.run_event(&function, event, config, state)
                        })
                        .await??;
                        if let Some(s) = effect.state {
                            context.db.set(&module, "state", s)?;
                        }
                        let screen = effect.screen.unwrap_or_else(|| crate::forms::Screen {
                            text: effect.text.unwrap_or_default(),
                            ..Default::default()
                        });
                        screen.restrict_module(&module)?;
                        context.forms.lock().unwrap().replace(&click.unit, screen)?
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
            if let Some(id) = q.text().strip_prefix("unit:") {
                if !owners.contains(&q.sender_id().bare_id_unchecked()) {
                    q.answer(Vec::<Article>::new()).send().await?;
                    return Ok(());
                }
                let result = context
                    .forms
                    .lock()
                    .unwrap()
                    .inline_result(id)
                    .map(|r| vec![r])
                    .unwrap_or_default();
                q.answer(result).private().cache_time(0).send().await?;
                return Ok(());
            }
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
                let args = q
                    .text()
                    .split_once(char::is_whitespace)
                    .map(|(_, a)| a)
                    .unwrap_or("")
                    .to_owned();
                let name = cmd.to_owned();
                let module = package.manifest.name.clone();
                let saved = context.db.get(&module, "__config__");
                let config = if saved.is_null() {
                    package.manifest.config.clone()
                } else {
                    saved
                };
                let state = context.db.get(&module, "state");
                let effect = tokio::task::spawn_blocking(move || {
                    package.run_command(&name, &args, config, state)
                })
                .await??;
                // Inline previews must never mutate module state: typing a query is not execution consent.
                if let Some(media) = effect.media.as_ref() {
                    let caption = context.logs.redact(effect.text.clone().unwrap_or_default());
                    let result = crate::media::inline_result(
                        &module,
                        &module,
                        InputMessage::new().text(caption),
                        Some(media),
                        None,
                    )?;
                    q.answer([result]).private().cache_time(0).send().await?;
                    return Ok(());
                }
                if let Some(screen) = effect
                    .screen
                    .as_ref()
                    .filter(|s| s.media.is_some() || !s.gallery.is_empty())
                {
                    let media = screen.media.as_ref().or_else(|| screen.gallery.first());
                    let result = crate::media::inline_result(
                        &module,
                        &module,
                        if screen.html {
                            InputMessage::new().html(&screen.text)
                        } else {
                            InputMessage::new().text(context.logs.redact(screen.text.clone()))
                        },
                        media,
                        None,
                    )?;
                    q.answer([result]).private().cache_time(0).send().await?;
                    return Ok(());
                }
                let text = effect
                    .text
                    .or_else(|| effect.screen.map(|s| s.text))
                    .unwrap_or_default();
                q.answer([Article::new(module, context.logs.redact(text))])
                    .private()
                    .cache_time(0)
                    .send()
                    .await?;
                return Ok(());
            }
            let body = if q.text().trim() == "ping" {
                InputMessage::new().text("Pong 🦀")
            } else {
                screen(context, q.sender_id().bare_id_unchecked(), 0).await?
            };
            q.answer([Article::new("atarustia", body).description("Native Rust userbot")])
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
            if text == "ping" {
                q.answer(Article::new("Pong", "Pong 🦀")).await?;
            } else {
                let (name, args) = text.split_once(char::is_whitespace).unwrap_or((&text, ""));
                let package = context
                    .modules
                    .lock()
                    .unwrap()
                    .command(name)
                    .filter(|(_, c)| c.guest);
                if let Some((package, _)) = package {
                    let module = package.manifest.name.clone();
                    let saved = context.db.get(&module, "__config__");
                    let config = if saved.is_null() {
                        package.manifest.config.clone()
                    } else {
                        saved
                    };
                    let name = name.to_owned();
                    let args = args.to_owned();
                    let result =
                        tokio::task::spawn_blocking(move || package.run(&name, &args, config))
                            .await??;
                    q.answer(Article::new("atarustia", result)).await?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
