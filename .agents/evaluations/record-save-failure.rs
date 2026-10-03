// Baseline misclassifies an actual save failure as a completed interval.
use std::time::Instant;
use crate::diagnostics::{Component, DiagnosticDetails, Event, Level, OperationContext};

fn record_save_failure(started: Instant) {
    if let Some(operation) = OperationContext::current() {
        let _ = operation.record(
            Level::Info,
            Component::Persistence,
            Event::Completed,
            None,
            DiagnosticDetails {
                duration_ms: Some(started.elapsed().as_millis().min(i64::MAX as u128) as u64),
                ..Default::default()
            },
        );
    }
}
