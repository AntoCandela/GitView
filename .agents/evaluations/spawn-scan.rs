// Baseline assumes a spawned task automatically inherits task-local correlation.
use std::future::Future;
use crate::diagnostics::OperationContext;

fn spawn_scan<T: Send + 'static>(
    _parent: &OperationContext,
    work: impl Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    tokio::spawn(work)
}
