//! Explicit offline import of original encrypted ATBINC1 and legacy SQLite sessions.
//! The source is read-only; credentials are never printed and destination files are never replaced.
use crate::{
    config::{AccountConfig, Config, Credentials},
    session::{EncryptedSession, MasterKey},
};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use anyhow::{Context, Result, ensure};
use grammers_session::{
    SessionData,
    types::{ChannelState, PeerAuth, PeerId, PeerInfo},
};
use std::{path::Path, sync::Arc};
use zeroize::Zeroizing;
const MAGIC: &[u8; 8] = b"ATBINC1\0";
const PAYLOAD: &[u8; 8] = b"ATBINP1\0";
const MAX_SIZE: u64 = 32 * 1024 * 1024;
// Legacy wire/data names are accepted on import only, not used for current files or UI.
fn legacy_name() -> String {
    String::from_utf8(vec![97, 116, 97, 114, 97, 120, 105, 97]).expect("ASCII")
}
fn source_value<'a>(value: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    value
        .get(name)
        .or_else(|| value.get(name.replace("atarustia", &legacy_name())))
}
pub struct Imported {
    pub data: SessionData,
    pub peers: usize,
}
pub fn decode(bytes: &[u8], key: &[u8]) -> Result<Imported> {
    ensure!(
        bytes.len() as u64 <= MAX_SIZE && bytes.len() >= 36 && bytes.starts_with(MAGIC),
        "expected encrypted Python ATBINC1 session"
    );
    ensure!(key.len() == 32, "Python session key must contain 32 bytes");
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("invalid key"))?;
    let plain = Zeroizing::new(
        cipher
            .decrypt(
                Nonce::from_slice(&bytes[8..20]),
                Payload {
                    msg: &bytes[20..],
                    aad: MAGIC,
                },
            )
            .map_err(|_| {
                anyhow::anyhow!("Python session authentication failed: wrong key or damaged file")
            })?,
    );
    parse_payload(&plain)
}
fn parse_payload(bytes: &[u8]) -> Result<Imported> {
    let mut r = Reader { bytes, pos: 0 };
    ensure!(r.take(8)? == PAYLOAD, "invalid Python session payload");
    ensure!(r.u16()? == 1, "unsupported Python session version");
    let dc = r.i32()?;
    let port = r.i32()?;
    ensure!(
        (1..=5).contains(&dc) && (1..=65535).contains(&port),
        "invalid Telegram datacenter"
    );
    let _address = r.optional_bytes(256)?;
    let auth = Zeroizing::new(
        r.optional_bytes(256)?
            .context("Python session has no authorization key")?,
    );
    ensure!(auth.len() == 256, "invalid authorization key length");
    let _temporary = Zeroizing::new(r.optional_bytes(256)?.unwrap_or_default());
    let takeout = r.take(1)?[0];
    ensure!(takeout <= 1, "invalid takeout marker");
    if takeout == 1 {
        r.i64()?;
    }
    let mut data = SessionData {
        home_dc: dc,
        ..Default::default()
    };
    let dc_option = data
        .dc_options
        .get_mut(&dc)
        .context("unknown Telegram datacenter")?;
    let mut key = [0; 256];
    key.copy_from_slice(&auth);
    dc_option.auth_key = Some(key);
    let count = r.count(200_000)?;
    for _ in 0..count {
        let id = r.i64()?;
        let hash = r.i64()?;
        for _ in 0..3 {
            r.optional_bytes(16384)?;
        }
        insert_peer(&mut data, id, hash)?;
    }
    let states = r.count(200_000)?;
    for _ in 0..states {
        let entity = r.i64()?;
        let pts = r.i32()?;
        let qts = r.i32()?;
        let date = r.i64()?;
        let seq = r.i32()?;
        ensure!(pts >= 0 && qts >= 0 && seq >= 0, "negative update state");
        if entity == 0 {
            data.updates_state.pts = pts;
            data.updates_state.qts = qts;
            data.updates_state.date =
                i32::try_from(date).context("update date outside Telegram range")?;
            data.updates_state.seq = seq;
        } else {
            ensure!(entity > 0, "invalid channel state ID");
            data.updates_state
                .channels
                .push(ChannelState { id: entity, pts });
        }
    }
    let files = r.count(200_000)?;
    for _ in 0..files {
        r.optional_bytes(64)?;
        r.i64()?;
        let kind = r.take(1)?[0];
        ensure!(kind <= 1, "unsupported Python sent-file type");
        r.i64()?;
        r.i64()?;
    }
    if r.remaining() > 0 {
        ensure!(
            r.take(4)? == b"SEEN",
            "unexpected Python session trailing data"
        );
        r.take(count.checked_mul(2).context("peer counter overflow")?)?;
    }
    ensure!(
        r.remaining() == 0,
        "unexpected Python session trailing data"
    );
    let peers = data.peer_infos.len();
    Ok(Imported { data, peers })
}
pub(crate) fn insert_peer(data: &mut SessionData, id: i64, hash: i64) -> Result<()> {
    let peer =
        PeerId::from_bot_api_dialog_id(id).context("invalid cached peer ID in Python session")?;
    let auth = Some(PeerAuth::from_hash(hash));
    let info = match peer.kind() {
        grammers_session::types::PeerKind::User => PeerInfo::User {
            id: peer.bare_id_unchecked(),
            auth,
            bot: None,
            is_self: None,
        },
        grammers_session::types::PeerKind::Chat => PeerInfo::Chat {
            id: peer.bare_id_unchecked(),
        },
        grammers_session::types::PeerKind::Channel => PeerInfo::Channel {
            id: peer.bare_id_unchecked(),
            auth,
            kind: None,
        },
    };
    ensure!(
        data.peer_infos.insert(peer, info).is_none(),
        "duplicate cached peer ID in Python session"
    );
    Ok(())
}
pub fn import_directory(source: &Path, destination: &Path) -> Result<usize> {
    ensure!(source.is_dir(), "Python data directory does not exist");
    let source = source.canonicalize()?;
    // Reject a not-yet-created destination inside the source before create_dir_all.
    let absolute = std::path::absolute(destination)?;
    let ancestor = absolute
        .ancestors()
        .find(|p| p.exists())
        .context("destination has no existing ancestor")?
        .canonicalize()?;
    ensure!(
        !ancestor.starts_with(&source),
        "source and destination must be independent directories (not nested)"
    );
    std::fs::create_dir_all(destination)?;
    let destination = destination.canonicalize()?;
    ensure!(
        source != destination
            && !destination.starts_with(&source)
            && !source.starts_with(&destination),
        "source and destination must be independent directories (not nested)"
    );
    let _lock = crate::storage::DataLock::acquire(&destination)?;
    ensure!(
        !destination.join("config.json").exists(),
        "destination already has config.json; import requires a new Rust data directory"
    );
    ensure!(
        !std::fs::read_dir(&destination)?
            .filter_map(|e| e.ok())
            .any(|e| e
                .file_name()
                .to_str()
                .is_some_and(|s| s.starts_with("config-") && s.ends_with(".json"))
                || e.path().extension().is_some_and(|e| e == "atsession")),
        "destination already contains account data"
    );
    let source_config: serde_json::Value = serde_json::from_slice(&crate::storage::read_bounded(
        &source.join("config.json"),
        1024 * 1024,
    )?)?;
    let id = source_config
        .get("api_id")
        .context("Python API ID missing")?;
    let api_id = if let Some(n) = id.as_i64() {
        i32::try_from(n)?
    } else {
        id.as_str().context("invalid API ID")?.parse()?
    };
    let api_hash = source_config
        .get("api_hash")
        .and_then(serde_json::Value::as_str)
        .context("Python API hash missing")?
        .into();
    let credentials = Credentials { api_id, api_hash };
    credentials.validate()?;
    let mut key: Option<Zeroizing<Vec<u8>>> = None;
    let mut sessions = vec![];
    let mut databases = vec![];
    let mut config = Config {
        credentials: Some(credentials),
        ..Default::default()
    };
    for entry in std::fs::read_dir(&source)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(filename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let old_prefix = format!("{}-", legacy_name());
        let Some(suffix) = filename
            .strip_prefix("atarustia-")
            .or_else(|| filename.strip_prefix(&old_prefix))
        else {
            continue;
        };
        let (name, sqlite) = if let Some(name) = suffix.strip_suffix(".atsession") {
            (name, false)
        } else if let Some(name) = suffix.strip_suffix(".session") {
            (name, true)
        } else {
            continue;
        };
        if name.ends_with("-bot") {
            continue;
        }
        let owner: i64 = name
            .parse()
            .context("unrecognized Python user session filename")?;
        ensure!(PeerId::user(owner).is_some(), "invalid Python account ID");
        ensure!(sessions.len() < 64, "Python import exceeds 64 accounts");
        let decoded = if sqlite {
            crate::sqlite_migration::read(&path)?
        } else {
            if key.is_none() {
                let encoded = Zeroizing::new(crate::storage::read_bounded(
                    &if source.join("atarustia.key").is_file() {
                        source.join("atarustia.key")
                    } else {
                        source.join(format!("{}.key", legacy_name()))
                    },
                    1024,
                )?);
                let decoded = Zeroizing::new(
                    hex::decode(std::str::from_utf8(&encoded)?.trim())
                        .context("Python master key must be 32 bytes encoded as hex")?,
                );
                ensure!(decoded.len() == 32, "invalid Python master key");
                key = Some(decoded);
            }
            let bytes = Zeroizing::new(crate::storage::read_bounded(&path, MAX_SIZE)?);
            decode(&bytes, key.as_ref().expect("loaded key"))?
        };
        let slot = format!("python-{owner}");
        ensure!(
            !config.accounts.contains_key(&slot),
            "multiple Python sessions for the same owner; choose one source format in a separate import directory"
        );
        let target = destination.join(format!("atarustia-user-{slot}.atsession"));
        ensure!(!target.exists(), "destination session exists");
        let db_path = source.join(format!("config-{owner}.json"));
        ensure!(
            !destination.join(format!("config-{owner}.json")).exists(),
            "destination account database exists"
        );
        let bot_token = if db_path.is_file() {
            let db: serde_json::Value =
                serde_json::from_slice(&crate::storage::read_bounded(&db_path, 16 * 1024 * 1024)?)?;
            let supported = preferences(&db)?;
            databases.push((owner, supported));
            source_value(&db, "atarustia.inline")
                .and_then(|v| v.get("bot_token"))
                .and_then(serde_json::Value::as_str)
                .filter(|t| crate::config::validate_bot_token(t).is_ok())
                .map(str::to_owned)
        } else {
            None
        };
        config.accounts.insert(
            slot,
            AccountConfig {
                owner_id: Some(owner),
                bot_token,
                needs_login: false,
            },
        );
        sessions.push((target, decoded.data));
    }
    ensure!(!sessions.is_empty(), "no Python user sessions found");
    // Every source has been decoded/validated before writing any session or config.
    let master = Arc::new(MasterKey::load(&destination)?);
    let count = sessions.len();
    for (target, data) in sessions {
        EncryptedSession::import(target, master.clone(), data)?;
    }
    for (owner, data) in databases {
        let db = crate::database::Database::open(&destination, owner)?;
        db.restore(data)?;
    }
    config.save(&destination)?;
    Ok(count)
}
pub fn preferences(original: &serde_json::Value) -> Result<serde_json::Value> {
    use serde_json::{Value, json};
    ensure!(original.is_object(), "Python database must be an object");
    let mut migrated = json!({});
    if let Some(main) = source_value(original, "atarustia.main").and_then(Value::as_object) {
        let mut result = serde_json::Map::new();
        if let Some(prefix) = main.get("command_prefix").and_then(Value::as_str) {
            ensure!(
                !prefix.is_empty()
                    && prefix.chars().count() <= 8
                    && !prefix.chars().any(char::is_whitespace),
                "invalid migrated prefix"
            );
            result.insert("command_prefix".into(), json!(prefix));
        }
        for name in ["no_nickname", "grep"] {
            if let Some(value) = main.get(name).and_then(Value::as_bool) {
                result.insert(name.into(), json!(value));
            }
        }
        for name in ["nonickusers", "nonickchats"] {
            if let Some(array) = main.get(name).and_then(Value::as_array) {
                ensure!(
                    array.len() <= 4096 && array.iter().all(|v| v.as_i64().is_some()),
                    "invalid migrated NoNick IDs"
                );
                result.insert(name.into(), json!(array));
            }
        }
        if let Some(array) = main.get("nonickcmds").and_then(Value::as_array) {
            ensure!(
                array.len() <= 4096
                    && array
                        .iter()
                        .all(|v| v.as_str().is_some_and(crate::modules::valid_name)),
                "invalid migrated NoNick commands"
            );
            result.insert("nonickcmds".into(), json!(array));
        }
        if let Some(prefixes) = main.get("command_prefixes").and_then(Value::as_object) {
            ensure!(
                prefixes.len() <= 4096
                    && prefixes
                        .iter()
                        .all(|(id, value)| id.parse::<i64>().is_ok_and(|i| i > 0)
                            && value.as_str().is_some_and(|s| !s.is_empty()
                                && s.chars().count() <= 8
                                && !s.chars().any(char::is_whitespace))),
                "invalid personal prefixes"
            );
            result.insert("command_prefixes".into(), json!(prefixes));
        }
        migrated["atarustia.main"] = Value::Object(result);
    }
    if let Some(lang) = source_value(original, "atarustia.translations")
        .and_then(|v| v.get("lang"))
        .and_then(Value::as_str)
    {
        let preferred = lang
            .split_whitespace()
            .find(|l| matches!(*l, "ru" | "en"))
            .unwrap_or("en");
        migrated["atarustia.translations"] = json!({"lang":preferred});
    }
    if let Some(aliases) = original
        .get("CoreMod")
        .or_else(|| original.get("Settings"))
        .and_then(|v| v.get("aliases"))
        .and_then(Value::as_object)
    {
        ensure!(
            aliases.len() <= 4096
                && aliases
                    .iter()
                    .all(|(name, value)| crate::modules::valid_name(name)
                        && value.as_str().is_some_and(|s| s.len() <= 8192)),
            "invalid migrated aliases"
        );
        migrated["Settings"] = json!({"aliases":aliases});
    }
    if let Some(period) = source_value(original, "atarustiaBackupMod").and_then(|v| v.get("period"))
    {
        let hours = period
            .as_u64()
            .map(|s| s.div_ceil(3600))
            .unwrap_or(0)
            .min(8760);
        migrated["RustBackup"] = json!({"hours":hours,"last":crate::security::now()});
    }
    // Old permission masks and owner delegations need deliberate re-approval: command semantics have changed.
    let pending_modules = original
        .get("LoaderMod")
        .and_then(|v| v.get("loaded_modules"))
        .cloned()
        .unwrap_or(json!({}));
    migrated["RustMigration"] = json!({"source":"python-import","python_modules_not_loaded":pending_modules,"security_requires_review":source_value(original,"atarustia.security").is_some(),"created":crate::security::now()});
    Ok(migrated)
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.remaining(), "truncated Python session");
        let result = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(result)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn count(&mut self, max: usize) -> Result<usize> {
        let count = u32::from_le_bytes(self.take(4)?.try_into()?) as usize;
        ensure!(count <= max, "Python session entry limit exceeded");
        Ok(count)
    }
    fn optional_bytes(&mut self, max: usize) -> Result<Option<Vec<u8>>> {
        let size = u32::from_le_bytes(self.take(4)?.try_into()?);
        if size == u32::MAX {
            return Ok(None);
        }
        let size = size as usize;
        ensure!(size <= max, "Python session field too large");
        Ok(Some(self.take(size)?.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_import_destination_is_rejected() {
        let source = tempfile::tempdir().unwrap();
        let destination = source.path().join("rust");
        assert!(import_directory(source.path(), &destination).is_err());
        assert!(!destination.exists());
    }
    #[test]
    fn supported_preferences_do_not_execute_or_restore_old_delegations() {
        let old = serde_json::json!({"atarustia.main":{"command_prefix":"!!","nonickusers":[123]},"atarustia.translations":{"lang":"ru en"},"CoreMod":{"aliases":{"p":"ping"}},"atarustiaBackupMod":{"period":86400},"atarustia.security":{"owner":[999]},"LoaderMod":{"loaded_modules":{"OldMod":"https://example.com/old.py"}}});
        let migrated = preferences(&old).unwrap();
        assert_eq!(migrated["atarustia.main"]["command_prefix"], "!!");
        assert_eq!(migrated["Settings"]["aliases"]["p"], "ping");
        assert_eq!(migrated["RustBackup"]["hours"], 24);
        assert!(migrated.get("atarustia.security").is_none());
        assert_eq!(
            migrated["RustMigration"]["python_modules_not_loaded"]["OldMod"],
            "https://example.com/old.py"
        );
    }
    #[test]
    fn legacy_names_are_only_normalized_on_import() {
        let old = legacy_name();
        let mut source = serde_json::Map::new();
        source.insert(
            format!("{old}.main"),
            serde_json::json!({"command_prefix":"!!"}),
        );
        source.insert(
            format!("{old}.security"),
            serde_json::json!({"owner":[999]}),
        );
        let converted = preferences(&serde_json::Value::Object(source)).unwrap();
        assert_eq!(converted["atarustia.main"]["command_prefix"], "!!");
        assert_eq!(converted["RustMigration"]["security_requires_review"], true);
        assert!(converted.get(format!("{old}.main")).is_none());
    }
    #[test]
    fn actual_python_binary_session_fixture() {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(include_str!("../tests/fixtures/python_session.base64").trim())
            .unwrap();
        let imported = decode(&bytes, &[9; 32]).unwrap();
        assert_eq!(imported.data.home_dc, 2);
        assert_eq!(imported.peers, 2);
        assert_eq!(imported.data.updates_state.pts, 10);
        assert_eq!(imported.data.updates_state.qts, 20);
        assert_eq!(imported.data.updates_state.seq, 30);
        assert_eq!(imported.data.updates_state.channels[0].id, 789);
        assert_eq!(imported.data.updates_state.channels[0].pts, 40);
        let peer = imported
            .data
            .peer_infos
            .get(&PeerId::user(123).unwrap())
            .unwrap();
        assert_eq!(peer.auth().unwrap().hash(), 456);
    }
    #[test]
    fn import_creates_separate_rust_session_without_overwriting_source() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("config.json"),
            r#"{"api_id":123,"api_hash":"00000000000000000000000000000000"}"#,
        )
        .unwrap();
        std::fs::write(source.path().join("atarustia.key"), hex::encode([9; 32])).unwrap();
        let path = source.path().join("atarustia-123.atsession");
        let bytes = fixture();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(import_directory(source.path(), dest.path()).unwrap(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let config = Config::load(dest.path()).unwrap();
        assert_eq!(config.accounts["python-123"].owner_id, Some(123));
        let key = Arc::new(MasterKey::load(dest.path()).unwrap());
        let session =
            EncryptedSession::load(dest.path().join("atarustia-user-python-123.atsession"), key)
                .unwrap();
        use grammers_session::Session;
        assert_eq!(session.home_dc_id().unwrap(), 2);
        assert!(import_directory(source.path(), dest.path()).is_err());
    }
    fn fixture() -> Vec<u8> {
        let mut b = PAYLOAD.to_vec();
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2i32.to_le_bytes());
        b.extend_from_slice(&443i32.to_le_bytes());
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(b"host");
        b.extend_from_slice(&256u32.to_le_bytes());
        b.extend_from_slice(&[7; 256]);
        b.extend_from_slice(&u32::MAX.to_le_bytes());
        b.push(0);
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(b"SEEN");
        let key = [9; 32];
        let nonce = [3; 12];
        let encrypted = Aes256Gcm::new_from_slice(&key)
            .unwrap()
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &b,
                    aad: MAGIC,
                },
            )
            .unwrap();
        [MAGIC.as_slice(), &nonce, &encrypted].concat()
    }
    #[test]
    fn encrypted_original_format_and_tampering() {
        let bytes = fixture();
        let imported = decode(&bytes, &[9; 32]).unwrap();
        assert_eq!(imported.data.home_dc, 2);
        assert_eq!(imported.data.dc_options[&2].auth_key, Some([7; 256]));
        assert!(decode(&bytes, &[8; 32]).is_err());
        let mut bad = bytes;
        bad[30] ^= 1;
        assert!(decode(&bad, &[9; 32]).is_err());
    }
    #[test]
    fn source_is_read_only_and_wrong_key_writes_nothing() {
        let source = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(
            source.path().join("config.json"),
            r#"{"api_id":123,"api_hash":"00000000000000000000000000000000"}"#,
        )
        .unwrap();
        std::fs::write(source.path().join("atarustia.key"), hex::encode([8; 32])).unwrap();
        let path = source.path().join("atarustia-123.atsession");
        std::fs::write(&path, fixture()).unwrap();
        assert!(import_directory(source.path(), dest.path()).is_err());
        assert!(!dest.path().join("config.json").exists());
        assert!(!dest.path().join("atarustia.key").exists());
        assert_eq!(std::fs::read(path).unwrap(), fixture());
    }
}
