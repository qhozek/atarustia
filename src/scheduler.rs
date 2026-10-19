//! Per-module loop scheduler; identity includes the pinned source revision.
use crate::{
    database::Database,
    modules::{Loop, Package},
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct Scheduler {
    next: HashMap<String, Instant>,
}
impl Scheduler {
    pub fn due(&mut self, packages: &[Package], db: &Database) -> Vec<(Package, Loop)> {
        let now = Instant::now();
        let mut active = std::collections::HashSet::new();
        let mut result = Vec::new();
        for p in packages {
            for task in &p.manifest.loops {
                let key = format!(
                    "{}:{}:{}",
                    p.manifest.name,
                    crate::modules::digest(p.source.as_bytes()),
                    task.function
                );
                active.insert(key.clone());
                let enabled = db
                    .get(&p.manifest.name, "loops")
                    .get(&task.function)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(task.autostart);
                if !enabled {
                    self.next.remove(&key);
                    continue;
                }
                let next = self
                    .next
                    .entry(key)
                    .or_insert(now + Duration::from_secs(task.interval_seconds));
                if *next <= now {
                    *next = now + Duration::from_secs(task.interval_seconds);
                    result.push((p.clone(), task.clone()));
                }
            }
        }
        self.next.retain(|k, _| active.contains(k));
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_loop_can_start_again() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let mut p: Package =
            serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
        p.manifest.loops.push(Loop {
            function: "tick".into(),
            interval_seconds: 1,
            autostart: true,
        });
        let mut s = Scheduler::default();
        assert!(s.due(&[p.clone()], &db).is_empty());
        let key = s.next.keys().next().unwrap().clone();
        s.next
            .insert(key.clone(), Instant::now() - Duration::from_secs(1));
        assert_eq!(s.due(&[p.clone()], &db).len(), 1);
        db.set("hello", "loops", serde_json::json!({"tick":false}))
            .unwrap();
        s.due(&[p.clone()], &db);
        assert!(s.next.is_empty());
        db.set("hello", "loops", serde_json::json!({"tick":true}))
            .unwrap();
        s.due(&[p], &db);
        assert_eq!(s.next.len(), 1);
    }
}
