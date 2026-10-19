//! Bounded import of original .backup/mods.zip/JSON snapshots.
//! Never extracts archive paths, runs Python or installs requirements.
use crate::{
    backup::{Backup, Kind},
    storage::atomic_write,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const MAX_INPUT: usize = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 32 * 1024 * 1024;
const MAX_MEMBER: u64 = 16 * 1024 * 1024;
const MAX_SOURCE: u64 = 512 * 1024;
const MAX_ENTRIES: usize = 256;

pub struct Import {
    pub backup: Backup,
    pub sources: BTreeMap<String, Vec<u8>>,
    pub requirements: BTreeMap<String, Vec<u8>>,
    pub module_urls: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuarantinedFile {
    pub original_name: String,
    pub stored_name: String,
    pub sha256: String,
    pub bytes: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub format: String,
    pub version: u32,
    pub owner: i64,
    pub archive_sha256: String,
    pub has_database: bool,
    pub python_sources: Vec<QuarantinedFile>,
    pub requirements: Vec<QuarantinedFile>,
    pub module_urls: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}
pub struct Staged {
    pub directory: PathBuf,
    pub report: Report,
}
#[derive(Default)]
struct Budget {
    expanded: u64,
    entries: usize,
}
#[derive(Default)]
struct Parts {
    db: Option<Value>,
    sources: BTreeMap<String, Vec<u8>>,
    requirements: BTreeMap<String, Vec<u8>>,
    urls: BTreeMap<String, String>,
    stored_names: HashSet<String>,
}

/// The owner is explicit: original ZIP and DB-only backups have no reliable account identity.
pub fn parse(bytes: &[u8], owner: i64) -> Result<Import> {
    ensure!(
        grammers_session::types::PeerId::user(owner).is_some(),
        "invalid backup owner ID"
    );
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_INPUT,
        "legacy backup must be 1 byte–16 MiB"
    );
    let mut parts = Parts::default();
    if bytes.starts_with(b"PK") {
        archive(bytes, owner, 0, &mut Budget::default(), &mut parts)?;
    } else {
        let value: Value =
            serde_json::from_slice(bytes).context("expected Python JSON or ZIP backup")?;
        ensure!(
            value.get("format").is_none(),
            "this is not a Python backup; use the normal Rust restore commands"
        );
        if value
            .as_object()
            .is_some_and(|m| m.values().all(Value::is_string))
            && !value.as_object().unwrap().is_empty()
        {
            parts.urls = urls(&value)?;
        } else {
            validate_db(&value)?;
            parts.db = Some(value);
        }
    }
    ensure!(
        parts.db.is_some() || !parts.sources.is_empty() || !parts.urls.is_empty(),
        "no supported backup content found"
    );
    let kind = if parts.db.is_some() {
        Kind::Database
    } else {
        Kind::Modules
    };
    let mut database = if let Some(db) = parts.db {
        crate::migration::preferences(&db)?
    } else {
        json!({})
    };
    if !parts.urls.is_empty() {
        if database.get("RustMigration").is_none() {
            database["RustMigration"] = json!({});
        }
        database["RustMigration"]["python_modules_not_loaded"] = json!(parts.urls);
    }
    Ok(Import {
        backup: Backup {
            format: "atarustia-rust".into(),
            version: 1,
            owner,
            created: crate::security::now(),
            database,
            modules: vec![],
            kind,
        },
        sources: parts.sources,
        requirements: parts.requirements,
        module_urls: parts.urls,
    })
}
fn validate_db(value: &Value) -> Result<()> {
    let map = value
        .as_object()
        .context("Python database must be a JSON object")?;
    ensure!(
        map.values().all(Value::is_object),
        "Python database namespaces must be objects"
    );
    Ok(())
}
fn urls(value: &Value) -> Result<BTreeMap<String, String>> {
    let map = value
        .as_object()
        .context("module URL list must be an object")?;
    ensure!(map.len() <= MAX_ENTRIES, "too many module URLs");
    let mut out = BTreeMap::new();
    for (name, value) in map {
        ensure!(
            !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "invalid Python module class name"
        );
        let raw = value.as_str().context("module URL must be a string")?;
        ensure!(raw.len() <= 8192, "module URL too long");
        let url = reqwest::Url::parse(raw)?;
        ensure!(
            matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
            "module origin is not an HTTP(S) URL"
        );
        out.insert(name.clone(), raw.into());
    }
    Ok(out)
}
fn safe_name(name: &str) -> Result<()> {
    // Original backups write flat member names. Reject rather than normalize ambiguity.
    ensure!(
        !name.is_empty()
            && name.len() <= 200
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')),
        "unsupported archive member name"
    );
    ensure!(
        !name.starts_with('.') && !name.contains(".."),
        "ambiguous archive member name"
    );
    // Names become review text files but still must not select a Windows device.
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()),
        "reserved archive filename"
    );
    Ok(())
}
fn archive(
    bytes: &[u8],
    owner: i64,
    depth: usize,
    budget: &mut Budget,
    parts: &mut Parts,
) -> Result<()> {
    ensure!(
        depth <= 1,
        "only the original outer backup → mods.zip nesting is supported"
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("invalid ZIP backup")?;
    ensure!(
        zip.len() <= MAX_ENTRIES && budget.entries + zip.len() <= MAX_ENTRIES,
        "legacy backup exceeds 256 archive entries"
    );
    budget.entries += zip.len();
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        let mut file = zip
            .by_index(index)
            .context("unreadable/encrypted ZIP member")?;
        let name = file.name().to_owned();
        safe_name(&name)?;
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "duplicate or case-colliding ZIP member"
        );
        ensure!(
            !file.is_dir()
                && file
                    .unix_mode()
                    .is_none_or(|m| m & 0o170000 == 0 || m & 0o170000 == 0o100000),
            "archive member must be a regular file"
        );
        let cap = if name.ends_with(".py") {
            MAX_SOURCE
        } else if name.starts_with("pip-backup-") && name.ends_with(".txt") {
            256 * 1024
        } else {
            MAX_MEMBER
        };
        ensure!(
            file.size() <= cap
                && budget
                    .expanded
                    .checked_add(file.size())
                    .is_some_and(|n| n <= MAX_EXPANDED),
            "legacy backup expansion limit exceeded"
        );
        budget.expanded += file.size();
        let mut body = Vec::new();
        (&mut file)
            .take(cap + 1)
            .read_to_end(&mut body)
            .context("invalid ZIP data/CRC")?;
        ensure!(body.len() as u64 == file.size(), "ZIP member size mismatch");
        match name.as_str() {
            "db.json" if depth == 0 => {
                ensure!(parts.db.is_none(), "duplicate database");
                let db: Value = serde_json::from_slice(&body)?;
                validate_db(&db)?;
                parts.db = Some(db);
            }
            "mods.zip" if depth == 0 => archive(&body, owner, depth + 1, budget, parts)?,
            "db_mods.json" => {
                let incoming = urls(&serde_json::from_slice::<Value>(&body)?)?;
                for (name, url) in incoming {
                    ensure!(
                        parts.urls.insert(name, url).is_none(),
                        "duplicate module URL definition"
                    );
                }
            }
            _ if name.ends_with(".py") => {
                ensure!(
                    parts.stored_names.insert(name.to_ascii_lowercase()),
                    "case-colliding Python source across archive levels"
                );
                ensure!(
                    name.ends_with(&format!("_{owner}.py")),
                    "Python module filename belongs to a different owner or has no account suffix"
                );
                std::str::from_utf8(&body).context("Python module source must be UTF-8")?;
                ensure!(
                    parts.sources.insert(name, body).is_none(),
                    "duplicate Python source"
                );
            }
            _ if name.starts_with("pip-backup-") && name.ends_with(".txt") => {
                ensure!(
                    parts.stored_names.insert(name.to_ascii_lowercase()),
                    "case-colliding requirements list across archive levels"
                );
                ensure!(body.len() <= 256 * 1024, "requirements list too large");
                std::str::from_utf8(&body).context("requirements list must be UTF-8")?;
                ensure!(
                    parts.requirements.insert(name, body).is_none(),
                    "duplicate requirements list"
                );
            }
            _ => bail!(
                "unsupported legacy backup member; expected db.json, mods.zip, db_mods.json, account module or pip-backup text"
            ),
        }
    }
    Ok(())
}

