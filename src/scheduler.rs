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
            if db
                .get("RustModuleLifecycle", &p.manifest.name)
                .get("status")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|s| s != "active")
            {
                continue;
            }
            for task in &p.manifest.loops {
                let key = format!("{}:{}:{}", p.manifest.name, p.revision(), task.function);
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
                if let Some(clause) = &task.stop_clause {
                    // The clause lives inside module state, so the module itself
                    // can stop its loop by returning `state: {clause: false}`
                    let mut state = db.get(&p.manifest.name, "state");
                    match state.get(clause).and_then(serde_json::Value::as_bool) {
                        Some(false) => {
                            self.next.remove(&key);
                            continue;
                        }
                        Some(true) => {}
                        None => {
                            if !state.is_object() {
                                state = serde_json::json!({});
                            }
                            state[clause.as_str()] = serde_json::json!(true);
                            let _ = db.set(&p.manifest.name, "state", state);
                        }
                    }
                }
                let interval = Duration::from_secs(task.interval_seconds);
                let start = if task.wait_before {
                    interval
                } else {
                    Duration::ZERO
                };
                let next = self.next.entry(key).or_insert(now + start);
                if *next <= now {
                    *next = now + interval;
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
            wait_before: true,
            stop_clause: None,
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
    fn package_with_loop(wait_before: bool, stop_clause: Option<&str>) -> Package {
        let mut p: Package =
            serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
        p.manifest.loops.push(Loop {
            function: "tick".into(),
            interval_seconds: 60,
            autostart: true,
            wait_before,
            stop_clause: stop_clause.map(str::to_owned),
        });
        p
    }
    fn set_clause(db: &Database, clause: &str, value: bool) {
        let mut state = db.get("hello", "state");
        if !state.is_object() {
            state = serde_json::json!({});
        }
        state[clause] = serde_json::json!(value);
        db.set("hello", "state", state).unwrap();
    }
    #[test]
    fn loop_without_wait_before_runs_immediately() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let p = package_with_loop(false, None);
        let mut s = Scheduler::default();
        assert_eq!(s.due(&[p], &db).len(), 1);
    }
    #[test]
    fn stop_clause_marks_start_and_stops_when_reset() {
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        let p = package_with_loop(false, Some("alive"));
        let mut s = Scheduler::default();
        assert_eq!(s.due(std::slice::from_ref(&p), &db).len(), 1);
        // First schedule marked the clause inside module state
        assert_eq!(db.get("hello", "state")["alive"], serde_json::json!(true));
        // The module stopping itself parks the loop: nothing due, nothing scheduled
        set_clause(&db, "alive", false);
        assert!(s.due(std::slice::from_ref(&p), &db).is_empty());
        assert!(s.next.is_empty());
        // Setting it back resumes the loop; without wait_before it starts at once
        set_clause(&db, "alive", true);
        assert_eq!(s.due(&[p], &db).len(), 1);
        assert_eq!(s.next.len(), 1);
    }
}
