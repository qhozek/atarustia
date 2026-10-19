use crate::{
    commands::Context,
    security::{self, Permission, Rule, RuleKind, SecurityGroup, TargetRule},
};
use anyhow::{Context as _, Result, ensure};
use grammers_client::message::Message;
use serde_json::{Value, json};

pub const COMMANDS: &[(&str, &str)] = &[
    ("newsgroup", "<name> — создать группу доступа"),
    ("delsgroup", "<name> — удалить группу"),
    ("sgroups", "группы доступа"),
    ("sgroup", "<name> — участники и правила"),
    ("sgroupadd", "<group> <user> — добавить участника"),
    ("sgroupdel", "<group> <user> — удалить участника"),
    (
        "tsecrm",
        "user|chat|group <target> <rule> — удалить разрешение",
    ),
    ("tsecclr", "user|chat|group <target> — очистить разрешения"),
    ("inlinesec", "<command> owner|everyone — inline-доступ"),
    ("querysec", "on|off — внешние inline-запросы"),
];
pub async fn resolve_user(ctx: &Context, m: &Message, value: Option<&str>) -> Result<i64> {
    if let Some(value) = value {
        if let Ok(id) = value.parse::<i64>() {
            ensure!(id > 0, "user ID must be positive");
            return Ok(id);
        }
        let peer = ctx
            .client
            .resolve_username(value.trim_start_matches('@'))
            .await?
            .context("user not found")?;
        ensure!(
            matches!(peer, grammers_client::peer::Peer::User(_)),
            "target must be a user"
        );
        return Ok(peer.id().bare_id_unchecked());
    }
    let reply = m
        .get_reply()
        .await?
        .context("provide a user ID, username or reply")?;
    let sender = reply
        .sender_id()
        .filter(|s| s.kind() == grammers_session::types::PeerKind::User)
        .context("reply has no user sender")?;
    Ok(sender.bare_id_unchecked())
}
fn check_rule(ctx: &Context, rule: &Rule) -> Result<()> {
    let registry = ctx.modules.lock().unwrap();
    ensure!(
        match rule.kind {
            RuleKind::Command =>
                crate::commands::is_builtin(&rule.name) || registry.command(&rule.name).is_some(),
            RuleKind::Module => registry.list().iter().any(|m| m.name == rule.name),
            RuleKind::Inline =>
                matches!(rule.name.as_str(), "help" | "ping")
                    || registry.command(&rule.name).is_some(),
        },
        "unknown rule target"
    );
    Ok(())
}
pub async fn execute(
    ctx: &Context,
    m: &Message,
    command: &str,
    args: &str,
) -> Result<Option<String>> {
    let p: Vec<_> = args.split_whitespace().collect();
    let mut groups = security::groups(&ctx.db);
    let text = match command {
        "newsgroup" => {
            ensure!(
                p.len() == 1 && security::group_name(p[0]),
                "usage: newsgroup <alphanumeric_name>"
            );
            ensure!(!groups.contains_key(p[0]), "group exists");
            groups.insert(
                p[0].into(),
                SecurityGroup {
                    name: p[0].into(),
                    users: vec![],
                    permissions: vec![],
                },
            );
            ctx.db.set(
                "atarustia.security",
                "sgroups",
                serde_json::to_value(groups)?,
            )?;
            "Security group created".into()
        }
        "delsgroup" => {
            ensure!(groups.remove(args).is_some(), "unknown group");
            ctx.db.set(
                "atarustia.security",
                "sgroups",
                serde_json::to_value(groups)?,
            )?;
            "Security group deleted".into()
        }
        "sgroups" => serde_json::to_string_pretty(&groups)?,
        "sgroup" => serde_json::to_string_pretty(groups.get(args).context("unknown group")?)?,
        "sgroupadd" | "sgroupdel" => {
            ensure!(
                !p.is_empty() && p.len() <= 2,
                "usage: sgroupadd <group> [user]"
            );
            let user = resolve_user(ctx, m, p.get(1).copied()).await?;
            let group = groups.get_mut(p[0]).context("unknown group")?;
            if command == "sgroupadd" {
                if !group.users.contains(&user) {
                    group.users.push(user);
                }
            } else {
                group.users.retain(|id| *id != user);
            }
            ctx.db.set(
                "atarustia.security",
                "sgroups",
                serde_json::to_value(groups)?,
            )?;
            "Group membership saved".into()
        }
        "tsec" => {
            if p.is_empty() {
                return Ok(Some(serde_json::to_string_pretty(
                    &json!({"targeted":ctx.db.get("atarustia.security","targeted"),"legacy_grants":ctx.db.get("atarustia.security","grants"),"groups":groups}),
                )?));
            }
            let (kind, target, raw, duration) = if p.len() == 3 {
                ("user", p[0], p[1], p[2])
            } else {
                ensure!(
                    p.len() == 4,
                    "usage: tsec user|chat|group <target> command/name|module/name|inline/name <seconds|0>"
                );
                (p[0], p[1], p[2], p[3])
            };
            let rule =
                security::parse_rule(raw, duration.parse().context("duration must be seconds")?)?;
            check_rule(ctx, &rule)?;
            if kind == "group" {
                groups
                    .get_mut(target)
                    .context("unknown group")?
                    .permissions
                    .push(rule);
                ctx.db.set(
                    "atarustia.security",
                    "sgroups",
                    serde_json::to_value(groups)?,
                )?;
            } else {
                ensure!(
                    matches!(kind, "user" | "chat"),
                    "target type must be user, chat or group"
                );
                let target = if kind == "user" {
                    resolve_user(ctx, m, Some(target)).await?
                } else {
                    let id = target.parse::<i64>()?;
                    ensure!(id < 0, "chat must use its negative Bot API ID");
                    id
                };
                let mut rules: Vec<TargetRule> =
                    serde_json::from_value(ctx.db.get("atarustia.security", "targeted"))
                        .unwrap_or_default();
                rules.retain(|r| r.rule.active());
                ensure!(rules.len() < 4096, "too many rules");
                rules.push(TargetRule {
                    target,
                    chat: kind == "chat",
                    rule,
                });
                ctx.db.set(
                    "atarustia.security",
                    "targeted",
                    serde_json::to_value(rules)?,
                )?;
            }
            "Permission granted".into()
        }
        "tsecrm" | "tsecclr" => {
            ensure!(
                p.len() == if command == "tsecrm" { 3 } else { 2 },
                "usage: tsecrm user|chat|group <target> <rule>; tsecclr user|chat|group <target>"
            );
            let rule = if command == "tsecrm" {
                Some(security::parse_rule(p[2], 0)?)
            } else {
                None
            };
            if p[0] == "group" {
                let group = groups.get_mut(p[1]).context("unknown group")?;
                group.permissions.retain(|r| {
                    rule.as_ref()
                        .is_some_and(|target| r.kind != target.kind || r.name != target.name)
                });
                ctx.db.set(
                    "atarustia.security",
                    "sgroups",
                    serde_json::to_value(groups)?,
                )?;
            } else {
                ensure!(matches!(p[0], "user" | "chat"), "invalid target type");
                let id = if p[0] == "user" {
                    resolve_user(ctx, m, Some(p[1])).await?
                } else {
                    p[1].parse()?
                };
                let mut rules: Vec<TargetRule> =
                    serde_json::from_value(ctx.db.get("atarustia.security", "targeted"))
                        .unwrap_or_default();
                rules.retain(|r| {
                    r.target != id
                        || r.chat != (p[0] == "chat")
                        || rule.as_ref().is_some_and(|target| {
                            r.rule.kind != target.kind || r.rule.name != target.name
                        })
                });
                ctx.db.set(
                    "atarustia.security",
                    "targeted",
                    serde_json::to_value(rules)?,
                )?;
                let mut legacy: Vec<security::Grant> =
                    serde_json::from_value(ctx.db.get("atarustia.security", "grants"))
                        .unwrap_or_default();
                legacy.retain(|g| {
                    (if p[0] == "chat" {
                        g.chat != Some(id)
                    } else {
                        g.user != Some(id)
                    }) || rule.as_ref().is_some_and(|r| r.name != g.command)
                });
                ctx.db.set(
                    "atarustia.security",
                    "grants",
                    serde_json::to_value(legacy)?,
                )?;
            }
            "Permissions removed".into()
        }
        "inlinesec" => {
            ensure!(p.len() == 2, "usage: inlinesec <command> owner|everyone");
            ensure!(
                matches!(p[1], "owner" | "everyone"),
                "invalid inline permission"
            );
            ensure!(
                matches!(p[0], "help" | "ping")
                    || ctx
                        .modules
                        .lock()
                        .unwrap()
                        .command(p[0])
                        .is_some_and(|(_, c)| c.inline),
                "unknown inline command"
            );
            let permission: Permission = serde_json::from_value(json!(p[1]))?;
            let mut masks = ctx
                .db
                .get("atarustia.security", "inline_masks")
                .as_object()
                .cloned()
                .unwrap_or_default();
            masks.insert(p[0].into(), serde_json::to_value(permission)?);
            ctx.db
                .set("atarustia.security", "inline_masks", Value::Object(masks))?;
            "Inline permissions saved".into()
        }
        "querysec" => {
            ensure!(matches!(args, "on" | "off"), "usage: querysec on|off");
            ctx.db
                .set("atarustia.security", "allow_inline_query", args == "on")?;
            format!("External inline queries: {args}")
        }
        _ => return Ok(None),
    };
    Ok(Some(text))
}
pub fn inline_allowed(
    db: &crate::database::Database,
    owner: i64,
    user: i64,
    command: &str,
) -> bool {
    if security::owners(db, owner).contains(&user) {
        return true;
    }
    if !db
        .get("atarustia.security", "allow_inline_query")
        .as_bool()
        .unwrap_or(false)
    {
        return false;
    }
    security::targeted(db, user, 0, command, None, true)
        || db
            .get("atarustia.security", "inline_masks")
            .get(command)
            .and_then(Value::as_str)
            == Some("everyone")
}