pub fn stage(root: &Path, bytes: &[u8], owner: i64) -> Result<Staged> {
    let imported = parse(bytes, owner)?; // Validate the entire input before creating staging files.
    let has_database = imported.backup.kind != Kind::Modules;
    let hash = crate::modules::digest(bytes);
    let report = Report {
        format: "atarustia-python-backup-review".into(), version: 1, owner, archive_sha256: hash.clone(), has_database,
        python_sources: imported.sources.iter().map(|(name,body)| QuarantinedFile { original_name:name.clone(), stored_name:format!("{name}.txt"), sha256:crate::modules::digest(body), bytes:body.len() }).collect(),
        requirements: imported.requirements.iter().map(|(name,body)| QuarantinedFile { original_name:name.clone(), stored_name:format!("{name}.review.txt"), sha256:crate::modules::digest(body), bytes:body.len() }).collect(),
        module_urls: imported.module_urls,
        warnings: vec!["Python code is preserved as .py.txt, not installed or executed".into(), "Requirements are review-only; no pip/uv/system package installation occurs".into(), "Owner identity is explicitly supplied; legacy backups do not reliably encode it".into(), "Only supported preferences are converted; old security grants, secrets and arbitrary module data are not activated".into()],
    };
    let root = root.canonicalize()?;
    let mut parent = root.clone();
    for component in ["legacy_imports".to_owned(), owner.to_string()] {
        parent.push(component);
        match std::fs::symlink_metadata(&parent) {
            Ok(meta) => ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "legacy staging directory is not a regular directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&parent)?
            }
            Err(error) => return Err(error.into()),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let target = parent.join(&hash);
    match std::fs::symlink_metadata(&target) {
        Ok(_) => bail!("this legacy backup is already staged or its destination is occupied"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let temp = tempfile::Builder::new()
        .prefix(".review-")
        .tempdir_in(&parent)?;
    if has_database {
        atomic_write(
            &temp.path().join("converted.backup.json"),
            &serde_json::to_vec_pretty(&imported.backup)?,
        )?;
    }
    for (name, body) in imported.sources {
        atomic_write(&temp.path().join(format!("{name}.txt")), &body)?;
    }
    for (name, body) in imported.requirements {
        atomic_write(&temp.path().join(format!("{name}.review.txt")), &body)?;
    }
    atomic_write(
        &temp.path().join("report.json"),
        &serde_json::to_vec_pretty(&report)?,
    )?;
    std::fs::rename(temp.path(), &target).context("publish legacy backup review")?;
    Ok(Staged {
        directory: target,
        report,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    #[test]
    fn actual_python_zip_fixture() {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(include_str!("../tests/fixtures/python_backup.base64").trim())
            .unwrap();
        let imported = parse(&bytes, 123).unwrap();
        assert_eq!(
            imported.backup.database["atarustia.main"]["command_prefix"],
            "!!"
        );
        assert_eq!(imported.backup.database["Settings"]["aliases"]["p"], "ping");
        assert!(imported.backup.database.get("atarustia.inline").is_none());
        assert!(imported.backup.database.get("atarustia.security").is_none());
        assert_eq!(imported.sources.len(), 1);
        assert_eq!(imported.requirements.len(), 1);
        assert_eq!(
            imported.module_urls["WeatherMod"],
            "https://example.com/weather.py"
        );
        assert!(parse(&bytes, 999).is_err());
    }
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in entries {
            w.start_file(
                *name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            w.write_all(body).unwrap();
        }
        w.finish().unwrap().into_inner()
    }
    #[test]
    fn original_nested_shape_is_converted_not_executed() {
        let inner = zip(&[
            ("DemoMod_123.py", b"raise RuntimeError('never run')"),
            (
                "db_mods.json",
                br#"{"DemoMod":"https://example.com/demo.py"}"#,
            ),
        ]);
        let outer = zip(&[
            (
                "db.json",
                br#"{"atarustia.main":{"command_prefix":"!!"},"atarustia.security":{"owner":[999]}}"#,
            ),
            ("mods.zip", &inner),
            (
                "pip-backup-test.txt",
                b"--index-url https://invalid.example\nanything",
            ),
        ]);
        let parsed = parse(&outer, 123).unwrap();
        assert_eq!(parsed.sources.len(), 1);
        assert_eq!(
            parsed.backup.database["atarustia.main"]["command_prefix"],
            "!!"
        );
        assert!(parsed.backup.database.get("atarustia.security").is_none());
        let dir = tempfile::tempdir().unwrap();
        let staged = stage(dir.path(), &outer, 123).unwrap();
        assert!(staged.directory.join("DemoMod_123.py.txt").is_file());
        assert!(!staged.directory.join("DemoMod_123.py").exists());
        assert!(staged.directory.join("converted.backup.json").is_file());
        assert!(stage(dir.path(), &outer, 123).is_err());
    }
    #[test]
    fn paths_case_collisions_and_other_accounts_are_rejected() {
        for name in [
            "../outside.py",
            "C:\\outside.py",
            "sub/Demo_123.py",
            ".hidden",
            "Demo_456.py",
        ] {
            assert!(parse(&zip(&[(name, b"x")]), 123).is_err());
        }
        assert!(parse(&zip(&[("Demo_123.py", b"a"), ("demo_123.py", b"b")]), 123).is_err());
    }
    #[test]
    fn bounded_member_and_depth() {
        let huge = vec![b'x'; MAX_SOURCE as usize + 1];
        assert!(parse(&zip(&[("Demo_123.py", &huge)]), 123).is_err());
        let nested = zip(&[("mods.zip", &zip(&[("Demo_123.py", b"x")]))]);
        assert!(parse(&zip(&[("mods.zip", &nested)]), 123).is_err());
    }
    #[test]
    fn windows_device_names_and_oversized_json_are_rejected() {
        assert!(safe_name("CON.py").is_err());
        assert!(safe_name("LPT1.txt").is_err());
        assert!(parse(&vec![b' '; MAX_INPUT + 1], 123).is_err());
    }
    #[test]
    fn standalone_database_and_urls() {
        let db = parse(br#"{"atarustia.translations":{"lang":"ru"}}"#, 123).unwrap();
        assert_eq!(db.backup.database["atarustia.translations"]["lang"], "ru");
        let urls = parse(br#"{"DemoMod":"https://example.com/demo.py"}"#, 123).unwrap();
        assert_eq!(urls.backup.kind, Kind::Modules);
        assert!(parse(br#"{"format":"atarustia-rust"}"#, 123).is_err());
    }
    #[test]
    fn invalid_input_does_not_create_review_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(stage(dir.path(), &zip(&[("unexpected.bin", b"bad")]), 123).is_err());
        assert!(!dir.path().join("legacy_imports").exists());
    }
    #[test]
    fn case_collision_across_nested_levels_is_rejected() {
        let inner = zip(&[("demo_123.py", b"inner")]);
        let outer = zip(&[("Demo_123.py", b"outer"), ("mods.zip", &inner)]);
        assert!(parse(&outer, 123).is_err());
    }
    #[test]
    fn requirements_only_is_not_a_restorable_backup() {
        assert!(parse(&zip(&[("pip-backup-only.txt", b"anything")]), 123).is_err());
    }
}
