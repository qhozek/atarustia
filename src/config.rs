use crate::storage::atomic_write;
use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Parser, Debug)]
#[command(version, about = "atarustia · native Rust Telegram userbot")]
pub struct Options {
    #[arg(long, default_value = "data")]
    pub data_root: PathBuf,
    #[arg(long)]
    pub no_web: bool,
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub listen: std::net::SocketAddr,
    #[arg(long, default_value = "default")]
    pub account: String,
    #[arg(long)]
    pub no_auth: bool,
    #[arg(long, env = "ATARUSTIA_API_ID")]
    pub api_id: Option<i32>,
    #[arg(long, env = "ATARUSTIA_API_HASH", hide_env_values = true)]
    pub api_hash: Option<String>,
    #[arg(long, env = "ATARUSTIA_BOT_TOKEN", hide_env_values = true)]
    pub bot_token: Option<String>,
    /// Enable owner-only .terminal/.ec/.ecpp/.enode commands. Disabled by default.
    #[arg(long)]
    pub allow_shell: bool,
    /// SOCKS5 URL or MTProxy URL/link (mtproxy://host:port?secret=..., tg://proxy, https://t.me/proxy).
    #[arg(long, env = "ATARUSTIA_PROXY", hide_env_values = true)]
    pub proxy: Option<String>,
    /// Do not create or discover an inline bot through BotFather.
    #[arg(long)]
    pub no_bot: bool,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long, short = 'p')]
    pub phone: Option<String>,
    /// Default Telegram Desktop API pair, or custom credentials entered in the terminal.
    #[arg(long,default_value="ask",value_parser=["ask","tdesktop","custom"])]
    pub api_preset: String,
    /// Offline import of encrypted binary v1 / legacy SQLite v6–v8 from a separate Python data directory.
    #[arg(long, conflicts_with = "import_backup")]
    pub import_python: Option<PathBuf>,
    /// Offline review/conversion of an original Python .backup/.zip/JSON backup.
    #[arg(long, requires = "import_owner")]
    pub import_backup: Option<PathBuf>,
    /// Account owner for legacy backups (the old formats do not reliably encode it).
    #[arg(long, requires = "import_backup")]
    pub import_owner: Option<i64>,
}
pub fn desktop_credentials() -> Credentials {
    Credentials {
        api_id: 611335,
        api_hash: "d524b414d21f4d37f08684c1df41ac9c".into(),
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub api_id: i32,
    pub api_hash: String,
}
impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("api_id", &self.api_id)
            .finish_non_exhaustive()
    }
}
impl Credentials {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.api_id > 0, "API ID must be positive");
        ensure!(
            self.api_hash.len() == 32 && self.api_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "API hash must be 32 hex characters"
        );
        Ok(())
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AccountConfig {
    pub owner_id: Option<i64>,
    pub bot_token: Option<String>,
    pub needs_login: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub credentials: Option<Credentials>,
    pub accounts: BTreeMap<String, AccountConfig>,
}
impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let p = root.join("config.json");
        if !p.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&std::fs::read(&p)?)
            .with_context(|| format!("invalid {}; refusing to overwrite", p.display()))
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        atomic_write(&root.join("config.json"), &serde_json::to_vec_pretty(self)?)
    }
}
pub fn validate_account(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 48
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "account must contain 1–48 ASCII letters, digits, _ or -"
    );
    Ok(())
}
pub fn validate_bot_token(token: &str) -> Result<()> {
    let (id, secret) = token
        .split_once(':')
        .context("bot token must contain ID:secret")?;
    ensure!(
        id.parse::<i64>().is_ok_and(|id| id > 0)
            && (30..=64).contains(&secret.len())
            && secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid bot token"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_cannot_escape_data_root() {
        for n in ["../x", "a/b", "a\\b", "", ".", "a b"] {
            assert!(validate_account(n).is_err());
        }
        assert!(validate_account("my-account_1").is_ok());
    }
    #[test]
    fn config_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::default();
        c.accounts.insert("one".into(), AccountConfig::default());
        c.save(dir.path()).unwrap();
        assert!(
            Config::load(dir.path())
                .unwrap()
                .accounts
                .contains_key("one")
        );
    }
    #[test]
    fn invalid_config_is_never_replaced() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("config.json"), b"{broken").unwrap();
        assert!(Config::load(d.path()).is_err());
        assert_eq!(
            std::fs::read(d.path().join("config.json")).unwrap(),
            b"{broken"
        );
    }
    #[test]
    fn credentials_and_tokens() {
        assert!(
            Credentials {
                api_id: 1,
                api_hash: "0".repeat(32)
            }
            .validate()
            .is_ok()
        );
        assert!(
            Credentials {
                api_id: 0,
                api_hash: "0".repeat(32)
            }
            .validate()
            .is_err()
        );
        assert!(
            Credentials {
                api_id: 1,
                api_hash: "z".repeat(32)
            }
            .validate()
            .is_err()
        );
        assert!(validate_bot_token(&format!("123:{}", "a".repeat(35))).is_ok());
        for token in ["123", "-1:short", "x:123", ""] {
            assert!(validate_bot_token(token).is_err());
        }
    }
}
