//! Owns native request cancellation and resources that must finish before runtime exit.
//!
//! Task-local ownership follows explicitly inherited background jobs, including default
//! Git adapters. Resource permits outlive cancelled callers until reaping or blocking I/O ends.

use std::future::Future;
use std::sync::Arc;
use parking_lot::Mutex;
use tokio::sync::Notify;

pub(crate) const SHUTTING_DOWN: &str = "Application is shutting down.";

tokio::task_local! {
    static CURRENT: NativeWork;
}

#[derive(Default)]
struct WorkState {
    closing: bool,
    active: usize,
}

#[derive(Default)]
struct SharedWork {
    state: Mutex<WorkState>,
    cancelled: Notify,
    idle: Notify,
}

#[derive(Clone, Default)]
pub(crate) struct NativeWork(Arc<SharedWork>);

pub(crate) struct WorkPermit(NativeWork);

impl NativeWork {
    pub(crate) fn close(&self) {
        self.0.state.lock().closing = true;
        self.0.cancelled.notify_waiters();
    }

    pub(crate) fn is_closing(&self) -> bool { self.0.state.lock().closing }

    pub(crate) fn admit(&self) -> Result<WorkPermit, &'static str> {
        let mut state = self.0.state.lock();
        if state.closing { return Err(SHUTTING_DOWN); }
        state.active += 1;
        Ok(WorkPermit(self.clone()))
    }

    fn resource(&self) -> WorkPermit {
        // Already-admitted work can create cleanup resources after close; count them too.
        self.0.state.lock().active += 1;
        WorkPermit(self.clone())
    }

    pub(crate) fn scope<T>(&self, future: impl Future<Output = T>) -> impl Future<Output = T> {
        CURRENT.scope(self.clone(), future)
    }

    /// Rejects new requests and cancels existing ones without discarding their cleanup ownership.
    pub(crate) async fn run<T>(&self, future: impl Future<Output = T>) -> Result<T, &'static str> {
        self.admit()?.run(future).await
    }

    async fn cancelled(&self) {
        let cancelled = self.0.cancelled.notified();
        tokio::pin!(cancelled);
        cancelled.as_mut().enable();
        if !self.is_closing() { cancelled.await; }
    }

    /// Call only after closing admission, and never from a request owned by this lifetime.
    pub(crate) async fn drain(&self) {
        loop {
            let idle = self.0.idle.notified();
            tokio::pin!(idle);
            idle.as_mut().enable();
            if self.0.state.lock().active == 0 { return; }
            idle.await;
        }
    }
}

impl WorkPermit {
    pub(crate) fn run<'a, T: 'a>(
        &'a self, future: impl Future<Output = T> + 'a,
    ) -> impl Future<Output = Result<T, &'static str>> + 'a {
        self.0.scope(async {
            tokio::select! {
                biased;
                _ = self.0.cancelled() => Err(SHUTTING_DOWN),
                outcome = future => {
                    // Linearize completion against close, including closure during the final poll.
                    if self.0.is_closing() { Err(SHUTTING_DOWN) } else { Ok(outcome) }
                },
            }
        })
    }
}

impl Drop for WorkPermit {
    fn drop(&mut self) {
        let mut state = self.0.0.state.lock();
        state.active -= 1;
        if state.active == 0 { self.0.0.idle.notify_waiters(); }
    }
}

pub(crate) fn current_resource() -> Option<WorkPermit> {
    CURRENT.try_with(NativeWork::resource).ok()
}

/// Tokio does not inherit task locals. Capture ownership before spawning a background producer.
pub(crate) fn inherit(future: impl Future<Output = ()>) -> impl Future<Output = ()> {
    let owner = CURRENT.try_with(Clone::clone).ok();
    async move {
        match owner {
            Some(owner) => { let _ = owner.run(future).await; }
            None => future.await,
        }
    }
}

// Value first: an abandoned blocking result must finish destruction before releasing ownership.
struct BlockingOutput<T> {
    value: T,
    _work: Option<WorkPermit>,
}

/// Starts eagerly and retains cleanup ownership through destruction of an abandoned result.
pub(crate) fn spawn_blocking<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> impl Future<Output = Result<T, tokio::task::JoinError>> {
    let work = current_resource();
    let task = tokio::task::spawn_blocking(move || BlockingOutput { value: operation(), _work: work });
    async move { task.await.map(|output| output.value) }
}
