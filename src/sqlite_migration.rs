//! Offline reader for legacy Python SQLiteSession v6–v8.
//! Deserialization uses an in-memory, read-only SQLite database, never the source path.
use crate::migration::Imported;
use anyhow::{Context, Result, ensure};
use grammers_session::{
    SessionData,
    types::{ChannelState, PeerId},
};
use rusqlite::{Connection, MAIN_DB, limits::Limit, types::ValueRef};
use std::{
    collections::HashSet,
    path::Path,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_ROWS: usize = 200_000;
const TABLES: [&str; 5] = [
    "version",
    "sessions",
    "entities",
    "sent_files",
    "update_state",
];

pub fn read(path: &Path) -> Result<Imported> {
    // A running SQLite writer may have uncheckpointed data. Never ignore its journal/WAL.
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        ensure!(
            std::fs::symlink_metadata(Path::new(&sidecar))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "SQLite session has journal/WAL sidecars; stop the Python client and checkpoint/close it first"
        );
    }
    let bytes = Zeroizing::new(crate::storage::read_bounded(path, MAX_BYTES as u64)?);
    decode(&bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Imported> {
    ensure!(
        bytes.len() >= 100 && bytes.len() <= MAX_BYTES && bytes.starts_with(b"SQLite format 3\0"),
        "expected SQLite session (100 bytes–32 MiB)"
    );
    ensure!(
        bytes[18] == 1 && bytes[19] == 1,
        "SQLite WAL-mode database is not a standalone snapshot; checkpoint and switch to DELETE journal mode first"
    );
    let mut connection = Connection::open_in_memory()?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_BYTES as i32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, 16 * 1024)?;
    connection.set_limit(Limit::SQLITE_LIMIT_COLUMN, 64)?;
    connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    connection
        .deserialize_read_exact(MAIN_DB, bytes, bytes.len(), true)
        .context("invalid SQLite session image")?;
    connection.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; PRAGMA temp_store=MEMORY;",
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    connection.progress_handler(1000, Some(move || Instant::now() >= deadline));
    validate_schema(&connection)?;
    let version: i64 = connection.query_row("SELECT version FROM version", [], |r| r.get(0))?;
    ensure!(
        (6..=8).contains(&version),
        "unsupported SQLite session version; only v6–v8 are supported (v5 and older have unreliable cached access hashes)"
    );
    validate_columns(&connection, version)?;
    ensure!(
        count(&connection, "version")? == 1,
        "ambiguous SQLite session version"
    );
    ensure!(
        count(&connection, "sessions")? == 1,
        "SQLite session must have exactly one authorization/DC row"
    );
    let mut data = SessionData::default();
    {
        let mut query = connection.prepare(
            "SELECT dc_id, port, length(auth_key), typeof(auth_key), auth_key FROM sessions",
        )?;
        let mut rows = query.query([])?;
        let row = rows
            .next()?
            .context("SQLite session has no authorization row")?;
        let dc: i32 = row.get(0)?;
        let port: i32 = row.get(1)?;
        ensure!(
            (1..=5).contains(&dc) && (1..=65535).contains(&port),
            "invalid Telegram datacenter"
        );
        ensure!(
            row.get::<_, i64>(2)? == 256 && row.get::<_, String>(3)? == "blob",
            "SQLite authorization key must be a 256-byte blob"
        );
        let auth = Zeroizing::new(row.get::<_, Vec<u8>>(4)?);
        let mut key = [0; 256];
        key.copy_from_slice(&auth);
        data.home_dc = dc;
        // As with encrypted binary import, use known production DC endpoints, not an arbitrary legacy address.
        data.dc_options
            .get_mut(&dc)
            .context("unknown Telegram datacenter")?
            .auth_key = Some(key);
    }
    count(&connection, "entities")?;
    let mut query = connection.prepare("SELECT id, hash FROM entities")?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        crate::migration::insert_peer(&mut data, row.get(0)?, row.get(1)?)?;
    }
    count(&connection, "update_state")?;
    let mut query = connection.prepare("SELECT id, pts, qts, date, seq FROM update_state")?;
    let mut rows = query.query([])?;
    let mut state_ids = HashSet::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let pts: i32 = row.get(1)?;
        let qts: i32 = row.get(2)?;
        let seq: i32 = row.get(4)?;
        // Python SQLiteSession saves datetime.timestamp(), often a REAL even for whole seconds.
        let date = match row.get_ref(3)? {
            ValueRef::Integer(n) => {
                i32::try_from(n).context("update date outside Telegram range")?
            }
            ValueRef::Real(n) if n.is_finite() && n >= 0.0 && n <= f64::from(i32::MAX) => n as i32,
            _ => anyhow::bail!("invalid SQLite update timestamp"),
        };
        ensure!(
            id >= 0 && pts >= 0 && qts >= 0 && seq >= 0 && date >= 0,
            "invalid SQLite update state"
        );
        ensure!(state_ids.insert(id), "duplicate SQLite update state ID");
        if id == 0 {
            data.updates_state.pts = pts;
            data.updates_state.qts = qts;
            data.updates_state.date = date;
            data.updates_state.seq = seq;
        } else {
            ensure!(
                PeerId::channel(id).is_some(),
                "invalid channel update state ID"
            );
            data.updates_state.channels.push(ChannelState { id, pts });
        }
    }
    let peers = data.peer_infos.len();
    Ok(Imported { data, peers })
}

