//! Bounded async work admission. No spawn-per-event waiters and no silent try-acquire drops.
use std::{
    collections::HashSet,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

type Work = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Queued,
    Coalesced,
    Stopped,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub accepted: usize,
    pub completed: usize,
    pub cancelled: usize,
    pub queued: usize,
    pub active: usize,
    pub coalesced: usize,
    pub backpressure: usize,
}
#[derive(Default)]
struct Counters {
    accepted: AtomicUsize,
    completed: AtomicUsize,
    cancelled: AtomicUsize,
    queued: AtomicUsize,
    active: AtomicUsize,
    coalesced: AtomicUsize,
    backpressure: AtomicUsize,
}
#[derive(Clone, Default)]
pub struct Metrics(Arc<Counters>);
impl Metrics {
    pub fn snapshot(&self) -> Snapshot {
        let c = &self.0;
        Snapshot {
            accepted: c.accepted.load(Ordering::Relaxed),
            completed: c.completed.load(Ordering::Relaxed),
            cancelled: c.cancelled.load(Ordering::Relaxed),
            queued: c.queued.load(Ordering::Relaxed),
            active: c.active.load(Ordering::Relaxed),
            coalesced: c.coalesced.load(Ordering::Relaxed),
            backpressure: c.backpressure.load(Ordering::Relaxed),
        }
    }
}
struct Guard {
    metrics: Metrics,
    keys: Arc<Mutex<HashSet<String>>>,
    key: Option<String>,
    started: bool,
    completed: bool,
}
impl Guard {
    fn start(&mut self) {
        self.started = true;
        self.metrics.0.queued.fetch_sub(1, Ordering::Relaxed);
        self.metrics.0.active.fetch_add(1, Ordering::Relaxed);
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            self.keys.lock().unwrap().remove(key);
        }
        if self.started {
            self.metrics.0.active.fetch_sub(1, Ordering::Relaxed);
        } else {
            self.metrics.0.queued.fetch_sub(1, Ordering::Relaxed);
        }
        if self.completed {
            self.metrics.0.completed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.metrics.0.cancelled.fetch_add(1, Ordering::Relaxed);
        }
    }
}
struct Job {
    work: Work,
    guard: Guard,
}

