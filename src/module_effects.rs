//! One validated Effect plan, shared by commands, watchers, loops and callbacks.
use crate::{commands::Context, forms::Screen, modules::Effect};
use anyhow::{Context as _, Result, ensure};
use grammers_client::{
    Client,
    message::{InputMessage, Message},
    update::CallbackQuery,
};
use grammers_session::types::{PeerId, PeerRef};

#[derive(Clone, Debug)]
pub enum Output {
    None,
    Text(String),
    Screen(Box<Screen>),
    Media(crate::media::Media, String),
    Album(Vec<crate::media::Media>, String),
    Frames(Vec<String>, u32),
}
pub struct Plan {
    pub state: Option<serde_json::Value>,
    pub suspend: Option<String>,
    pub output: Output,
}
impl Plan {
    pub fn require_single_result(&self) -> Result<()> {
        ensure!(
            !matches!(self.output, Output::Album(..) | Output::Frames(..)),
            "inline/guest result cannot contain an album or animation; return a screen gallery instead"
        );
        Ok(())
    }
    pub fn new(module: &str, effect: Effect) -> Result<Self> {
        let Effect {
            text,
            state,
            screen,
            media,
            album,
            frames,
            frame_ms,
            suspend,
        } = effect;
        ensure!(
            usize::from(screen.is_some())
                + usize::from(media.is_some())
                + usize::from(!album.is_empty())
                + usize::from(!frames.is_empty())
                <= 1,
            "conflicting module outputs"
        );
        ensure!(
            text.as_ref().is_none_or(|s| s.len() <= 128 * 1024),
            "module output too large"
        );
        if let Some(state) = &state {
            ensure!(
                serde_json::to_vec(state)?.len() <= 256 * 1024,
                "module state too large"
            );
        }
        if let Some(reason) = &suspend {
            ensure!(
                !reason.is_empty() && reason.len() <= 1024,
                "invalid suspend reason"
            );
        }
        let output = if let Some(screen) = screen {
            screen.validate()?;
            screen.restrict_module(module)?;
            Output::Screen(Box::new(screen))
        } else if let Some(media) = media {
            media.validate()?;
            ensure!(
                text.as_ref()
                    .is_none_or(|s| s.encode_utf16().count() <= 1024),
                "media caption exceeds 1024 UTF-16 units"
            );
            Output::Media(media, text.unwrap_or_default())
        } else if !album.is_empty() {
            crate::media::Media::validate_album(&album)?;
            ensure!(
                text.as_ref()
                    .is_none_or(|s| s.encode_utf16().count() <= 1024),
                "album caption exceeds 1024 UTF-16 units"
            );
            Output::Album(album, text.unwrap_or_default())
        } else if !frames.is_empty() {
            ensure!(
                frames.len() <= 100 && frames.iter().all(|f| f.encode_utf16().count() <= 4096),
                "animation exceeds Telegram limits"
            );
            ensure!(
                frame_ms == 0 || (100..=10_000).contains(&frame_ms),
                "invalid frame delay"
            );
            Output::Frames(frames, if frame_ms == 0 { 1000 } else { frame_ms })
        } else if let Some(text) = text.filter(|s| !s.is_empty()) {
            Output::Text(text)
        } else {
            Output::None
        };
        Ok(Self {
            state,
            suspend,
            output,
        })
    }
    pub fn require_callback_destination(&self, has_origin: bool) -> Result<()> {
        ensure!(
            has_origin || !matches!(self.output, Output::Album(..)),
            "album callback needs an originating chat"
        );
        Ok(())
    }
    pub fn commit(self, context: &Context, module: &str, preview: bool) -> Result<Output> {
        if !preview && (self.state.is_some() || self.suspend.is_some()) {
            context.db.transaction(|db| {
                if let Some(state) = self.state {
                    db.entry(module).or_insert_with(|| serde_json::json!({})).as_object_mut().context("invalid module namespace")?.insert("state".into(),state);
                }
                if let Some(reason) = &self.suspend {
                    db.entry("RustModuleLifecycle").or_insert_with(|| serde_json::json!({})).as_object_mut().context("invalid lifecycle namespace")?.insert(module.into(),serde_json::json!({"status":"suspended","reason":clean(context,reason.clone())}));
                }
                Ok(())
            })?;
            if self.suspend.is_some() {
                context.forms.lock().unwrap().invalidate_module(module);
            }
        }
        Ok(self.output)
    }
}
fn clean(context: &Context, text: String) -> String {
    context
        .logs
        .redact(crate::output::censor(text, &context.secrets))
}
fn sanitize(context: &Context, mut screen: Screen) -> Screen {
    screen.text = clean(context, screen.text);
    for text in &mut screen.pages {
        *text = clean(context, text.clone());
    }
    screen.rich_html = screen.rich_html.map(|s| clean(context, s));
    screen
}
/// A background form is sent by the account's bot, with no fabricated command gate.
/// It is owner-only, including forms produced by watchers of untrusted senders.
pub async fn background(
    context: &Context,
    module: &str,
    output: Output,
    bot: bool,
    chat: i64,
    reply: Option<i32>,
) -> Result<()> {
    if matches!(output, Output::None) {
        return Ok(());
    }
    let handle = context.bot.read().await.clone();
    let client = if bot {
        handle
            .as_ref()
            .context("inline bot disconnected")?
            .bot
            .client
            .clone()
    } else {
        context.client.clone()
    };
    let id = PeerId::from_bot_api_dialog_id(chat).context("invalid effect destination")?;
    let peer = if chat == context.owner && !bot {
        PeerId::self_user().to_ambient_ref()
    } else {
        client
            .resolve_peer(id.to_ambient_ref())
            .await?
            .to_ref()
            .await
            .map_err(anyhow::Error::from_boxed)?
            .context("effect peer missing")?
    };
    deliver(context, module, output, &client, peer, reply).await
}
pub async fn message(
    context: &Context,
    module: &str,
    output: Output,
    message: &Message,
) -> Result<Option<String>> {
    match output {
        Output::None => Ok(None),
        Output::Text(text) => Ok(Some(text)),
        Output::Screen(screen) => {
            let screen = sanitize(context, *screen);
            if crate::module_lifecycle::active(context, module)
                && let Some(bot) = context.bot.read().await.clone()
                && bot.form(context, message, screen.clone()).await.is_ok()
            {
                return Ok(None);
            }
            // Preserve pages/media when the inline UI is unavailable.
            let peer = crate::telegram::peer(message).await?;
            deliver(
                context,
                module,
                Output::Screen(Box::new(screen)),
                &context.client,
                peer,
                Some(message.id()),
            )
            .await?;
            Ok(None)
        }
        output => {
            let peer = crate::telegram::peer(message).await?;
            deliver(
                context,
                module,
                output,
                &context.client,
                peer,
                Some(message.id()),
            )
            .await?;
            Ok(None)
        }
    }
}
async fn deliver(
    context: &Context,
    module: &str,
    output: Output,
    client: &Client,
    peer: PeerRef,
    reply: Option<i32>,
) -> Result<()> {
    match output {
        Output::None => {}
        Output::Text(text) => {
            let text = clean(context, text);
            if crate::output::pages(&text).len() > 50 {
                crate::telegram::send_file(
                    client,
                    peer,
                    "module_output.txt",
                    text.into_bytes(),
                    "Module output",
                )
                .await?;
            } else {
                for page in crate::output::pages(&text) {
                    client
                        .send_message(peer, InputMessage::new().text(page).reply_to(reply))
                        .await?;
                }
            }
        }
        Output::Frames(frames, delay) => {
            let first = client
                .send_message(
                    peer,
                    InputMessage::new()
                        .text(clean(context, frames[0].clone()))
                        .reply_to(reply),
                )
                .await?;
            for frame in frames.into_iter().skip(1) {
                tokio::select! { _=context.stop.cancelled()=>break, _=tokio::time::sleep(std::time::Duration::from_millis(delay.into()))=>{} }
                first.edit(clean(context, frame)).await?;
            }
        }
        Output::Media(media, text) => {
            client
                .send_message(
                    peer,
                    InputMessage::new()
                        .text(clean(context, text))
                        .reply_to(reply)
                        .media(media.prepare(client).await?),
                )
                .await?;
        }
        Output::Album(album, text) => {
            let mut items = vec![];
            for (index, media) in album.iter().enumerate() {
                items.push(
                    grammers_client::media::InputMedia::new()
                        .caption(if index == 0 {
                            clean(context, text.clone())
                        } else {
                            String::new()
                        })
                        .reply_to(reply)
                        .media(media.prepare(client).await?),
                );
            }
            client.send_album(peer, items).await?;
        }
        Output::Screen(screen) => {
            let screen = sanitize(context, *screen);
            screen.restrict_module(module)?;
            if crate::module_lifecycle::active(context, module)
                && let Some(bot) = context.bot.read().await.clone()
            {
                // Resolve using the bot's own peer cache; user access hashes are not bot access hashes.
                let bot_peer = if peer.id == PeerId::self_user() {
                    PeerId::from_bot_api_dialog_id(context.owner).context("invalid owner ID")?
                } else {
                    peer.id
                };
                let resolved = async {
                    bot.bot
                        .client
                        .resolve_peer(bot_peer.to_ambient_ref())
                        .await?
                        .to_ref()
                        .await
                        .map_err(anyhow::Error::from_boxed)?
                        .context("bot peer missing")
                }
                .await;
                if let Ok(bot_peer) = resolved {
                    // Saved Messages and the bot's PM with its owner are different conversations.
                    let reply = if peer.id == PeerId::self_user()
                        || peer.id.kind() == grammers_session::types::PeerKind::User
                            && peer.id != bot_peer.id
                    {
                        None
                    } else {
                        reply
                    };
                    let (unit, _) = context.forms.lock().unwrap().create(
                        screen.clone(),
                        context.owner,
                        vec![],
                    )?;
                    context.forms.lock().unwrap().bind_module(&unit, module)?;
                    let sent = async {
                        let body = context.forms.lock().unwrap().render(&unit, 0)?;
                        let media = context.forms.lock().unwrap().media(&unit);
                        let rich = context.forms.lock().unwrap().rich_source(&unit);
                        if let Some(media) = media {
                            bot.bot
                                .send_media(bot_peer, &media, body.reply_to(reply))
                                .await?;
                        } else if let Some(source) = rich {
                            let rich = grammers_client::message::InputRichMessage::html(source);
                            if bot
                                .bot
                                .send_message(
                                    bot_peer,
                                    body.text("")
                                        .fmt_entities([])
                                        .rich_message(rich)
                                        .reply_to(reply),
                                )
                                .await
                                .is_err()
                            {
                                context.forms.lock().unwrap().plain_fallback(&unit);
                                let body = context.forms.lock().unwrap().render(&unit, 0)?;
                                bot.bot.send_message(bot_peer, body.reply_to(reply)).await?;
                            }
                        } else {
                            bot.bot.send_message(bot_peer, body.reply_to(reply)).await?;
                        }
                        Ok::<_, anyhow::Error>(())
                    }
                    .await;
                    if sent.is_ok() {
                        return Ok(());
                    }
                    context.forms.lock().unwrap().remove(&unit);
                }
            }
            let pages = if screen.pages.is_empty() {
                crate::output::pages(&screen.text)
            } else {
                screen.pages.clone()
            };
            if let Some(media) = screen.media.as_ref() {
                client
                    .send_message(
                        peer,
                        InputMessage::new()
                            .text(screen.text.clone())
                            .reply_to(reply)
                            .media(media.prepare(client).await?),
                    )
                    .await?;
            } else if !screen.gallery.is_empty() {
                for media in &screen.gallery {
                    client
                        .send_message(
                            peer,
                            InputMessage::new()
                                .text(screen.text.clone())
                                .reply_to(reply)
                                .media(media.prepare(client).await?),
                        )
                        .await?;
                }
            } else {
                for page in pages {
                    client
                        .send_message(peer, InputMessage::new().text(page).reply_to(reply))
                        .await?;
                }
            }
        }
    }
    Ok(())
}
/// Callback output edits the existing unit. Albums cannot be an inline edit;
/// deliver them as a new message to the known originating chat instead.
pub async fn callback(
    context: &Context,
    module: &str,
    output: Output,
    query: &CallbackQuery,
    unit: &str,
) -> Result<()> {
    if !crate::module_lifecycle::active(context, module) {
        query.answer().alert("Module suspended").send().await?;
        return Ok(());
    }
    let bot = context
        .bot
        .read()
        .await
        .clone()
        .context("inline bot disconnected")?;
    let output = match output {
        Output::None => {
            query.answer().send().await?;
            return Ok(());
        }
        Output::Frames(frames, delay) => {
            for (index, frame) in frames.into_iter().enumerate() {
                if index > 0 {
                    tokio::select! { _=context.stop.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_millis(delay.into()))=>{} }
                }
                let body = context.forms.lock().unwrap().replace(
                    unit,
                    Screen {
                        text: clean(context, frame),
                        ..Default::default()
                    },
                )?;
                bot.bot.edit_callback(query, body).await?;
            }
            query.answer().send().await?;
            return Ok(());
        }
        Output::Album(album, text) => {
            let gate = context
                .forms
                .lock()
                .unwrap()
                .gate(unit)
                .context("album callback needs an originating chat")?;
            background(
                context,
                module,
                Output::Album(album, text),
                false,
                gate.message.peer_id().bot_api_dialog_id_unchecked(),
                Some(gate.message.id()),
            )
            .await?;
            query.answer().send().await?;
            return Ok(());
        }
        Output::Media(media, text) => Screen {
            text,
            media: Some(media),
            ..Default::default()
        },
        Output::Text(text) => Screen {
            text,
            ..Default::default()
        },
        Output::Screen(screen) => *screen,
    };
    let body = context
        .forms
        .lock()
        .unwrap()
        .replace(unit, sanitize(context, output))?;
    edit_screen(context, query, unit, body).await?;
    query.answer().send().await?;
    Ok(())
}
pub async fn edit_screen(
    context: &Context,
    query: &CallbackQuery,
    unit: &str,
    body: InputMessage,
) -> Result<()> {
    let bot = context
        .bot
        .read()
        .await
        .clone()
        .context("inline bot disconnected")?;
    let media = context.forms.lock().unwrap().media(unit);
    if let Some(media) = media {
        bot.bot.edit_callback_media(query, body, &media).await?;
    } else {
        let rich = context.forms.lock().unwrap().rich_source(unit);
        if rich.is_some() {
            if let Err(error) = bot.bot.edit_callback_rich(query, body.clone(), rich).await {
                if !context.forms.lock().unwrap().plain_fallback(unit) {
                    return Err(error.into());
                }
                let plain = context.forms.lock().unwrap().render(unit, 0)?;
                bot.bot.edit_callback(query, plain).await?;
            }
        } else {
            bot.bot.edit_callback(query, body).await?;
        }
    }
    Ok(())
}
/// Inline/guest transport accepts one result, not an animation or album.
pub fn inline(
    context: &Context,
    module: &str,
    command: &str,
    output: Output,
    user: i64,
) -> Result<grammers_client::tl::enums::InputBotInlineResult> {
    let screen = match output {
        Output::None => Screen {
            text: "\u{ad}".into(),
            ..Default::default()
        },
        Output::Text(text) => Screen {
            text,
            ..Default::default()
        },
        Output::Screen(screen) => *screen,
        Output::Media(media, text) => Screen {
            text,
            media: Some(media),
            ..Default::default()
        },
        Output::Album(..) | Output::Frames(..) => anyhow::bail!(
            "inline/guest result cannot contain an album or animation; return a screen gallery instead"
        ),
    };
    let screen = sanitize(context, screen);
    screen.restrict_module(module)?;
    // Stateless inline answers need no unit; keep the bounded form store for actual UI.
    if screen.buttons.is_empty()
        && screen.rich_buttons.is_empty()
        && screen.pages.is_empty()
        && screen.gallery.is_empty()
    {
        let body = if screen.html {
            InputMessage::new().html(&screen.text)
        } else {
            InputMessage::new().text(&screen.text)
        };
        return crate::media::inline_result(
            module,
            module,
            body,
            screen.media.as_ref(),
            screen.rich_html.as_deref(),
        );
    }
    let (unit, _) = context
        .forms
        .lock()
        .unwrap()
        .create(screen, user, vec![context.owner])?;
    context.forms.lock().unwrap().bind_module(&unit, module)?;
    context
        .forms
        .lock()
        .unwrap()
        .bind_inline_command(&unit, command)?;
    let result = context.forms.lock().unwrap().inline_result(&unit);
    if result.is_err() {
        context.forms.lock().unwrap().remove(&unit);
    }
    result
}
