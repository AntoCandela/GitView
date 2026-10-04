//! Records scoped native lifecycles and classifies existing domain outcomes without payloads.

use std::{future::Future, str::FromStr, time::Instant};

use crate::diagnostics::{Code, Component, DiagnosticDetails, DiagnosticSink, Event, HealthState, Level, OperationContext, OperationKind};
use crate::browsing::{RepositoryFileResult, RepositoryFilesResult};
use crate::diff::committed::CommitReviewResult;
use crate::diff::{ReviewErrorCode, ReviewResult};
use crate::git::GitError;
use crate::history::{HistoryErrorCode, HistoryPageResult};
use crate::inspection::{CommitFilesResult, ContextOptionsResult};
use crate::observation::{ObservationErrorCode, ObservationSnapshot};
use crate::git::status::StatusError;
use crate::workspace::{MutationOutcome, OpenOutcome, SelectOutcome, WorkspaceSnapshot};
use crate::workspace::persistence::PersistenceErrorCode;

/// Reuses the caller's diagnostic identity; disabled capture does not allocate a new operation.
pub(crate) fn context(sink: &DiagnosticSink, kind: OperationKind) -> Option<OperationContext> {
    OperationContext::current().map(|current| current.with_kind(kind)).or_else(|| {
        (!matches!(sink.health().state, HealthState::Disabled | HealthState::Stopped))
            .then(|| OperationContext::new(sink.clone(), None, None).with_kind(kind))
    })
}

/// Establishes async poll-local correlation without requiring a writer in ephemeral callers.
pub(crate) async fn scoped<F: Future>(context: Option<&OperationContext>, future: F) -> F::Output {
    match context {
        Some(context) => context.scope(future).await,
        None => future.await,
    }
}

/// One component interval within an operation; repeated Git subprocesses share its identity.
pub(crate) struct OperationTrace {
    context: OperationContext,
    component: Component,
    started: Instant,
    finished: bool,
}

impl OperationTrace {
    pub(crate) fn new(context: OperationContext, component: Component) -> Self {
        context.record(Level::Info, component, Event::Started, None, DiagnosticDetails::default());
        Self { context, component, started: Instant::now(), finished: false }
    }

    pub(crate) fn finish(&mut self, event: Event, code: Option<Code>, mut details: DiagnosticDetails) {
        if self.finished { return; }
        self.finished = true;
        // The future completed even if capture drops its terminal row; health reports that loss.
        details.duration_ms = Some(self.started.elapsed().as_millis().min(i64::MAX as u128) as u64);
        self.context.record(if code.is_some() { Level::Error } else { Level::Info }, self.component, event, code, details);
    }

    pub(crate) fn outcome<T: DiagnosticOutcome>(&mut self, outcome: &T) {
        let (event, code) = outcome.diagnostic_outcome();
        self.finish(event, code, DiagnosticDetails::default());
    }
}

impl Drop for OperationTrace {
    fn drop(&mut self) {
        if !self.finished {
            // Interrupted work is not a domain failure or proof that child cleanup completed.
            self.context.record(Level::Warn, self.component, Event::Cancelled, None, DiagnosticDetails {
                duration_ms: Some(self.started.elapsed().as_millis().min(i64::MAX as u128) as u64),
                ..Default::default()
            });
        }
    }
}

pub(crate) trait DiagnosticOutcome {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>);
}

impl DiagnosticOutcome for OpenOutcome {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Cancelled { .. } => (Event::Cancelled, None),
            Self::Rejected { code, .. } => (Event::Failed, Some(Code::from_str(code.code()).unwrap_or(Code::RepositoryUnavailable))),
            _ => (Event::Completed, None),
        }
    }
}

impl DiagnosticOutcome for SelectOutcome {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Selected { .. } => (Event::Completed, None),
            Self::NotFound { .. } => (Event::Failed, Some(Code::RepositoryUnavailable)),
        }
    }
}

impl DiagnosticOutcome for MutationOutcome {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Updated { .. } => (Event::Completed, None),
            Self::NotFound { .. } => (Event::Failed, Some(Code::RepositoryUnavailable)),
            Self::Rejected { .. } => (Event::Failed, Some(Code::InvalidArguments)),
        }
    }
}

impl DiagnosticOutcome for WorkspaceSnapshot {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) { (Event::Completed, None) }
}

impl DiagnosticOutcome for ObservationSnapshot {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Unavailable { error_code, .. } => (Event::Failed, Some((*error_code).into())),
            _ => (Event::Completed, None),
        }
    }
}

impl DiagnosticOutcome for ReviewResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Text { .. } => (Event::Completed, None),
            Self::StaleSelection | Self::StaleObservation => (Event::Superseded, None),
            Self::Unsupported { .. } => (Event::Failed, Some(Code::ReviewUnsupported)),
            Self::Unavailable { code, .. } => (Event::Failed, Some((*code).into())),
        }
    }
}

impl DiagnosticOutcome for CommitReviewResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Text { .. } => (Event::Completed, None),
            Self::StaleSelection => (Event::Superseded, None),
            Self::Unsupported { .. } => (Event::Failed, Some(Code::ReviewUnsupported)),
            Self::Unavailable { code, .. } => (Event::Failed, Some((*code).into())),
        }
    }
}

