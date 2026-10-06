//! Defines the closed diagnostic vocabulary and bounded, non-payload metadata.

use serde::{Deserialize, Serialize};
use std::{future::Future, str::FromStr};
use uuid::Uuid;
use super::DiagnosticSink;

macro_rules! vocabulary {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[repr(u8)]
        pub enum $name { $(#[serde(rename = $text)] $variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }
        impl FromStr for $name {
            type Err = Code;
            fn from_str(value: &str) -> Result<Self, Code> {
                match value { $($text => Ok(Self::$variant)),+, _ => Err(Code::InvalidArguments) }
            }
        }
    };
}

vocabulary!(Level { Debug => "debug", Info => "info", Warn => "warn", Error => "error" });
vocabulary!(Component {
    Renderer => "renderer", Ipc => "ipc", Application => "application", Git => "git",
    Process => "process", Observation => "observation", Persistence => "persistence",
    Startup => "startup", Diagnostics => "diagnostics"
});
// Lifecycle names are interpreted together with the component column.
vocabulary!(Event {
    Started => "started", Completed => "completed", Failed => "failed",
    Cancelled => "cancelled", Superseded => "superseded",
    CleanupCompleted => "cleanup_completed", CleanupFailed => "cleanup_failed"
});
vocabulary!(Code {
    GitUnavailable => "git_unavailable", NotRepository => "not_repository",
    Inaccessible => "inaccessible", UnsafeRepository => "unsafe_repository",
    ProbeTimeout => "probe_timeout", RepositoryChanged => "repository_changed",
    UnsupportedPathEncoding => "unsupported_path_encoding", RepositoryUnavailable => "repository_unavailable",
    InvalidStatus => "invalid_status", StatusUnavailable => "status_unavailable",
    UnsupportedConfiguration => "unsupported_configuration",
    LoadFailed => "load_failed", UnsupportedVersion => "unsupported_version",
    SaveFailed => "save_failed", StorageUnavailable => "storage_unavailable",
    ProcessStart => "process_start", ProcessIo => "process_io", OutputLimit => "output_limit",
    Deadline => "deadline", Cleanup => "cleanup", Transport => "transport", Overflow => "overflow",
    Storage => "storage", Schema => "schema", InvalidRecord => "invalid_record",
    ReviewUnsupported => "review_unsupported", InvalidOutput => "invalid_output", ChangedDuringRead => "changed_during_read",
    ResourceLimit => "resource_limit", StaleSelection => "stale_selection", StaleCursor => "stale_cursor", MissingObjects => "missing_objects",
    InvalidArguments => "invalid_arguments", Shutdown => "shutdown", Timeout => "timeout", Disabled => "disabled"
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthState { Healthy, Degraded, Disabled, Stopped }

#[derive(Clone, Copy, Debug, Serialize)]
pub struct DiagnosticHealth {
    pub state: HealthState,
    pub accepted: u64,
    pub written: u64,
    pub dropped: u64,
    pub last_error_code: Option<Code>,
}

/// Byte counts and monotonic durations must fit SQLite's signed integer range.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiagnosticDetails {
    pub duration_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub stdout_bytes: Option<u64>,
    pub stderr_bytes: Option<u64>,
    pub cleanup_failed: bool,
}

impl DiagnosticDetails {
    pub(super) fn valid(self) -> bool {
        [self.duration_ms, self.stdout_bytes, self.stderr_bytes]
            .into_iter().flatten().all(|value| value <= i64::MAX as u64)
    }
}

vocabulary!(OperationKind {
    WorkspaceSnapshot => "workspace_snapshot", OpenRepository => "open_repository",
    SelectContext => "select_context", RefreshAvailability => "refresh_availability",
    ObserveContext => "observe_context", RenameRepository => "rename_repository",
    RemoveRepository => "remove_repository", RestoreWorkspace => "restore_workspace",
    RecoverContext => "recover_context", ScanContext => "scan_context",
    ReviewFile => "review_file",
    ReviewCommitFile => "review_commit_file",
    ListRepositoryFiles => "list_repository_files", ReviewRepositoryFile => "review_repository_file",
    HistoryPage => "history_page",
    ListContexts => "list_contexts", SelectWorktree => "select_worktree", CommitFiles => "commit_files", UpstreamFiles => "upstream_files",
    PersistWorkspace => "persist_workspace", Startup => "startup", Unknown => "unknown"
});

#[derive(Clone, Copy, Debug)]
pub struct DiagnosticRecord {
    pub operation_id: Uuid,
    pub parent_operation_id: Option<Uuid>,
    pub operation_kind: OperationKind,
    pub level: Level,
    pub component: Component,
    pub event: Event,
    pub code: Option<Code>,
    pub details: DiagnosticDetails,
}

impl DiagnosticRecord {
    pub(super) fn valid(self) -> bool {
        valid_id(self.operation_id) && self.parent_operation_id.is_none_or(valid_id) && self.details.valid()
    }
}

pub(super) fn valid_id(id: Uuid) -> bool {
    id.get_version() == Some(uuid::Version::Random) && id.get_variant() == uuid::Variant::RFC4122
}

pub(super) fn parse_id(value: &str) -> Result<Uuid, Code> {
    let id = Uuid::parse_str(value).map_err(|_| Code::InvalidArguments)?;
    let mut buffer = Uuid::encode_buffer();
    if !valid_id(id) || id.hyphenated().encode_lower(&mut buffer) != value { return Err(Code::InvalidArguments); }
    Ok(id)
}

tokio::task_local! { static CURRENT: OperationContext; }

/// Opaque tracing identity. It carries no repository identity or native payload.
#[derive(Clone)]
pub struct OperationContext {
    sink: DiagnosticSink,
    operation_id: Uuid,
    parent_operation_id: Option<Uuid>,
    kind: OperationKind,
}

impl OperationContext {
    /// IDs supplied by IPC must already be validated UUID v4 values.
    pub fn new(sink: DiagnosticSink, operation_id: Option<Uuid>, parent_operation_id: Option<Uuid>) -> Self {
        Self { sink, operation_id: operation_id.unwrap_or_else(Uuid::new_v4), parent_operation_id, kind: OperationKind::Unknown }
    }

    pub fn child(&self) -> Self {
        Self::new(self.sink.clone(), None, Some(self.operation_id)).with_kind(self.kind)
    }

    pub fn with_kind(mut self, kind: OperationKind) -> Self { self.kind = kind; self }
    pub fn kind(&self) -> OperationKind { self.kind }

    pub fn id(&self) -> Uuid { self.operation_id }
    pub fn parent_id(&self) -> Option<Uuid> { self.parent_operation_id }

    /// Scope is restored on completion or cancellation; spawned tasks require an explicit scope.
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub fn current() -> Option<Self> { CURRENT.try_with(Clone::clone).ok() }

    pub fn record(&self, level: Level, component: Component, event: Event, code: Option<Code>, details: DiagnosticDetails) -> bool {
        self.sink.record(DiagnosticRecord {
            operation_id: self.operation_id, parent_operation_id: self.parent_operation_id,
            operation_kind: self.kind,
            level, component, event, code, details,
        })
    }
}
