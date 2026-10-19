//! Async per-account circuit breaker. Waiting never blocks Tokio or other accounts.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::Mutex,
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub enabled: bool,
    pub window_seconds: u64,
    pub threshold: usize,
    pub pause_seconds: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            enabled: false,
            window_seconds: 15,
            threshold: 100,
            pause_seconds: 30,
        }
    }
}
#[derive(Default)]
pub struct Limiter(Mutex<State>);
#[derive(Default)]
struct State {
    calls: VecDeque<Instant>,
    until: Option<Instant>,
    suspended: Option<Instant>,
}
impl Limiter {
    pub fn suspend(&self, seconds: u64) {
        self.0.lock().unwrap().suspended = Some(Instant::now() + Duration::from_secs(seconds));
    }
    pub fn delay(&self, limits: &Limits) -> Duration {
        if !limits.enabled {
            return Duration::ZERO;
        }
        let now = Instant::now();
        let mut state = self.0.lock().unwrap();
        if state.suspended.is_some_and(|s| s > now) {
            return Duration::ZERO;
        }
        if let Some(until) = state.until.filter(|u| *u > now) {
            return until.duration_since(now);
        }
        while state.calls.front().is_some_and(|t| {
            now.duration_since(*t) >= Duration::from_secs(limits.window_seconds.max(1))
        }) {
            state.calls.pop_front();
        }
        state.calls.push_back(now);
        if state.calls.len() > limits.threshold.clamp(1, 4096) {
            let pause = Duration::from_secs(limits.pause_seconds.clamp(1, 3600));
            state.until = Some(now + pause);
            state.calls.clear();
            pause
        } else {
            Duration::ZERO
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn breaker_and_suspension() {
        let l = Limiter::default();
        let limits = Limits {
            enabled: true,
            threshold: 2,
            ..Default::default()
        };
        assert!(l.delay(&limits).is_zero());
        assert!(l.delay(&limits).is_zero());
        assert!(!l.delay(&limits).is_zero());
        l.suspend(60);
        assert!(l.delay(&limits).is_zero());
    }
}
