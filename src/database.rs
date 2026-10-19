use crate::storage::atomic_write;
use anyhow::{Context, Result, ensure};
use serde_json::{Map, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Transactions are validated and durably written before replacing the visible state.
pub struct Database {
    path: PathBuf,
    data: Mutex<Map<String, Value>>,
}
impl Database {
    pub fn open(root: &Path, owner: i64) -> Result<Self> {
        let path = root.join(format!("config-{owner}.json"));
        let data = if path.exists() {
            serde_json::from_slice::<Map<String, Value>>(&std::fs::read(&path)?).with_context(
                || {
                    format!(
                        "corrupt {}; preserve and restore it before continuing",
                        path.display()
                    )
                },
            )?
        } else {
            Map::new()
        };
        Self::validate(&data)?;
        Ok(Self {
            path,
            data: Mutex::new(data),
        })
    }
    fn validate(data: &Map<String, Value>) -> Result<()> {
        ensure!(
            data.values().all(Value::is_object),
            "database namespaces must be JSON objects"
        );
        Ok(())
    }
    pub fn get(&self, owner: &str, key: &str) -> Value {
        self.data
            .lock()
            .expect("database poisoned")
            .get(owner)
            .and_then(|v| v.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    }
    pub fn set(&self, owner: &str, key: &str, value: impl Into<Value>) -> Result<()> {
        self.transaction(|data| {
            data.entry(owner)
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .context("invalid namespace")?
                .insert(key.into(), value.into());
            Ok(())
        })
    }
    pub fn transaction<T>(
        &self,
        f: impl FnOnce(&mut Map<String, Value>) -> Result<T>,
    ) -> Result<T> {
        let mut current = self
            .data
            .lock()
            .map_err(|_| anyhow::anyhow!("database poisoned"))?;
        let mut next = current.clone();
        let result = f(&mut next)?;
        Self::validate(&next)?;
        atomic_write(&self.path, &serde_json::to_vec_pretty(&next)?)?;
        *current = next;
        Ok(result)
    }
    pub fn snapshot(&self) -> Result<Value> {
        Ok(Value::Object(
            self.data
                .lock()
                .map_err(|_| anyhow::anyhow!("database poisoned"))?
                .clone(),
        ))
    }
    pub fn restore(&self, value: Value) -> Result<()> {
        let map = value
            .as_object()
            .context("backup must be a JSON object")?
            .clone();
        Self::validate(&map)?;
        self.transaction(|data| {
            *data = map;
            Ok(())
        })
    }
    pub fn prefix(&self) -> String {
        self.get("atarustia.main", "command_prefix")
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(".")
            .into()
    }
    pub fn prefix_for(&self, user: Option<i64>, primary: i64) -> String {
        if user == Some(primary) {
            return self.prefix();
        }
        user.and_then(|id| {
            self.get("atarustia.main", "command_prefixes")
                .get(id.to_string())
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| self.prefix())
    }
    pub fn language(&self) -> String {
        self.get("atarustia.translations", "lang")
            .as_str()
            .unwrap_or("ru")
            .into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rollback_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path(), 1).unwrap();
        db.set("m", "v", 42).unwrap();
        assert!(
            db.transaction(|m| {
                m.insert("bad".into(), Value::Bool(true));
                Ok(())
            })
            .is_err()
        );
        assert_eq!(Database::open(dir.path(), 1).unwrap().get("m", "v"), 42);
        assert!(db.restore(serde_json::json!([1, 2])).is_err());
    }
    #[test]
    fn failed_write_does_not_commit_memory() {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path(), 2).unwrap();
        std::fs::create_dir(dir.path().join("config-2.json")).unwrap();
        assert!(db.set("m", "v", 1).is_err());
        assert!(db.get("m", "v").is_null());
    }
    #[test]
    fn per_user_prefix_does_not_change_primary() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        db.set("atarustia.main", "command_prefix", "!!").unwrap();
        db.set(
            "atarustia.main",
            "command_prefixes",
            serde_json::json!({"2":"?"}),
        )
        .unwrap();
        assert_eq!(db.prefix_for(Some(1), 1), "!!");
        assert_eq!(db.prefix_for(Some(2), 1), "?");
        assert_eq!(db.prefix_for(Some(3), 1), "!!");
    }
}