fn count(connection: &Connection, table: &str) -> Result<usize> {
    ensure!(TABLES.contains(&table), "unexpected SQLite table");
    // LIMIT bounds rows visited even if a malformed source has no primary key.
    let count: i64 = connection.query_row(
        &format!(
            "SELECT count(*) FROM (SELECT 1 FROM {table} LIMIT {})",
            MAX_ROWS + 1
        ),
        [],
        |r| r.get(0),
    )?;
    ensure!(
        count >= 0 && count as usize <= MAX_ROWS,
        "SQLite session row limit exceeded"
    );
    Ok(count as usize)
}

fn validate_columns(connection: &Connection, version: i64) -> Result<()> {
    let mut tables = vec![
        ("version", vec!["version"]),
        (
            "sessions",
            vec!["dc_id", "server_address", "port", "auth_key", "takeout_id"],
        ),
        ("entities", vec!["id", "hash", "username", "phone", "name"]),
        (
            "sent_files",
            vec!["md5_digest", "file_size", "type", "id", "hash"],
        ),
        ("update_state", vec!["id", "pts", "qts", "date", "seq"]),
    ];
    if version >= 8 {
        tables[1].1.push("tmp_auth_key");
    }
    if version >= 7 {
        tables[2].1.push("date");
    }
    for (table, expected) in tables {
        let mut query = connection.prepare(&format!("PRAGMA table_xinfo({table})"))?;
        let mut rows = query.query([])?;
        let mut names = vec![];
        while let Some(row) = rows.next()? {
            let hidden: i64 = row.get(6)?;
            ensure!(
                hidden == 0,
                "generated/hidden SQLite columns are not supported"
            );
            names.push(row.get::<_, String>(1)?);
        }
        ensure!(
            names == expected,
            "SQLite columns do not match the declared session version"
        );
    }
    Ok(())
}
fn validate_schema(connection: &Connection) -> Result<()> {
    let mut query = connection.prepare("SELECT type,name,sql FROM sqlite_schema LIMIT 33")?;
    let mut rows = query.query([])?;
    let mut seen = HashSet::new();
    let mut total = 0;
    while let Some(row) = rows.next()? {
        total += 1;
        ensure!(total <= 32, "too many SQLite schema objects");
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        let sql: Option<String> = row.get(2)?;
        if kind == "index" && name.starts_with("sqlite_autoindex_") && sql.is_none() {
            continue;
        }
        ensure!(
            kind == "table" && TABLES.contains(&name.as_str()),
            "SQLite session contains unexpected schema objects (views/triggers/extra tables are not supported)"
        );
        let sql = sql.context("missing SQLite table declaration")?;
        ensure!(
            !sql.to_ascii_uppercase().contains("VIRTUAL"),
            "virtual SQLite tables are not supported"
        );
        ensure!(seen.insert(name), "duplicate SQLite table declaration");
    }
    ensure!(
        TABLES.iter().all(|t| seen.contains(*t)),
        "SQLite session is missing required tables"
    );
    Ok(())
}
