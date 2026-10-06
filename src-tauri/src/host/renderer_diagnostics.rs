//! Accepts only typed, bounded renderer terminal metadata for native capture.

use serde::Deserialize;

use super::{operation_id, INVALID_DIAGNOSTIC_METADATA};
use crate::diagnostics::{Code, Component, DiagnosticDetails, DiagnosticStore, Event, Level, OperationContext, OperationKind};

const MAX_RENDERER_DURATION_MS: u64 = 86_400_000;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RendererCommand {
    WorkspaceSnapshot,
    OpenChosenRepository,
    SelectContext,
    RenameRepository,
    RemoveRepository,
    RefreshEntryAvailability,
    ObserveSelectedContext,
    ReviewFile,
    HistoryPage,
    ListContexts,
    SelectWorktree,
    CommitFiles,
    UpstreamFiles,
    ReviewCommitFile,
    ListRepositoryFiles,
    ReviewRepositoryFile,
}

impl RendererCommand {
    fn kind(&self) -> OperationKind {
        match self {
            Self::WorkspaceSnapshot => OperationKind::WorkspaceSnapshot,
            Self::OpenChosenRepository => OperationKind::OpenRepository,
            Self::SelectContext => OperationKind::SelectContext,
            Self::RenameRepository => OperationKind::RenameRepository,
            Self::RemoveRepository => OperationKind::RemoveRepository,
            Self::RefreshEntryAvailability => OperationKind::RefreshAvailability,
            Self::ObserveSelectedContext => OperationKind::ObserveContext,
            Self::ReviewFile => OperationKind::ReviewFile,
            Self::HistoryPage => OperationKind::HistoryPage,
            Self::ListContexts => OperationKind::ListContexts,
            Self::SelectWorktree => OperationKind::SelectWorktree,
            Self::CommitFiles => OperationKind::CommitFiles,
            Self::UpstreamFiles => OperationKind::UpstreamFiles,
            Self::ReviewCommitFile => OperationKind::ReviewCommitFile,
            Self::ListRepositoryFiles => OperationKind::ListRepositoryFiles,
            Self::ReviewRepositoryFile => OperationKind::ReviewRepositoryFile,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RendererPhase { Completed, TransportFailed }

/// Strict renderer terminals contain no arguments, results, paths, or unknown error payloads.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RendererDiagnostic {
    operation_id: String,
    command: RendererCommand,
    phase: RendererPhase,
    duration_ms: u64,
}

impl RendererDiagnostic {
    pub(super) fn record(self, store: &DiagnosticStore) -> Result<(), &'static str> {
        let id = operation_id(&self.operation_id)?;
        if self.duration_ms > MAX_RENDERER_DURATION_MS { return Err(INVALID_DIAGNOSTIC_METADATA); }
        let context = OperationContext::new(store.sink(), Some(id), None).with_kind(self.command.kind());
        let (level, event, code) = match self.phase {
            RendererPhase::Completed => (Level::Info, Event::Completed, None),
            RendererPhase::TransportFailed => (Level::Error, Event::Failed, Some(Code::Transport)),
        };
        // Enqueue failure is visible through health; it must not change the repository result.
        context.record(level, Component::Renderer, event, code, DiagnosticDetails {
            duration_ms: Some(self.duration_ms), ..Default::default()
        });
        Ok(())
    }
}
