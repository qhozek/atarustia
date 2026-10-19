use atarustia::{
    config::Config,
    migration,
    session::{EncryptedSession, MasterKey},
    sqlite_migration,
};
use base64::Engine as _;
use grammers_session::{Session, types::PeerId};
use rusqlite::{Connection, params};
use std::{
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

fn fixture() -> Vec<u8> {
    let zip = base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures/python_sqlite.zip.base64").trim())
        .unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    let mut bytes = vec![];
    archive
        .by_name("synthetic.session")
        .unwrap()
        .read_to_end(&mut bytes)
        .unwrap();
    bytes
}
fn source(root: &Path) {
    std::fs::write(
        root.join("config.json"),
        br#"{"api_id":123,"api_hash":"00000000000000000000000000000000"}"#,
    )
    .unwrap();
    std::fs::write(root.join("atarustia-123.session"), fixture()).unwrap();
}
fn mutate(sql: &str) -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.session");
    std::fs::write(&path, fixture()).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch(sql).unwrap();
    drop(connection);
    std::fs::read(path).unwrap()
}
#[test]
fn original_sqlite_fixture_has_dc_peers_and_fractional_timestamp() {
    let imported = sqlite_migration::decode(&fixture()).unwrap();
    assert_eq!(imported.data.home_dc, 2);
    assert_eq!(imported.data.dc_options[&2].auth_key, Some([7; 256]));
    assert_eq!(imported.peers, 3);
    assert_eq!(
        imported.data.peer_infos[&PeerId::user(123).unwrap()]
            .auth()
            .unwrap()
            .hash(),
        456
    );
    assert_eq!(
        imported.data.peer_infos[&PeerId::channel(789).unwrap()]
            .auth()
            .unwrap()
            .hash(),
        654
    );
    assert!(
        imported
            .data
            .peer_infos
            .contains_key(&PeerId::chat(456).unwrap())
    );
    assert_eq!(imported.data.updates_state.pts, 10);
    assert_eq!(imported.data.updates_state.qts, 20);
    assert_eq!(imported.data.updates_state.seq, 30);
    assert_eq!(imported.data.updates_state.date, 1792324800);
    assert_eq!(imported.data.updates_state.channels[0].id, 789);
    assert_eq!(imported.data.updates_state.channels[0].pts, 40);
}
#[test]
fn v6_v7_are_read_without_schema_upgrades() {
    for (version, sql) in [
        (
            6,
            "ALTER TABLE sessions DROP COLUMN tmp_auth_key; ALTER TABLE entities DROP COLUMN date;",
        ),
        (7, "ALTER TABLE sessions DROP COLUMN tmp_auth_key;"),
    ] {
        let bytes = mutate(&format!("{sql} UPDATE version SET version={version};"));
        assert_eq!(sqlite_migration::decode(&bytes).unwrap().peers, 3);
    }
}
#[test]
fn malformed_versions_auth_peers_states_and_views_are_rejected() {
    for sql in [
        "UPDATE version SET version=5",
        "UPDATE version SET version=9",
        "DELETE FROM version",
        "INSERT INTO version VALUES (7)",
        "DELETE FROM sessions",
        "UPDATE sessions SET auth_key=x'0102'",
        "UPDATE sessions SET dc_id=99",
        "UPDATE sessions SET port=0",
        "UPDATE entities SET id=0 WHERE id=123",
        "UPDATE update_state SET pts=-1",
        "UPDATE update_state SET date='invalid'",
        "UPDATE update_state SET date=1e100",
        "UPDATE update_state SET date=-1",
        "UPDATE update_state SET id=-1 WHERE id=789",
        "DROP TABLE entities",
        "ALTER TABLE entities ADD COLUMN extra INTEGER",
        "ALTER TABLE entities ADD COLUMN computed INTEGER GENERATED ALWAYS AS (id+1) VIRTUAL",
        "UPDATE version SET version=7",
        "CREATE TABLE extra (x)",
        "CREATE TRIGGER unexpected AFTER UPDATE ON sessions BEGIN SELECT 1; END",
        "ALTER TABLE entities RENAME TO saved_entities; CREATE VIEW entities AS SELECT * FROM saved_entities",
    ] {
        assert!(
            sqlite_migration::decode(&mutate(sql)).is_err(),
            "accepted {sql}"
        );
    }
    let mut bytes = fixture();
    bytes[18] = 2;
    bytes[19] = 2;
    assert!(sqlite_migration::decode(&bytes).is_err());
    assert!(sqlite_migration::decode(b"SQLite format 3\0").is_err());
}
#[test]
fn sqlite_import_is_read_only_and_creates_encrypted_rust_session() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    source(src.path());
    std::fs::write(
        src.path().join("config-123.json"),
        br#"{"atarustia.main":{"command_prefix":"!!"},"atarustia.security":{"owner":[999]}}"#,
    )
    .unwrap();
    let before = std::fs::read(src.path().join("atarustia-123.session")).unwrap();
    assert_eq!(
        migration::import_directory(src.path(), dst.path()).unwrap(),
        1
    );
    assert_eq!(
        std::fs::read(src.path().join("atarustia-123.session")).unwrap(),
        before
    );
    assert!(!src.path().join("atarustia.key").exists());
    assert_eq!(
        Config::load(dst.path()).unwrap().accounts["python-123"].owner_id,
        Some(123)
    );
    let path = dst.path().join("atarustia-user-python-123.atsession");
    let encrypted = std::fs::read(&path).unwrap();
    assert!(encrypted.starts_with(b"ATRUST1\0"));
    assert!(!encrypted.windows(256).any(|b| b == [7; 256]));
    let session =
        EncryptedSession::load(path, Arc::new(MasterKey::load(dst.path()).unwrap())).unwrap();
    assert_eq!(session.home_dc_id().unwrap(), 2);
    assert_eq!(
        session.dc_option(2).unwrap().unwrap().auth_key,
        Some([7; 256])
    );
    let db = atarustia::database::Database::open(dst.path(), 123).unwrap();
    assert_eq!(db.prefix(), "!!");
    assert!(db.snapshot().unwrap().get("atarustia.security").is_none());
    assert!(migration::import_directory(src.path(), dst.path()).is_err());
}
#[test]
fn journal_sidecars_are_not_ignored_and_fail_before_destination_secrets() {
    for suffix in ["-wal", "-shm", "-journal"] {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        source(src.path());
        let path = src.path().join(format!("atarustia-123.session{suffix}"));
        std::fs::write(&path, b"pending").unwrap();
        assert!(migration::import_directory(src.path(), dst.path()).is_err());
        assert!(!dst.path().join("atarustia.key").exists());
        assert!(!dst.path().join("config.json").exists());
        assert_eq!(std::fs::read(path).unwrap(), b"pending");
    }
}
#[test]
fn duplicate_formats_are_rejected_before_writing_accounts() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    source(src.path());
    let binary = base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures/python_session.base64").trim())
        .unwrap();
    std::fs::write(src.path().join("atarustia-123.atsession"), binary).unwrap();
    std::fs::write(src.path().join("atarustia.key"), hex::encode([9; 32])).unwrap();
    assert!(migration::import_directory(src.path(), dst.path()).is_err());
    assert!(!dst.path().join("atarustia.key").exists());
    assert!(!dst.path().join("config.json").exists());
}
#[test]
fn row_budget_rejects_oversized_entity_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.session");
    std::fs::write(&path, fixture()).unwrap();
    let mut connection = Connection::open(&path).unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut query = transaction
            .prepare("INSERT INTO entities (id,hash) VALUES (?,?)")
            .unwrap();
        for id in 1000..201001 {
            query.execute(params![id, 1]).unwrap();
        }
    }
    transaction.commit().unwrap();
    drop(connection);
    assert!(sqlite_migration::read(&path).is_err());
}