pub struct WorkQueue {
    name: &'static str,
    sender: mpsc::Sender<Job>,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
    metrics: Metrics,
    keys: Arc<Mutex<HashSet<String>>>,
}
impl WorkQueue {
    pub fn new(
        name: &'static str,
        workers: usize,
        capacity: usize,
        stop: &CancellationToken,
    ) -> Self {
        assert!(workers > 0 && capacity > 0);
        let (sender, mut receiver) = mpsc::channel::<Job>(capacity);
        let stop = stop.child_token();
        let runner_stop = stop.clone();
        let metrics = Metrics::default();
        let runner_metrics = metrics.clone();
        let task = tokio::spawn(async move {
            let mut running = JoinSet::new();
            loop {
                tokio::select! {
                    biased;
                    _ = runner_stop.cancelled() => break,
                    Some(result) = running.join_next(), if !running.is_empty() => {
                        if result.is_err() { tracing::error!(queue=name,"queued handler panicked or was cancelled"); }
                    }
                    job = receiver.recv(), if running.len() < workers => {
                        let Some(Job {work, mut guard}) = job else { break; };
                        running.spawn(async move { guard.start(); work.await; guard.completed = true; drop(guard); });
                    }
                }
            }
            receiver.close();
            running.abort_all();
            while running.join_next().await.is_some() {}
            while receiver.try_recv().is_ok() {}
            let status = runner_metrics.snapshot();
            tracing::info!(
                queue = name,
                accepted = status.accepted,
                completed = status.completed,
                cancelled = status.cancelled,
                coalesced = status.coalesced,
                backpressure = status.backpressure,
                "work queue stopped; accepted unfinished work was explicitly cancelled, not replayed"
            );
        });
        Self {
            name,
            sender,
            stop,
            task: Some(task),
            metrics,
            keys: Default::default(),
        }
    }
    pub fn metrics(&self) -> Metrics {
        self.metrics.clone()
    }
    fn job(&self, work: Work, key: Option<String>) -> Job {
        self.metrics.0.accepted.fetch_add(1, Ordering::Relaxed);
        self.metrics.0.queued.fetch_add(1, Ordering::Relaxed);
        Job {
            work,
            guard: Guard {
                metrics: self.metrics.clone(),
                keys: self.keys.clone(),
                key,
                started: false,
                completed: false,
            },
        }
    }
    /// Backpressure is awaited in the producer itself, never an unbounded spawned waiter.
    pub async fn submit(&self, work: impl Future<Output = ()> + Send + 'static) -> Admission {
        self.submit_key(None, Box::pin(work)).await
    }
    /// A periodic tick already queued/running is coalesced, not scheduled twice.
    pub async fn submit_unique(
        &self,
        key: String,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> Admission {
        self.submit_key(Some(key), Box::pin(work)).await
    }
    async fn submit_key(&self, key: Option<String>, work: Work) -> Admission {
        if self.stop.is_cancelled() {
            return Admission::Stopped;
        }
        if key
            .as_ref()
            .is_some_and(|key| self.keys.lock().unwrap().contains(key))
        {
            self.metrics.0.coalesced.fetch_add(1, Ordering::Relaxed);
            return Admission::Coalesced;
        }
        if self.sender.capacity() == 0 {
            self.metrics.0.backpressure.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(queue = self.name, "work queue full; waiting for capacity");
        }
        let permit = tokio::select! {
            biased;
            _ = self.stop.cancelled() => return Admission::Stopped,
            permit = self.sender.reserve() => match permit {Ok(permit)=>permit,Err(_)=>return Admission::Stopped},
        };
        if self.stop.is_cancelled() {
            return Admission::Stopped;
        }
        if let Some(key) = &key
            && !self.keys.lock().unwrap().insert(key.clone())
        {
            self.metrics.0.coalesced.fetch_add(1, Ordering::Relaxed);
            return Admission::Coalesced;
        }
        permit.send(self.job(work, key));
        Admission::Queued
    }
    /// The inline demultiplexer must keep reading control updates. Overflow is a
    /// visible stream failure, never a dropped event or a bypass of authorization.
    pub fn try_submit(
        &self,
        work: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<Admission> {
        if self.stop.is_cancelled() {
            return Ok(Admission::Stopped);
        }
        let permit = self.sender.try_reserve().map_err(|_| {
            tracing::error!(
                queue = self.name,
                "inline work queue exhausted; stopping stream rather than dropping an event"
            );
            anyhow::anyhow!(
                "{} queue exhausted; event not admitted; stream stopped, no automatic replay",
                self.name
            )
        })?;
        permit.send(self.job(Box::pin(work), None));
        Ok(Admission::Queued)
    }
    pub async fn shutdown(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take()
            && task.await.is_err()
        {
            tracing::error!(
                queue = self.name,
                "work queue runner failed during shutdown"
            );
        }
    }
}
impl Drop for WorkQueue {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::{Notify, Semaphore};

    async fn until(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("queue condition timed out");
    }

    #[tokio::test]
    async fn burst_over_32_executes_every_job_once_with_bounded_workers_and_backpressure() {
        let stop = CancellationToken::new();
        let mut queue = WorkQueue::new("test-burst", 4, 8, &stop);
        let metrics = queue.metrics();
        let hits = Arc::new(Mutex::new(vec![0usize; 160]));
        let high = Arc::new(AtomicUsize::new(0));
        for id in 0..160 {
            let hits = hits.clone();
            let high = high.clone();
            let metrics = metrics.clone();
            assert_eq!(
                queue
                    .submit(async move {
                        high.fetch_max(metrics.snapshot().active, Ordering::Relaxed);
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        hits.lock().unwrap()[id] += 1;
                    })
                    .await,
                Admission::Queued
            );
        }
        until(|| metrics.snapshot().completed == 160).await;
        assert!(hits.lock().unwrap().iter().all(|n| *n == 1));
        assert!(high.load(Ordering::Relaxed) <= 4);
        assert!(metrics.snapshot().backpressure > 0);
        queue.shutdown().await;
        let status = metrics.snapshot();
        assert_eq!(status.accepted, status.completed);
        assert_eq!(status.cancelled, 0);
        assert_eq!(status.active + status.queued, 0);
    }

    #[tokio::test]
    async fn shutdown_releases_blocked_producer_and_accounts_for_queued_and_active_work() {
        let stop = CancellationToken::new();
        let mut queue = WorkQueue::new("test-stop", 1, 1, &stop);
        let metrics = queue.metrics();
        queue.submit(std::future::pending()).await;
        until(|| metrics.snapshot().active == 1).await;
        queue.submit(std::future::pending()).await;
        let cancel = stop.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel.cancel();
        });
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), queue.submit(async {}))
                .await
                .unwrap(),
            Admission::Stopped
        );
        queue.shutdown().await;
        let status = metrics.snapshot();
        assert_eq!(status.accepted, 2);
        assert_eq!(status.cancelled, 2);
        assert_eq!(status.queued + status.active, 0);
    }

    #[tokio::test]
    async fn reserved_delivery_completes_when_all_regular_workers_wait_for_forms() {
        let stop = CancellationToken::new();
        let mut regular = WorkQueue::new("test-regular", 32, 64, &stop);
        let mut control = WorkQueue::new("test-delivery", 2, 4, &stop);
        let regular_metrics = regular.metrics();
        let gate = Arc::new(Semaphore::new(0));
        for _ in 0..32 {
            let gate = gate.clone();
            regular
                .submit(async move {
                    gate.acquire().await.unwrap().forget();
                })
                .await;
        }
        until(|| regular_metrics.snapshot().active == 32).await;
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..64 {
            let hit = done.clone();
            regular
                .submit(async move {
                    hit.fetch_add(1, Ordering::Relaxed);
                })
                .await;
        }
        assert_eq!(regular_metrics.snapshot().queued, 64);
        let release = gate.clone();
        control
            .try_submit(async move {
                release.add_permits(32);
            })
            .unwrap();
        until(|| done.load(Ordering::Relaxed) == 64 && regular_metrics.snapshot().completed == 96)
            .await;
        regular.shutdown().await;
        control.shutdown().await;
    }

    #[tokio::test]
    async fn periodic_ticks_coalesce_and_resume_after_completion_or_panic() {
        let stop = CancellationToken::new();
        let mut queue = WorkQueue::new("test-periodic", 1, 4, &stop);
        let metrics = queue.metrics();
        let release = Arc::new(Notify::new());
        let waiter = release.clone();
        queue
            .submit_unique("loop:a".into(), async move {
                waiter.notified().await;
            })
            .await;
        assert_eq!(
            queue
                .submit_unique("loop:a".into(), async { panic!("must not run") })
                .await,
            Admission::Coalesced
        );
        release.notify_one();
        until(|| metrics.snapshot().completed == 1).await;
        assert_eq!(
            queue
                .submit_unique("loop:a".into(), async { panic!("synthetic handler panic") })
                .await,
            Admission::Queued
        );
        until(|| metrics.snapshot().cancelled == 1).await;
        assert_eq!(
            queue.submit_unique("loop:a".into(), async {}).await,
            Admission::Queued
        );
        until(|| metrics.snapshot().completed == 2).await;
        queue.shutdown().await;
    }

    #[tokio::test]
    async fn inline_overflow_is_an_error_not_a_silent_drop_or_unbounded_waiter() {
        let stop = CancellationToken::new();
        let mut queue = WorkQueue::new("test-overflow", 1, 1, &stop);
        let metrics = queue.metrics();
        queue.try_submit(std::future::pending()).unwrap();
        until(|| metrics.snapshot().active == 1).await;
        queue.try_submit(std::future::pending()).unwrap();
        assert!(
            queue
                .try_submit(async {})
                .unwrap_err()
                .to_string()
                .contains("queue exhausted")
        );
        assert_eq!(metrics.snapshot().accepted, 2);
        assert_eq!(metrics.snapshot().queued, 1);
        queue.shutdown().await;
        assert_eq!(metrics.snapshot().cancelled, 2);
    }
}
