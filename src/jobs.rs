//! Cancellation ownership for native executor commands, keyed by their Telegram message.
use std::{collections::HashMap, sync::Mutex};
use tokio_util::sync::CancellationToken;
#[derive(Default)]
pub struct Jobs(Mutex<HashMap<(i64, i32), CancellationToken>>);
impl Jobs {
    pub fn insert(&self, chat: i64, id: i32, stop: CancellationToken) -> bool {
        let mut jobs = self.0.lock().unwrap();
        if jobs.contains_key(&(chat, id)) {
            return false;
        }
        jobs.insert((chat, id), stop);
        true
    }
    pub fn remove(&self, chat: i64, id: i32) {
        self.0.lock().unwrap().remove(&(chat, id));
    }
    pub fn cancel(&self, chat: i64, id: i32) -> bool {
        if let Some(token) = self.0.lock().unwrap().get(&(chat, id)) {
            token.cancel();
            true
        } else {
            false
        }
    }
    pub fn cancel_all(&self) {
        for token in self.0.lock().unwrap().values() {
            token.cancel();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_identity_and_cancel() {
        let j = Jobs::default();
        let token = CancellationToken::new();
        assert!(j.insert(1, 2, token.clone()));
        assert!(!j.cancel(2, 2));
        assert!(j.cancel(1, 2));
        assert!(token.is_cancelled());
        j.remove(1, 2);
        assert!(!j.cancel(1, 2));
    }
}
