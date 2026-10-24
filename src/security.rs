use crate::database::Database;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    #[default]
    Owner,
    Private,
    Group,
    Everyone,
    GroupOwner,
    GroupAdmin,
    GroupAdminAddAdmins,
    GroupAdminChangeInfo,
    GroupAdminBanUsers,
    GroupAdminDeleteMessages,
    GroupAdminPinMessages,
    GroupAdminInviteUsers,
    GroupMember,
    Mask {
        bits: u32,
    },
}
impl Permission {
    pub const OWNER: u32 = 1;
    pub const GROUP_OWNER: u32 = 1 << 3;
    pub const GROUP_ADMIN: u32 = 1 << 10;
    pub const GROUP_MEMBER: u32 = 1 << 11;
    pub const PM: u32 = 1 << 12;
    pub const EVERYONE: u32 = 1 << 13;
    pub const VALID: u32 = ((1 << 14) - 1) & !6;
    pub fn bits(self) -> u32 {
        match self {
            Self::Owner => 1,
            Self::Private => Self::PM,
            Self::Group | Self::GroupMember => Self::GROUP_MEMBER,
            Self::Everyone => Self::EVERYONE,
            Self::GroupOwner => Self::GROUP_OWNER,
            Self::GroupAdmin => Self::GROUP_ADMIN,
            Self::GroupAdminAddAdmins => 1 << 4,
            Self::GroupAdminChangeInfo => 1 << 5,
            Self::GroupAdminBanUsers => 1 << 6,
            Self::GroupAdminDeleteMessages => 1 << 7,
            Self::GroupAdminPinMessages => 1 << 8,
            Self::GroupAdminInviteUsers => 1 << 9,
            Self::Mask { bits } => bits,
        }
    }
    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bits() & !Self::VALID == 0,
            "security mask contains unknown or deprecated bits"
        );
        Ok(())
    }
    pub fn parse(spec: &str) -> anyhow::Result<Self> {
        if let Ok(bits) = spec.parse::<u32>() {
            let permission = Self::Mask { bits };
            permission.validate()?;
            return Ok(permission);
        }
        if spec.contains('|') {
            let mut bits = 0;
            for part in spec.split('|') {
                bits |= Self::parse(part.trim())?.bits();
            }
            let permission = Self::Mask { bits };
            permission.validate()?;
            return Ok(permission);
        }
        let permission: Self = serde_json::from_value(serde_json::Value::String(spec.into()))?;
        permission.validate()?;
        Ok(permission)
    }
    pub fn from_value(value: serde_json::Value) -> anyhow::Result<Self> {
        let permission = if let Some(bits) = value.as_u64() {
            Self::Mask {
                bits: u32::try_from(bits)?,
            }
        } else if let Some(spec) = value.as_str() {
            return Self::parse(spec);
        } else {
            serde_json::from_value(value)?
        };
        permission.validate()?;
        Ok(permission)
    }
    pub fn needs_admin(self) -> bool {
        self.bits() & (((1 << 12) - 1) & !7) != 0
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    Command,
    Module,
    Inline,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub kind: RuleKind,
    pub name: String,
    #[serde(default)]
    pub expires: u64,
}
impl Rule {
    pub fn active(&self) -> bool {
        self.expires == 0 || self.expires > now()
    }
    pub fn matches(&self, command: &str, module: Option<&str>, inline: bool) -> bool {
        self.active()
            && match self.kind {
                RuleKind::Command => !inline && self.name == command,
                RuleKind::Module => !inline && module == Some(self.name.as_str()),
                RuleKind::Inline => inline && self.name == command,
            }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityGroup {
    pub name: String,
    #[serde(default)]
    pub users: Vec<i64>,
    #[serde(default)]
    pub permissions: Vec<Rule>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetRule {
    pub target: i64,
    pub chat: bool,
    pub rule: Rule,
}
pub fn groups(db: &Database) -> std::collections::BTreeMap<String, SecurityGroup> {
    serde_json::from_value(db.get("atarustia.security", "sgroups")).unwrap_or_default()
}
pub fn targeted(
    db: &Database,
    user: i64,
    chat: i64,
    command: &str,
    module: Option<&str>,
    inline: bool,
) -> bool {
    let rules: Vec<TargetRule> =
        serde_json::from_value(db.get("atarustia.security", "targeted")).unwrap_or_default();
    rules.iter().any(|r| {
        (if r.chat {
            !inline && r.target == chat
        } else {
            r.target == user
        }) && r.rule.matches(command, module, inline)
    }) || groups(db).values().any(|g| {
        g.users.contains(&user)
            && g.permissions
                .iter()
                .any(|r| r.matches(command, module, inline))
    })
}
pub fn group_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 48
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}
pub fn parse_rule(raw: &str, seconds: u64) -> anyhow::Result<Rule> {
    let (kind, name) = raw.split_once('/').unwrap_or(("command", raw));
    anyhow::ensure!(crate::modules::valid_name(name), "invalid rule name");
    let kind = match kind {
        "command" => RuleKind::Command,
        "module" => RuleKind::Module,
        "inline" => RuleKind::Inline,
        _ => anyhow::bail!("rule must be command/name, module/name or inline/name"),
    };
    if kind == RuleKind::Command {
        anyhow::ensure!(!owner_only(name), "privileged command cannot be delegated");
    }
    anyhow::ensure!(seconds <= 365 * 86400, "maximum duration is 365 days");
    Ok(Rule {
        kind,
        name: name.into(),
        expires: if seconds == 0 { 0 } else { now() + seconds },
    })
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Grant {
    pub user: Option<i64>,
    pub chat: Option<i64>,
    pub command: String,
    pub expires: u64,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn owners(db: &Database, primary: i64) -> Vec<i64> {
    let mut ids: Vec<i64> =
        serde_json::from_value(db.get("atarustia.security", "owner")).unwrap_or_default();
    if !ids.contains(&primary) {
        ids.push(primary);
    }
    ids
}
pub fn allowed(
    db: &Database,
    primary: i64,
    user: Option<i64>,
    chat: i64,
    command: &str,
    permission: Permission,
) -> bool {
    let Some(user) = user else {
        return false;
    };
    if permission.validate().is_err() {
        return false;
    }
    if owners(db, primary).contains(&user) {
        return true;
    }
    // Powerful native commands may never be delegated through a broad permission mask.
    if owner_only(command) {
        return false;
    }
    let grants: Vec<Grant> =
        serde_json::from_value(db.get("atarustia.security", "grants")).unwrap_or_default();
    if grants.iter().any(|g| {
        (g.expires == 0 || g.expires > now())
            && g.command == command
            && (g.user == Some(user) || g.chat == Some(chat))
    }) {
        return true;
    }
    match permission {
        Permission::Owner => false,
        Permission::Private => chat > 0,
        Permission::Group => chat < 0,
        Permission::Everyone => true,
        Permission::Mask { bits } => {
            bits & Permission::EVERYONE != 0
                || chat > 0 && bits & Permission::PM != 0
                || chat < 0 && bits & Permission::GROUP_MEMBER != 0
        }
        _ => false,
    }
}
pub fn owner_only(cmd: &str) -> bool {
    matches!(
        cmd,
        "logs"
            | "suspend"
            | "tglogs"
            | "loops"
            | "newsgroup"
            | "delsgroup"
            | "sgroupadd"
            | "sgroupdel"
            | "sgroup"
            | "sgroups"
            | "tsecrm"
            | "tsecclr"
            | "inlinesec"
            | "querysec"
            | "nonickcmd"
            | "nonickcmds"
            | "nonickuser"
            | "nonickusers"
            | "nonickchat"
            | "nonickchats"
            | "watcher"
            | "watchers"
            | "watcherbl"
            | "helphide"
            | "cleardb"
            | "clearmodule"
            | "dllangpack"
            | "api_fw_protection"
            | "suspend_api_protect"
            | "addacc"
            | "ch_ataraxia_bot"
            | "ch_bot_token"
            | "clearlogs"
            | "setup"
            | "settings"
            | "terminal"
            | "terminate"
            | "ec"
            | "ecpp"
            | "enode"
            | "e"
            | "dlm"
            | "lm"
            | "approve"
            | "review"
            | "importbackup"
            | "ulm"
            | "clearmodules"
            | "owneradd"
            | "ownerrm"
            | "security"
            | "tsec"
            | "restoreall"
            | "restoredb"
            | "restoremods"
            | "backupall"
            | "backupdb"
            | "backupmods"
            | "config"
            | "modules"
            | "ml"
            | "weburl"
            | "restart"
            | "update"
            | "fconfig"
            | "setprefix"
            | "addalias"
            | "delalias"
            | "setlang"
            | "set_backup_period"
    )
}
pub fn effective(db: &Database, permission: Permission) -> Permission {
    if let Some(mask) = db
        .get("atarustia.security", "bounding_mask")
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
    {
        Permission::Mask {
            bits: permission.bits() & mask & Permission::VALID,
        }
    } else {
        permission
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bitmap_matches_original_bits() {
        assert_eq!(
            Permission::parse("group_admin_pin_messages|private")
                .unwrap()
                .bits(),
            (1 << 8) | (1 << 12)
        );
        assert_eq!(
            Permission::parse("4352").unwrap().bits(),
            (1 << 8) | (1 << 12)
        );
        assert!(Permission::parse("2").is_err());
        assert!(Permission::parse("16384").is_err());
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let permission = Permission::parse("private|group_admin_pin_messages").unwrap();
        assert!(allowed(&db, 1, Some(2), 2, "ping", permission));
        assert!(!allowed(&db, 1, Some(2), -2, "ping", permission));
        assert!(!allowed(&db, 1, Some(2), 2, "terminal", permission));
    }
    #[test]
    fn default_deny_and_expiry() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        assert!(!allowed(&db, 1, Some(2), -1, "ping", Permission::Owner));
        assert!(allowed(&db, 1, Some(1), -1, "terminal", Permission::Owner));
        db.set(
            "atarustia.security",
            "grants",
            serde_json::json!([{ "user":2,"chat":null,"command":"ping","expires":now()-1 }]),
        )
        .unwrap();
        assert!(!allowed(&db, 1, Some(2), -1, "ping", Permission::Owner));
        assert!(!allowed(
            &db,
            1,
            Some(2),
            2,
            "terminal",
            Permission::Everyone
        ));
        assert!(!allowed(&db, 1, None, 2, "ping", Permission::Everyone));
    }
    #[test]
    fn precise_target_and_immutable_primary_owner() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path(), 1).unwrap();
        db.set("atarustia.security", "owner", serde_json::json!([2]))
            .unwrap();
        assert!(owners(&db, 1).contains(&1));
        db.set(
            "atarustia.security",
            "grants",
            serde_json::json!([{"user":3,"chat":null,"command":"ping","expires":now()+100}]),
        )
        .unwrap();
        assert!(allowed(&db, 1, Some(3), -5, "ping", Permission::Owner));
        assert!(!allowed(&db, 1, Some(3), -5, "help", Permission::Owner));
        assert!(!allowed(&db, 1, Some(4), -5, "ping", Permission::Owner));
    }
}
