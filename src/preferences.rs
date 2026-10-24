use crate::{commands::Context, database::Database};
use anyhow::{Context as _, Result, ensure};
use grammers_client::message::Message;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const COMMANDS: &[(&str, &str)] = &[
    ("helphide", "<module...> — скрыть/показать в справке"),
    ("nonickcmd", "<command> — переключить NoNick"),
    ("nonickcmds", "команды NoNick"),
    ("nonickuser", "[user] — переключить NoNick"),
    ("nonickusers", "пользователи NoNick"),
    ("nonickchat", "NoNick в текущем чате"),
    ("nonickchats", "чаты NoNick"),
    ("watchers", "список watchers и фильтров"),
    ("watcher", "<module> [-c|-p] [-o|-i] — настройки watcher"),
    ("watcherbl", "<module> — блокировка watcher в чате"),
    (
        "cleardb",
        "confirm — очистить настройки с предварительной копией",
    ),
    ("clearmodule", "<module> confirm — очистить данные модуля"),
];
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WatcherPolicy {
    pub disabled: bool,
    pub chats_only: bool,
    pub private_only: bool,
    pub outgoing_only: bool,
    pub incoming_only: bool,
    pub blocked_chats: Vec<i64>,
}
impl WatcherPolicy {
    pub fn accepts(&self, chat: i64, outgoing: bool) -> bool {
        !self.disabled
            && !self.blocked_chats.contains(&chat)
            && (!self.chats_only || chat < 0)
            && (!self.private_only || chat > 0)
            && (!self.outgoing_only || outgoing)
            && (!self.incoming_only || !outgoing)
    }
}
pub fn watcher_policy(db: &Database, module: &str) -> WatcherPolicy {
    serde_json::from_value(
        db.get("atarustia.main", "watcher_policies")
            .get(module)
            .cloned()
            .unwrap_or(Value::Null),
    )
    .unwrap_or_default()
}
pub fn hidden(db: &Database, module: &str) -> bool {
    db.get("atarustia.help", "hidden")
        .as_array()
        .is_some_and(|a| {
            a.iter().any(|v| {
                v.as_str()
                    .is_some_and(|name| crate::original::canonical_module(name) == module)
            })
        })
}
#[allow(clippy::too_many_arguments)] // Pure predicate; every authorization input is explicit for tests.
pub fn addressed(
    db: &Database,
    user: i64,
    chat: i64,
    command: &str,
    text: &str,
    username: Option<&str>,
    outgoing: bool,
    targeted: bool,
) -> bool {
    outgoing
        || chat > 0
        || targeted
        || db
            .get("atarustia.main", "no_nickname")
            .as_bool()
            .unwrap_or(false)
        || db
            .get("atarustia.main", "nonickcmds")
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(command)))
        || db
            .get("atarustia.main", "nonickusers")
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_i64() == Some(user)))
        || db
            .get("atarustia.main", "nonickchats")
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_i64() == Some(chat)))
        || username.is_some_and(|u| {
            text.split('@').skip(1).any(|part| {
                part.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(u))
            })
        })
}
fn toggle(db: &Database, namespace: &str, key: &str, value: Value) -> Result<bool> {
    db.transaction(|data| {
        let ns = data
            .entry(namespace)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .context("invalid namespace")?;
        let array = ns
            .entry(key)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("invalid list")?;
        if array.contains(&value) {
            array.retain(|v| v != &value);
            Ok(false)
        } else {
            array.push(value);
            Ok(true)
        }
    })
}
pub async fn execute(ctx: &Context, m: &Message, cmd: &str, args: &str) -> Result<Option<String>> {
    let text = match cmd {
        "helphide" => {
            ensure!(!args.is_empty(), "usage: helphide <module...>");
            let modules = crate::help::catalog(ctx).modules;
            let (mut hidden, mut shown) = (vec![], vec![]);
            for name in args.split_whitespace() {
                ensure!(
                    modules.iter().any(|m| m.name.eq_ignore_ascii_case(name)),
                    "unknown module"
                );
                let name = &modules
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(name))
                    .unwrap()
                    .name;
                if crate::help::toggle_hidden(ctx, name)? {
                    hidden.push(name.clone());
                } else {
                    shown.push(name.clone());
                }
            }
            let text = ctx.original(
                "ataraxia_info",
                "hidden_shown",
                &[
                    hidden.len().to_string(),
                    shown.len().to_string(),
                    hidden
                        .iter()
                        .map(|m| format!("👁‍🗨 <i>{}</i>", crate::output::escape(m)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    shown
                        .iter()
                        .map(|m| format!("👁 <i>{}</i>", crate::output::escape(m)))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ],
            );
            ctx.answer_html(m, &text).await?;
            return Ok(None);
        }
        "nonickcmd" => {
            ensure!(
                crate::commands::is_builtin(args)
                    || ctx.modules.lock().unwrap().command(args).is_some(),
                "unknown command"
            );
            let on = toggle(&ctx.db, "atarustia.main", "nonickcmds", json!(args))?;
            ctx.answer_html(
                m,
                &ctx.original(
                    "settings",
                    "cmd_nn",
                    &[
                        crate::output::escape(&format!("{}{args}", ctx.db.prefix())),
                        if on { "on" } else { "off" }.into(),
                    ],
                ),
            )
            .await?;
            return Ok(None);
        }
        "nonickuser" => {
            let id = crate::security_commands::resolve_user(
                ctx,
                m,
                if args.is_empty() { None } else { Some(args) },
            )
            .await?;
            let on = toggle(&ctx.db, "atarustia.main", "nonickusers", json!(id))?;
            ctx.answer_html(
                m,
                &ctx.original(
                    "settings",
                    "user_nn",
                    &[if on { "on" } else { "off" }.into()],
                ),
            )
            .await?;
            return Ok(None);
        }
        "nonickchat" => {
            if m.peer_id().bot_api_dialog_id_unchecked() > 0 {
                ctx.answer_html(m, &ctx.original("settings", "private_not_allowed", &[]))
                    .await?;
                return Ok(None);
            }
            let on = toggle(
                &ctx.db,
                "atarustia.main",
                "nonickchats",
                json!(m.peer_id().bot_api_dialog_id_unchecked()),
            )?;
            let name = m
                .peer()
                .and_then(|p| p.name().map(str::to_owned))
                .unwrap_or_default();
            ctx.answer_html(
                m,
                &ctx.original(
                    "settings",
                    "cmd_nn",
                    &[
                        crate::output::escape(&name),
                        if on { "on" } else { "off" }.into(),
                    ],
                ),
            )
            .await?;
            return Ok(None);
        }
        "nonickcmds" | "nonickusers" | "nonickchats" => {
            let ids = ctx.db.get("atarustia.main", cmd);
            let ids = ids.as_array().cloned().unwrap_or_default();
            let mut lines = vec![];
            for value in ids {
                if cmd == "nonickcmds" {
                    if let Some(name) = value.as_str() {
                        lines.push(format!(
                            "▫️ <code>{}</code>",
                            crate::output::escape(&format!("{}{name}", ctx.db.prefix()))
                        ));
                    }
                    continue;
                }
                if let Some(id) = value.as_i64()
                    && let Some(peer) = grammers_session::types::PeerId::from_bot_api_dialog_id(id)
                    && let Ok(entity) = ctx.client.resolve_peer(peer.to_ambient_ref()).await
                {
                    let name = crate::output::escape(entity.name().unwrap_or(""));
                    let url = if cmd == "nonickusers" {
                        format!("tg://user?id={id}")
                    } else {
                        format!("https://t.me/c/{}", peer.bare_id_unchecked())
                    };
                    lines.push(format!("▫️ <b><a href=\"{url}\">{name}</a></b>"));
                }
            }
            let text = if lines.is_empty() {
                ctx.original("settings", "nothing", &[])
            } else {
                ctx.original(
                    "settings",
                    match cmd {
                        "nonickcmds" => "cmd_nn_list",
                        "nonickusers" => "user_nn_list",
                        _ => "chat_nn_list",
                    },
                    &[lines.join("\n")],
                )
            };
            ctx.answer_html(m, &text).await?;
            return Ok(None);
        }
        "watchers" => {
            let list = ctx.modules.lock().unwrap().list();
            let lines = list
                .into_iter()
                .filter(|m| !m.watchers.is_empty())
                .map(|m| {
                    let p = watcher_policy(&ctx.db, &m.name);
                    format!(
                        "{} {}",
                        if p.disabled { "💢" } else { "♻️" },
                        crate::output::escape(&m.name)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            ctx.answer_html(m, &ctx.original("settings", "watchers", &[lines]))
                .await?;
            return Ok(None);
        }
        "watcher" | "watcherbl" => {
            let mut p = args.split_whitespace();
            let name = p.next().context("module missing")?;
            ensure!(
                ctx.modules
                    .lock()
                    .unwrap()
                    .list()
                    .iter()
                    .any(|m| m.name == name && !m.watchers.is_empty()),
                "module has no watchers"
            );
            let mut policy = watcher_policy(&ctx.db, name);
            let flags: Vec<_> = p.collect();
            if cmd == "watcherbl" {
                let id = m.peer_id().bot_api_dialog_id_unchecked();
                if policy.blocked_chats.contains(&id) {
                    policy.blocked_chats.retain(|c| *c != id);
                } else {
                    policy.blocked_chats.push(id);
                }
            } else if flags.is_empty() {
                policy.disabled = !policy.disabled;
            } else {
                ensure!(
                    flags
                        .iter()
                        .all(|f| matches!(*f, "-c" | "-p" | "-o" | "-i")),
                    "unknown watcher flag"
                );
                ensure!(
                    !(flags.contains(&"-c") && flags.contains(&"-p"))
                        && !(flags.contains(&"-o") && flags.contains(&"-i")),
                    "conflicting filters"
                );
                policy.disabled = false;
                policy.chats_only = flags.contains(&"-c");
                policy.private_only = flags.contains(&"-p");
                policy.outgoing_only = flags.contains(&"-o");
                policy.incoming_only = flags.contains(&"-i");
            }
            let mut all = ctx
                .db
                .get("atarustia.main", "watcher_policies")
                .as_object()
                .cloned()
                .unwrap_or_default();
            all.insert(name.into(), serde_json::to_value(&policy)?);
            ctx.db
                .set("atarustia.main", "watcher_policies", Value::Object(all))?;
            serde_json::to_string_pretty(&policy)?
        }
        "cleardb" => {
            ensure!(
                args == "confirm" && m.peer_id().bot_api_dialog_id_unchecked() == ctx.owner,
                "use cleardb confirm in Saved Messages"
            );
            let bytes = crate::backup::create(&ctx.db, &ctx.modules.lock().unwrap(), ctx.owner)?;
            crate::backup::local(&ctx.root, &bytes, ctx.owner)?;
            ctx.db.restore(json!({}))?;
            "Database cleared; local backup saved".into()
        }
        "clearmodule" => {
            let p: Vec<_> = args.split_whitespace().collect();
            ensure!(
                p.len() == 2 && p[1] == "confirm",
                "usage: clearmodule <external_module> confirm"
            );
            ensure!(
                ctx.modules
                    .lock()
                    .unwrap()
                    .list()
                    .iter()
                    .any(|m| m.name == p[0]),
                "unknown external module"
            );
            ctx.db.transaction(|data| {
                data.remove(p[0]);
                Ok(())
            })?;
            "Module data cleared".into()
        }
        _ => return Ok(None),
    };
    Ok(Some(text))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn watcher_filters() {
        let p = WatcherPolicy {
            incoming_only: true,
            chats_only: true,
            blocked_chats: vec![-2],
            ..Default::default()
        };
        assert!(p.accepts(-1, false));
        assert!(!p.accepts(-1, true));
        assert!(!p.accepts(-2, false));
        assert!(!p.accepts(1, false));
    }
    #[test]
    fn nonick_and_targeted() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        assert!(!addressed(
            &db,
            2,
            -1,
            "ping",
            ".ping",
            Some("mine"),
            false,
            false
        ));
        assert!(addressed(
            &db,
            2,
            -1,
            "ping",
            ".ping@mine",
            Some("mine"),
            false,
            false
        ));
        assert!(addressed(&db, 2, -1, "ping", ".ping", None, false, true));
    }
}
