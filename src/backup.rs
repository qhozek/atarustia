use crate::{
    database::Database,
    modules::{Package, Registry},
    storage::atomic_write,
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Backup {
    pub format: String,
    pub version: u32,
    pub owner: i64,
    pub created: u64,
    pub database: serde_json::Value,
    pub modules: Vec<Package>,
    #[serde(default)]
    pub kind: Kind,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    All,
    Database,
    Modules,
}
pub fn create(db: &Database, modules: &Registry, owner: i64) -> Result<Vec<u8>> {
    create_kind(db, modules, owner, Kind::All)
}
pub fn create_kind(db: &Database, modules: &Registry, owner: i64, kind: Kind) -> Result<Vec<u8>> {
    let mut database = db.snapshot()?;
    // Bot token is never included; native credentials and sessions are separate too.
    if let Some(inline) = database
        .get_mut("atarustia.inline")
        .and_then(serde_json::Value::as_object_mut)
    {
        inline.remove("bot_token");
    }
    Ok(serde_json::to_vec_pretty(&Backup {
        format: "atarustia-rust".into(),
        version: 1,
        owner,
        created: crate::security::now(),
        database: if kind == Kind::Modules {
            serde_json::json!({})
        } else {
            database
        },
        modules: if kind == Kind::Database {
            vec![]
        } else {
            modules.export()
        },
        kind,
    })?)
}
pub fn parse(bytes: &[u8], owner: i64) -> Result<Backup> {
    ensure!(bytes.len() <= 16 * 1024 * 1024, "backup exceeds 16 MiB");
    let b: Backup = serde_json::from_slice(bytes)?;
    ensure!(
        b.format == "atarustia-rust" && b.version == 1,
        "unsupported backup format"
    );
    ensure!(b.owner == owner, "backup belongs to another account");
    ensure!(b.modules.len() <= 256, "too many modules in backup");
    ensure!(
        b.database.is_object()
            && b.database
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_object),
        "invalid database"
    );
    let mut module_names = std::collections::HashSet::new();
    let mut commands = std::collections::HashSet::new();
    for p in &b.modules {
        p.validate()?;
        ensure!(
            module_names.insert(&p.manifest.name),
            "duplicate module in backup"
        );
        for command in &p.manifest.commands {
            ensure!(
                commands.insert(&command.name),
                "duplicate command in backup"
            );
        }
    }
    Ok(b)
}
pub fn local(root: &Path, bytes: &[u8], owner: i64) -> Result<std::path::PathBuf> {
    let path = root.join("backups").join(format!(
        "{owner}-{}-{:08x}.json",
        crate::security::now(),
        rand::random::<u32>()
    ));
    atomic_write(&path, bytes)?;
    let mut copies: Vec<_> = std::fs::read_dir(root.join("backups"))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{owner}-"))
                && e.path().extension().is_some_and(|s| s == "json")
        })
        .collect();
    copies.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    let excess = copies.len().saturating_sub(20);
    for entry in copies.into_iter().take(excess) {
        std::fs::remove_file(entry.path())?;
    }
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excludes_token_and_validates_owner() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let r = Registry::load(d.path(), 1).unwrap();
        db.set("atarustia.inline", "bot_token", "secret-token")
            .unwrap();
        let bytes = create(&db, &r, 1).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("secret-token"));
        assert!(parse(&bytes, 1).is_ok());
        assert!(parse(&bytes, 2).is_err());
    }
    #[test]
    fn section_types_and_retention() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        db.set("mod", "key", 42).unwrap();
        let r = Registry::load(d.path(), 1).unwrap();
        let bytes = create_kind(&db, &r, 1, Kind::Modules).unwrap();
        let b = parse(&bytes, 1).unwrap();
        assert!(b.database.as_object().unwrap().is_empty());
        let bytes = create_kind(&db, &r, 1, Kind::Database).unwrap();
        assert!(parse(&bytes, 1).unwrap().modules.is_empty());
        for _ in 0..25 {
            local(d.path(), &bytes, 1).unwrap();
        }
        assert_eq!(
            std::fs::read_dir(d.path().join("backups")).unwrap().count(),
            20
        );
    }
    #[test]
    fn corrupt_and_duplicate_packages_rejected() {
        assert!(parse(b"not json", 1).is_err());
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let r = Registry::load(d.path(), 1).unwrap();
        let mut b: serde_json::Value =
            serde_json::from_slice(&create(&db, &r, 1).unwrap()).unwrap();
        let package: serde_json::Value =
            serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
        b["modules"] = serde_json::json!([package.clone(), package]);
        assert!(parse(&serde_json::to_vec(&b).unwrap(), 1).is_err());
    }
}