impl From<ReviewErrorCode> for Code {
    fn from(code: ReviewErrorCode) -> Self {
        match code {
            ReviewErrorCode::Inaccessible => Self::Inaccessible,
            ReviewErrorCode::GitUnavailable => Self::GitUnavailable,
            ReviewErrorCode::UnsafeRepository => Self::UnsafeRepository,
            ReviewErrorCode::Timeout => Self::Timeout,
            ReviewErrorCode::ChangedDuringRead => Self::ChangedDuringRead,
            ReviewErrorCode::InvalidOutput => Self::InvalidOutput,
        }
    }
}

impl DiagnosticOutcome for HistoryPageResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Page { .. } => (Event::Completed, None),
            Self::Unavailable { code, .. } | Self::Error { code, .. } => {
                let event = if matches!(code, HistoryErrorCode::StaleSelection | HistoryErrorCode::StaleCursor) { Event::Superseded } else { Event::Failed };
                (event, Some(match code {
                    HistoryErrorCode::Inaccessible => Code::Inaccessible,
                    HistoryErrorCode::GitUnavailable => Code::GitUnavailable,
                    HistoryErrorCode::UnsafeRepository => Code::UnsafeRepository,
                    HistoryErrorCode::Timeout => Code::Timeout,
                    HistoryErrorCode::InvalidOutput => Code::InvalidOutput,
                    HistoryErrorCode::ResourceLimit => Code::ResourceLimit,
                    HistoryErrorCode::StaleSelection => Code::StaleSelection,
                    HistoryErrorCode::StaleCursor => Code::StaleCursor,
                    HistoryErrorCode::MissingObjects => Code::MissingObjects,
                }))
            }
        }
    }
}

impl DiagnosticOutcome for ContextOptionsResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Options { .. } => (Event::Completed, None),
            Self::Unavailable { code, .. } | Self::Error { code, .. } => code.result().diagnostic_outcome(),
        }
    }
}

impl DiagnosticOutcome for CommitFilesResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Files { .. } => (Event::Completed, None),
            Self::Unavailable { code, .. } | Self::Error { code, .. } => code.result().diagnostic_outcome(),
        }
    }
}

impl DiagnosticOutcome for RepositoryFilesResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Files { .. } => (Event::Completed, None),
            Self::StaleSelection => (Event::Superseded, None),
            Self::Unavailable { code, .. } => code.result().diagnostic_outcome(),
        }
    }
}

impl DiagnosticOutcome for RepositoryFileResult {
    fn diagnostic_outcome(&self) -> (Event, Option<Code>) {
        match self {
            Self::Text { .. } => (Event::Completed, None),
            Self::StaleSelection => (Event::Superseded, None),
            Self::Unsupported { .. } => (Event::Failed, Some(Code::ReviewUnsupported)),
            Self::Unavailable { code } => (Event::Failed, Some((*code).into())),
        }
    }
}

impl From<GitError> for Code {
    fn from(error: GitError) -> Self {
        match error {
            GitError::GitUnavailable => Self::GitUnavailable,
            GitError::NotRepository => Self::NotRepository,
            GitError::Inaccessible => Self::Inaccessible,
            GitError::UnsafeRepository => Self::UnsafeRepository,
            GitError::ProbeTimeout => Self::ProbeTimeout,
            GitError::RepositoryChanged => Self::RepositoryChanged,
            GitError::UnsupportedPathEncoding => Self::UnsupportedPathEncoding,
            GitError::Unavailable => Self::RepositoryUnavailable,
        }
    }
}

impl From<ObservationErrorCode> for Code {
    fn from(error: ObservationErrorCode) -> Self {
        match error {
            ObservationErrorCode::Inaccessible => Self::Inaccessible,
            ObservationErrorCode::GitUnavailable => Self::GitUnavailable,
            ObservationErrorCode::UnsafeRepository => Self::UnsafeRepository,
            ObservationErrorCode::Timeout => Self::ProbeTimeout,
            ObservationErrorCode::InvalidStatus => Self::InvalidStatus,
            ObservationErrorCode::UnsupportedPathEncoding => Self::UnsupportedPathEncoding,
            ObservationErrorCode::ResourceLimit => Self::OutputLimit,
            ObservationErrorCode::UnsupportedConfiguration => Self::UnsupportedConfiguration,
        }
    }
}

impl From<PersistenceErrorCode> for Code {
    fn from(error: PersistenceErrorCode) -> Self {
        match error {
            PersistenceErrorCode::LoadFailed => Self::LoadFailed,
            PersistenceErrorCode::UnsupportedVersion => Self::UnsupportedVersion,
            PersistenceErrorCode::SaveFailed => Self::SaveFailed,
            PersistenceErrorCode::StorageUnavailable => Self::StorageUnavailable,
        }
    }
}

impl From<StatusError> for Code {
    fn from(error: StatusError) -> Self { ObservationErrorCode::from(error).into() }
}
