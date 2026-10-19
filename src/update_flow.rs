//! Update/restart lifecycle. Only an owner can start a persisted, commit-pinned operation.
use crate::{
    commands::Context,
    database::Database,
    forms::Screen,
    update_ui::Ui,
    updater::{self, Report},
};
use anyhow::{Context as _, Result, ensure};
use grammers_client::{
    message::{InputMessage, Message, ReplyMarkup},
    tl,
    update::CallbackQuery,
};
use grammers_session::types::{PeerId, PeerRef};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Instant};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
    User { peer: PeerRef, id: i32 },
    Bot { peer: PeerRef, id: i32 },
    Inline { id: String },
}
impl Target {
    async fn screen(&self, ctx: &Context, screen: &Screen) -> Result<()> {
        let premium = matches!(self, Self::User { .. })
            && ctx
                .db
                .get("RustRuntime", "premium")
                .as_bool()
                .unwrap_or(false);
        let input = if matches!(self, Self::User { .. }) {
            crate::update_ui::user_body(screen, premium)?
        } else {
            body(screen)
        };
        match self.edit(ctx,input).await {
            Err(error) if premium && error.downcast_ref::<grammers_client::InvocationError>().is_some_and(|e|matches!(e,grammers_client::InvocationError::Rpc(r) if matches!(r.name.as_str(),"PREMIUM_ACCOUNT_REQUIRED"|"CUSTOM_EMOJI_INVALID")))=>self.edit(ctx,crate::update_ui::user_body(screen,false)?).await,
            result=>result,
        }
    }
    pub async fn callback(query: &CallbackQuery) -> Result<Self> {
        Ok(match &query.raw {
            tl::enums::Update::InlineBotCallbackQuery(q) => Self::Inline {
                id: crate::file_id::encode_inline_id(&q.msg_id),
            },
            tl::enums::Update::BotCallbackQuery(q) => Self::Bot {
                peer: query
                    .peer_ref()
                    .await
                    .map_err(anyhow::Error::from_boxed)?
                    .context("callback peer missing")?,
                id: q.msg_id,
            },
            _ => anyhow::bail!("not a callback"),
        })
    }
    async fn message(_ctx: &Context, message: &Message) -> Result<Self> {
        let peer = crate::telegram::peer(message).await?;
        if message.outgoing() {
            Ok(Self::User {
                peer,
                id: message.id(),
            })
        } else {
            let reply = message.reply("⏳").await?;
            Ok(Self::User {
                peer,
                id: reply.id(),
            })
        }
    }
    async fn edit(&self, ctx: &Context, input: InputMessage) -> Result<()> {
        match self {
            Self::User { peer, id } => ctx.client.edit_message(*peer, *id, input).await?,
            Self::Bot { peer, id } => {
                let bot = ctx.bot.read().await.clone().context("bot disconnected")?;
                bot.bot
                    .native()
                    .edit_message_text(*peer, *id, input)
                    .await?;
            }
            Self::Inline { id } => {
                let bot = ctx.bot.read().await.clone().context("bot disconnected")?;
                bot.bot
                    .native()
                    .edit_inline_message(id, input, None, None)
                    .await?;
            }
        }
        Ok(())
    }
}
pub struct Surface {
    pub target: Target,
    pub unit: Option<String>,
}
impl Surface {
    pub async fn show(&self, ctx: &Context, screen: Screen) -> Result<()> {
        if let Some(unit) = &self.unit {
            let input = ctx.forms.lock().unwrap().replace(unit, screen)?;
            self.target.edit(ctx, input).await
        } else {
            self.target.screen(ctx, &screen).await
        }
    }
}
pub fn body(screen: &Screen) -> InputMessage {
    let input = if screen.html {
        InputMessage::new().html(&screen.text)
    } else {
        InputMessage::new().text(&screen.text)
    };
    input
        .link_preview(false)
        .reply_markup(ReplyMarkup::from_buttons(&[]))
}
fn modules(ctx: &Context) -> Vec<String> {
    let mut names = vec!["core".into()];
    names.extend(
        ctx.modules
            .lock()
            .unwrap()
            .list()
            .into_iter()
            .map(|m| m.name),
    );
    names.extend(ctx.native.list().into_iter().map(|m| m.name().to_owned()));
    names.sort();
    names.dedup();
    names
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub target: Target,
    pub update: bool,
    pub started: u64,
    pub modules: Vec<String>,
    pub from: Option<String>,
    pub expected: Option<String>,
    #[serde(default)]
    pub changes: Option<updater::Changes>,
}
pub fn remember(db: &Database, pending: &Pending) -> Result<()> {
    ensure!(
        pending.started > 0 && pending.modules.len() <= 1024,
        "invalid restart state"
    );
    if pending.update {
        ensure!(
            pending.expected.as_deref().is_some_and(updater::valid_hash)
                && pending.from.as_deref().is_some_and(updater::valid_hash),
            "invalid pending update identity"
        );
    }
    match &pending.target {
        Target::User { id, .. } | Target::Bot { id, .. } => ensure!(*id > 0, "invalid message ID"),
        Target::Inline { id } => {
            crate::file_id::decode_inline_id(id)?;
        }
    }
    db.transaction(|db| {
        let updater = db
            .entry("RustUpdater")
            .or_insert_with(|| serde_json::json!({}));
        updater["pending"] = serde_json::to_value(pending)?;
        db.entry("RustRuntime")
            .or_insert_with(|| serde_json::json!({}))["restart"] = true.into();
        Ok(())
    })
}
pub fn pending(db: &Database) -> Result<Option<Pending>> {
    let value = db.get("RustUpdater", "pending");
    Ok(if value.is_null() {
        None
    } else {
        Some(serde_json::from_value(value)?)
    })
}
async fn best_effort(surface: &Surface, ctx: &Context, screen: Screen) {
    if surface.show(ctx, screen).await.is_err() {
        tracing::warn!("could not edit updater progress; operation state is unchanged");
    }
}
pub async fn command(ctx: &Context, message: &Message, name: &str, args: &str) -> Result<()> {
    ensure!(
        message.sender_id().is_some_and(
            |id| crate::security::owners(&ctx.db, ctx.owner).contains(&id.bare_id_unchecked())
        ),
        "owner only updater"
    );
    let ui = Ui::new(&ctx.db);
    let force = args == "-f";
    if name == "restart" {
        ensure!(args.is_empty() || force, "usage: restart [-f]");
        if !force && let Some(bot) = ctx.bot.read().await.clone() {
            let screen = ui.restart();
            if bot.form(ctx, message, screen.clone()).await.is_ok() {
                return Ok(());
            }
            let surface = Surface {
                target: Target::message(ctx, message).await?,
                unit: None,
            };
            let mut screen = screen;
            screen.text += &format!(
                "\n\n<code>{}restart -f</code>",
                crate::output::escape(&ctx.db.prefix())
            );
            surface.show(ctx, screen).await?;
            return Ok(());
        }
        let surface = Surface {
            target: Target::message(ctx, message).await?,
            unit: None,
        };
        return restart(ctx, &surface).await;
    }
    ensure!(
        args.is_empty() || force || updater::valid_hash(args),
        "usage: update [-f|full_commit_hash]"
    );
    let report = {
        let _lock = updater::MAINTENANCE
            .try_lock()
            .context("another update/restart is running")?;
        updater::check(root()).await?
    };
    if report.current == report.incoming {
        if !args.is_empty() && !force {
            ensure!(
                args == report.incoming,
                "upstream changed; review .update again"
            );
        }
        if let Some(retry) = retry_report(ctx)?
            && retry.incoming == report.current
        {
            let surface = Surface {
                target: Target::message(ctx, message).await?,
                unit: None,
            };
            return update(ctx, &surface, retry, true).await;
        }
        let surface = Surface {
            target: Target::message(ctx, message).await?,
            unit: None,
        };
        surface.show(ctx, ui.current(&report)).await?;
        return Ok(());
    }
    if !args.is_empty() && !force {
        ensure!(
            args == report.incoming,
            "upstream changed; review .update again"
        );
    }
    if args.is_empty()
        && let Some(bot) = ctx.bot.read().await.clone()
    {
        if bot
            .form(ctx, message, ui.card(&report, false))
            .await
            .is_ok()
        {
            return Ok(());
        }
        // An inline transport failure must not count as the owner pressing Update.
        let mut screen = ui.card(&report, false);
        screen.text += &format!(
            "\n\n<code>{}update {}</code>",
            crate::output::escape(&ctx.db.prefix()),
            report.incoming
        );
        let surface = Surface {
            target: Target::message(ctx, message).await?,
            unit: None,
        };
        surface.show(ctx, screen).await?;
        return Ok(());
    }
    let surface = Surface {
        target: Target::message(ctx, message).await?,
        unit: None,
    };
    update(ctx, &surface, report, false).await
}
pub fn retry_report(ctx: &Context) -> Result<Option<Report>> {
    let value = ctx.db.get("RustUpdater", "retry");
    Ok(if value.is_null() {
        None
    } else {
        Some(serde_json::from_value(value)?)
    })
}
pub async fn callback(
    ctx: &Context,
    query: &CallbackQuery,
    unit: String,
    action: crate::forms::Action,
) -> Result<()> {
    ensure!(
        crate::security::owners(&ctx.db, ctx.owner)
            .contains(&query.sender_id().bare_id_unchecked()),
        "owner only updater"
    );
    query.answer().send().await?;
    let surface = Surface {
        target: Target::callback(query).await?,
        unit: Some(unit.clone()),
    };
    match action {
        crate::forms::Action::Update {
            current,
            incoming,
            retry,
        } => {
            ensure!(
                updater::valid_hash(&current) && updater::valid_hash(&incoming),
                "invalid reviewed commit"
            );
            let report = if retry {
                let report = retry_report(ctx)?.context("no dependency retry pending")?;
                ensure!(
                    report.current == current && report.incoming == incoming,
                    "retry no longer matches this update"
                );
                report
            } else {
                let result = updater::check(root()).await;
                match result {
                    Ok(report) if report.current == current && report.incoming == incoming => {
                        report
                    }
                    other => {
                        let error = match other {
                            Ok(_) => "Upstream changed; review .update again".to_owned(),
                            Err(error) => ctx.logs.redact(crate::output::error_message(
                                &error.to_string(),
                                &ctx.secrets,
                            )),
                        };
                        best_effort(&surface, ctx, Ui::new(&ctx.db).failed(&error, None)).await;
                        return Ok(());
                    }
                }
            };
            update(ctx, &surface, report, retry).await
        }
        crate::forms::Action::Restart => restart(ctx, &surface).await,
        crate::forms::Action::UpdateDismiss => {
            // Invalidate the entire form before deleting or editing its message.
            ctx.forms.lock().unwrap().remove(&unit);
            match &surface.target {
                Target::Bot { peer, id } => {
                    if let Some(bot) = ctx.bot.read().await.clone() {
                        bot.bot.native().delete_message(*peer, *id).await?;
                    }
                }
                _ => {
                    surface
                        .target
                        .edit(
                            ctx,
                            InputMessage::new()
                                .text("✕")
                                .reply_markup(ReplyMarkup::from_buttons(&[])),
                        )
                        .await?
                }
            }
            Ok(())
        }
        _ => anyhow::bail!("not an updater action"),
    }
}
pub async fn update(ctx: &Context, surface: &Surface, report: Report, retry: bool) -> Result<()> {
    let _lock = updater::MAINTENANCE
        .try_lock()
        .context("another update/restart is running")?;
    let _account = ctx
        .maintenance
        .try_lock()
        .context("account maintenance is running")?;
    ensure!(!ctx.stop.is_cancelled(), "shutdown already requested");
    let ui = Ui::new(&ctx.db);
    let started = crate::security::now();
    let timer = Instant::now();
    best_effort(surface, ctx, ui.progress(&[], 0)).await;
    let apply = if retry {
        updater::verify_retry(root(), &report.incoming).await
    } else {
        updater::apply_reviewed(root(), &report.current, &report.incoming).await
    };
    if let Err(error) = apply {
        best_effort(
            surface,
            ctx,
            ui.failed(
                &ctx.logs.redact(crate::output::error_message(
                    &error.to_string(),
                    &ctx.secrets,
                )),
                None,
            ),
        )
        .await;
        return Ok(());
    }
    ctx.db
        .set("RustUpdater", "retry", serde_json::to_value(&report)?)?;
    let mut done = vec![(0, timer.elapsed().as_millis() as u64)];
    best_effort(surface, ctx, ui.progress(&done, 1)).await;
    let syncing = Instant::now();
    let result =
        tokio::select! {_=ctx.stop.cancelled()=>return Ok(()),result=updater::sync(root())=>result};
    if let Err(error) = result {
        best_effort(
            surface,
            ctx,
            ui.failed(
                &ctx.logs.redact(crate::output::error_message(
                    &error.to_string(),
                    &ctx.secrets,
                )),
                Some((&report.current, &report.incoming)),
            ),
        )
        .await;
        return Ok(());
    }
    // A terminal git command may have modified the checkout while Cargo was running.
    if let Err(error) = updater::verify_retry(root(), &report.incoming).await {
        best_effort(
            surface,
            ctx,
            ui.failed(
                &ctx.logs.redact(crate::output::error_message(
                    &error.to_string(),
                    &ctx.secrets,
                )),
                None,
            ),
        )
        .await;
        return Ok(());
    }
    done.push((1, syncing.elapsed().as_millis() as u64));
    best_effort(surface, ctx, ui.progress(&done, 2)).await;
    remember(
        &ctx.db,
        &Pending {
            target: surface.target.clone(),
            update: true,
            started,
            modules: modules(ctx),
            from: Some(report.current),
            expected: Some(report.incoming),
            changes: Some(report.changes),
        },
    )?;
    ctx.stop.cancel();
    Ok(())
}
pub async fn restart(ctx: &Context, surface: &Surface) -> Result<()> {
    let _lock = updater::MAINTENANCE
        .try_lock()
        .context("another update/restart is running")?;
    let _account = ctx
        .maintenance
        .try_lock()
        .context("account maintenance is running")?;
    ensure!(!ctx.stop.is_cancelled(), "shutdown already requested");
    best_effort(surface, ctx, Ui::new(&ctx.db).restarting()).await;
    remember(
        &ctx.db,
        &Pending {
            target: surface.target.clone(),
            update: false,
            started: crate::security::now(),
            modules: modules(ctx),
            from: None,
            expected: None,
            changes: None,
        },
    )?;
    ctx.stop.cancel();
    Ok(())
}
pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Called only after native ready hooks and session setup, not when the process merely starts.
pub async fn complete(ctx: &Context) -> Result<()> {
    complete_inner(ctx, false).await
}
async fn complete_inner(ctx: &Context, fallback: bool) -> Result<()> {
    let _account = ctx.maintenance.lock().await;
    let Some(pending) = pending(&ctx.db)? else {
        return Ok(());
    };
    let bot_missing =
        !matches!(pending.target, Target::User { .. }) && ctx.bot.read().await.is_none();
    if bot_missing && !fallback {
        return Ok(());
    }
    let loaded = modules(ctx);
    let mut failed = pending
        .modules
        .iter()
        .filter(|name| !loaded.contains(name))
        .cloned()
        .collect::<Vec<_>>();
    let commit = updater::head(root()).await?;
    if pending
        .expected
        .as_ref()
        .is_some_and(|expected| expected != &commit)
    {
        failed.push("source revision differs from the requested update".into());
    }
    let screen = Ui::new(&ctx.db).completed(
        pending.update,
        &commit,
        crate::security::now().saturating_sub(pending.started) * 1000,
        loaded.len(),
        &failed,
        pending.changes.as_ref(),
    );
    let result = if bot_missing {
        Err(anyhow::anyhow!("inline bot did not become ready"))
    } else {
        pending.target.screen(ctx, &screen).await
    };
    match result {
        Ok(()) => {}
        Err(error) => {
            tracing::warn!(error=%crate::output::error_message(&error.to_string(),&ctx.secrets),"could not edit restart report; sending it to Saved Messages");
            ctx.client
                .send_message(PeerId::self_user().to_ambient_ref(), body(&screen))
                .await?;
        }
    }
    ctx.db.transaction(|db| {
        if let Some(state) = db.get_mut("RustUpdater") {
            state["pending"] = serde_json::Value::Null;
            if pending.update {
                state["retry"] = serde_json::Value::Null;
            }
        }
        Ok(())
    })
}
pub async fn complete_for(ctx: &Context, bot: bool) -> Result<()> {
    if let Some(pending) = pending(&ctx.db)?
        && matches!(pending.target, Target::User { .. }) != bot
    {
        complete(ctx).await?;
    }
    Ok(())
}
pub async fn ready_fallback(ctx: &Context) -> Result<()> {
    complete_inner(ctx, true).await
}

pub async fn notify(ctx: &Context) -> Result<()> {
    if !crate::update_ui::Config::load(&ctx.db)?.update_notifications || ctx.stop.is_cancelled() {
        return Ok(());
    }
    let Some(bot) = ctx.bot.read().await.clone() else {
        return Ok(());
    };
    let Ok(_lock) = updater::MAINTENANCE.try_lock() else {
        return Ok(());
    };
    let report = updater::check(root()).await?;
    if report.current == report.incoming
        || ctx.db.get("RustUpdater", "notified_commit").as_str() == Some(&report.incoming)
    {
        return Ok(());
    }
    let screen = Ui::new(&ctx.db).card(&report, true);
    let (unit, input) = ctx
        .forms
        .lock()
        .unwrap()
        .create(screen, ctx.owner, vec![])?;
    let peer = PeerId::user(ctx.owner)
        .context("invalid owner")?
        .to_ambient_ref();
    if let Err(error) = bot
        .bot
        .native()
        .send_message(peer, input.link_preview(false))
        .await
    {
        ctx.forms.lock().unwrap().remove(&unit);
        return Err(error);
    }
    ctx.db
        .set("RustUpdater", "notified_commit", report.incoming)
}