#[test]
fn mixed_formats_for_different_accounts_import_together() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    source(src.path());
    let binary = base64::engine::general_purpose::STANDARD
        .decode(include_str!("fixtures/python_session.base64").trim())
        .unwrap();
    std::fs::write(src.path().join("atarustia-456.atsession"), &binary).unwrap();
    std::fs::write(src.path().join("atarustia.key"), hex::encode([9; 32])).unwrap();
    assert_eq!(
        migration::import_directory(src.path(), dst.path()).unwrap(),
        2
    );
    let config = Config::load(dst.path()).unwrap();
    assert_eq!(config.accounts.len(), 2);
    let key = Arc::new(MasterKey::load(dst.path()).unwrap());
    for owner in [123, 456] {
        assert_eq!(
            config.accounts[&format!("python-{owner}")].owner_id,
            Some(owner)
        );
        let session = EncryptedSession::load(
            dst.path()
                .join(format!("atarustia-user-python-{owner}.atsession")),
            key.clone(),
        )
        .unwrap();
        assert_eq!(
            session.dc_option(2).unwrap().unwrap().auth_key,
            Some([7; 256])
        );
    }
    assert_eq!(
        std::fs::read(src.path().join("atarustia-456.atsession")).unwrap(),
        binary
    );
}

#[test]
fn destination_lock_prevents_parallel_import_before_secret_writes() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    source(src.path());
    let _lock = atarustia::storage::DataLock::acquire(dst.path()).unwrap();
    assert!(migration::import_directory(src.path(), dst.path()).is_err());
    assert!(!dst.path().join("atarustia.key").exists());
    assert!(!dst.path().join("config.json").exists());
}
