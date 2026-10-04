//! Owns selected-context monitoring, bounded native path identity and renderer-safe snapshots.
//!
//! Cached reads never trigger scans. A cancellable task owns only scan facts and shared
//! publication state, not the service, so dropping the service stops its child processes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;

use crate::diagnostic_operation::{self, OperationTrace};
use crate::diagnostics::{Component, DiagnosticDetails, DiagnosticSink, Event, OperationContext, OperationKind};
use crate::diff::{ReviewCategory, ReviewResult};
use crate::git::{GitError, GitProbe, RepositoryKind};
pub use crate::git::status::{ChangeKind, UnsupportedKind};
use crate::git::status::{GitStatusReader, StatusError, StatusPath};
use crate::workspace::{DirectoryIdentity, NativeIdentity, SelectedContext};

const MAX_REGISTERED_PATHS: usize = 16_384;
const MAX_REGISTERED_PATH_BYTES: usize = 8 * 1024 * 1024;
const SCAN_INTERVAL: Duration = Duration::from_secs(1);

/// Sanitized observation failures. None of these states imply a clean working tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationErrorCode {
    Inaccessible,
    GitUnavailable,
    UnsafeRepository,
    Timeout,
    InvalidStatus,
    UnsupportedPathEncoding,
    ResourceLimit,
    UnsupportedConfiguration,
}

impl From<StatusError> for ObservationErrorCode {
    fn from(error: StatusError) -> Self {
        match error {
            StatusError::Inaccessible => Self::Inaccessible,
            StatusError::GitUnavailable => Self::GitUnavailable,
            StatusError::UnsafeRepository => Self::UnsafeRepository,
            StatusError::Timeout => Self::Timeout,
            StatusError::InvalidStatus => Self::InvalidStatus,
            StatusError::UnsupportedPathEncoding => Self::UnsupportedPathEncoding,
            StatusError::ResourceLimit => Self::ResourceLimit,
            StatusError::UnsupportedConfiguration => Self::UnsupportedConfiguration,
        }
    }
}

impl From<GitError> for ObservationErrorCode {
    fn from(error: GitError) -> Self {
        match error {
            GitError::GitUnavailable => Self::GitUnavailable,
            GitError::UnsafeRepository => Self::UnsafeRepository,
            GitError::ProbeTimeout => Self::Timeout,
            GitError::UnsupportedPathEncoding => Self::UnsupportedPathEncoding,
            _ => Self::Inaccessible,
        }
    }
}

/// Display-only changed path. Tokens are opaque native authority, never path inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPath {
    pub path_id: String,
    pub stable_path_id: String,
    pub display_path: String,
    pub segments: Vec<String>,
    pub staged: Option<ChangeKind>,
    pub unstaged: Option<ChangeKind>,
    pub untracked: bool,
    pub conflict: bool,
    pub unsupported_kind: Option<UnsupportedKind>,
}

/// Complete observation replacement; revisions are per entry, independent of workspace revisions.
///
/// Only `Ready` with no files means clean. Bare contexts have no working tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ObservationSnapshot {
    Checking { entry_id: String, observation_revision: u64 },
    Ready { entry_id: String, observation_revision: u64, files: Vec<ChangedPath> },
    Unavailable { entry_id: String, observation_revision: u64, error_code: ObservationErrorCode },
    Bare { entry_id: String, observation_revision: u64 },
}

impl DirectoryIdentity {
    fn capture(path: &Path) -> Result<Self, GitError> {
        let metadata = std::fs::metadata(path).map_err(|_| GitError::Inaccessible)?;
        if !metadata.is_dir() || std::fs::canonicalize(path).ok().as_deref() != Some(path) {
            return Err(GitError::RepositoryChanged);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Ok(Self { device: metadata.dev(), inode: metadata.ino() })
        }
        #[cfg(not(unix))]
        {
            Ok(Self { created: metadata.created().map_err(|_| GitError::Inaccessible)? })
        }
    }
}

impl NativeIdentity {
    pub(crate) fn capture(root: &Path, git_dir: &Path) -> Result<Self, GitError> {
        Ok(Self { root: DirectoryIdentity::capture(root)?, git_dir: DirectoryIdentity::capture(git_dir)? })
    }
}

#[derive(Default)]
struct EntryObservation {
    revision: u64,
    snapshot: Option<ObservationSnapshot>,
    stable_paths: HashMap<PathBuf, String>,
    registered_bytes: usize,
    // The current revision alone authorizes native paths/origins/modes; older tokens are discarded.
    tokens: HashMap<String, StatusPath>,
}

impl EntryObservation {
    fn publish(&mut self, entry_id: &str, result: Result<Option<Vec<StatusPath>>, ObservationErrorCode>) {
        if let (Ok(Some(paths)), Some(ObservationSnapshot::Ready { files, .. })) = (&result, &self.snapshot) {
            // Unchanged validated facts keep authority usable during long comparisons.
            // Preview reads still reread bytes: equal Git status does not mean equal content.
            if paths.len() == files.len() && paths.iter().zip(files).all(|(path, file)| self.tokens.get(&file.path_id) == Some(path)) {
                return;
            }
        }
        self.revision += 1;
        self.tokens.clear();
        let result = result.and_then(|paths| paths.map(|paths| self.register(entry_id, paths)).transpose());
        self.snapshot = Some(match result {
            Ok(Some(files)) => ObservationSnapshot::Ready { entry_id: entry_id.to_owned(), observation_revision: self.revision, files },
            Ok(None) => ObservationSnapshot::Bare { entry_id: entry_id.to_owned(), observation_revision: self.revision },
            Err(error_code) => ObservationSnapshot::Unavailable { entry_id: entry_id.to_owned(), observation_revision: self.revision, error_code },
        });
    }

    fn register(&mut self, entry_id: &str, paths: Vec<StatusPath>) -> Result<Vec<ChangedPath>, ObservationErrorCode> {
        let mut files = Vec::with_capacity(paths.len());
        for path in paths {
            let stable_path_id = if let Some(id) = self.stable_paths.get(&path.native_path) {
                id.clone()
            } else {
                let bytes = path.native_path.as_os_str().len();
                if self.stable_paths.len() == MAX_REGISTERED_PATHS
                    || bytes > MAX_REGISTERED_PATH_BYTES.saturating_sub(self.registered_bytes) {
                    self.tokens.clear();
                    return Err(ObservationErrorCode::ResourceLimit);
                }
                let id = format!("{entry_id}-path-{}", self.stable_paths.len() + 1);
                self.stable_paths.insert(path.native_path.clone(), id.clone());
                self.registered_bytes += bytes;
                id
            };
            let path_id = format!("{entry_id}-revision-{}-path-{}", self.revision, files.len() + 1);
            files.push(ChangedPath {
                path_id: path_id.clone(),
                stable_path_id,
                display_path: path.display_path.clone(),
                segments: path.segments.clone(),
                staged: path.staged.clone(),
                unstaged: path.unstaged.clone(),
                untracked: path.untracked,
                conflict: path.conflict,
                unsupported_kind: path.unsupported_kind.clone(),
            });
            self.tokens.insert(path_id, path);
        }
        Ok(files)
    }
}

#[derive(Default)]
struct ObservationState {
    generation: u64,
    selected_id: Option<String>,
    context_epoch: String,
    observing: bool,
    entries: HashMap<String, EntryObservation>,
    started_scan: u64,
    completed_start: u64,
    completed_scan_sequence: u64,
    requested_start: u64,
    recovery_failures: u64,
}

/// Owns the only scan task. The task never owns this controller or the service.
#[derive(Default)]
pub(crate) struct ObservationController {
    state: Arc<Mutex<ObservationState>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    reader: GitStatusReader,
    diagnostics: DiagnosticSink,
    demand_suspended: AtomicBool,
    scan_requested: Arc<tokio::sync::Notify>,
    state_changed: Arc<tokio::sync::Notify>,
}

impl ObservationController {
    pub(crate) fn with_diagnostics(diagnostics: DiagnosticSink) -> Self {
        let mut controller = Self::default();
        controller.diagnostics = diagnostics;
        controller
    }

    pub(crate) fn stop(&self) -> Option<tokio::task::JoinHandle<()>> {
        let task = self.task.lock().take();
        let mut state = self.state.lock();
        state.generation += 1;
        state.observing = false;
        if let Some(task) = &task { task.abort(); }
        self.state_changed.notify_waiters();
        task
    }

    /// Invalidates cached native authority for an app-removed entry without touching its files.
    pub(crate) fn remove(&self, entry_id: &str) {
        let mut task = self.task.lock();
        let mut state = self.state.lock();
        state.entries.remove(entry_id);
        if state.selected_id.as_deref() == Some(entry_id) {
            state.generation += 1;
            state.selected_id = None;
            state.observing = false;
            if let Some(task) = task.take() {
                task.abort();
            }
        }
    }

    /// Changes the cached selected context without issuing I/O for an unverified root.
    pub(crate) fn select_unverified(&self, entry_id: &str, context_epoch: String) {
        if let Some(previous) = self.task.lock().take() {
            previous.abort();
        }
        let mut state = self.state.lock();
        state.generation += 1;
        state.selected_id = Some(entry_id.to_owned());
        state.context_epoch = context_epoch;
        state.observing = false;
        let entry = state.entries.entry(entry_id.to_owned()).or_default();
        entry.revision += 1;
        entry.tokens.clear();
        entry.snapshot = Some(ObservationSnapshot::Checking {
            entry_id: entry_id.to_owned(), observation_revision: entry.revision,
        });
    }

    pub(crate) fn is_observing(&self, entry_id: &str) -> bool {
        let state = self.state.lock();
        state.observing && state.selected_id.as_deref() == Some(entry_id)
    }

    /// Host visibility changes synchronously stop native producers before asynchronous reconciliation.
    pub(crate) fn set_demand(&self, demanded: bool) {
        self.demand_suspended.store(!demanded, Ordering::SeqCst);
        if !demanded { self.stop(); }
    }

    /// A ticket requires a scan STARTED after this call, not merely an old scan finishing.
    pub(crate) fn request_fresh_scan(&self, entry_id: &str) -> Option<(u64, u64)> {
        let mut state = self.state.lock();
        if !state.observing || state.selected_id.as_deref() != Some(entry_id) { return None; }
        let required_start = state.started_scan + 1;
        state.requested_start = required_start;
        let ticket = (state.generation, required_start);
        self.scan_requested.notify_one();
        Some(ticket)
    }

    /// Waits for this opening's pending recovery without treating a cached failure as a new attempt.
    pub(crate) async fn fresh_scan_after_recovery(&self, entry_id: &str, context_epoch: &str) -> Option<(u64, u64)> {
        let previous_failures = self.state.lock().recovery_failures;
        loop {
            let notified = self.state_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock();
                if state.context_epoch != context_epoch || state.selected_id.as_deref() != Some(entry_id) { return None; }
                if state.observing {
                    let required_start = state.started_scan + 1;
                    state.requested_start = required_start;
                    self.scan_requested.notify_one();
                    return Some((state.generation, required_start));
                }
                if state.recovery_failures > previous_failures { return None; }
            }
            notified.await;
        }
    }

    pub(crate) async fn await_fresh_scan(&self, ticket: (u64, u64)) -> bool {
        loop {
            let notified = self.state_changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let state = self.state.lock();
                if state.generation != ticket.0 || !state.observing { return false; }
                if state.completed_start >= ticket.1 { return true; }
            }
            notified.await;
        }
    }

    pub(crate) fn unverified_unavailable(&self, entry_id: &str) {
        let mut state = self.state.lock();
        if !state.observing && state.selected_id.as_deref() == Some(entry_id) {
            state.entries.entry(entry_id.to_owned()).or_default()
                .publish(entry_id, Err(ObservationErrorCode::Inaccessible));
            state.recovery_failures += 1;
            self.state_changed.notify_waiters();
        }
    }

    pub(crate) fn select(&self, context: SelectedContext, context_epoch: String) {
        let mut task_slot = self.task.lock();
        if self.demand_suspended.load(Ordering::SeqCst) { return; }
        let previous = task_slot.take();
        if let Some(task) = &previous {
            task.abort();
        }
        let generation = {
            let mut state = self.state.lock();
            state.generation += 1;
            state.selected_id = Some(context.entry_id.clone());
            state.context_epoch = context_epoch;
            state.observing = true;
            let generation = state.generation;
            let entry = state.entries.entry(context.entry_id.clone()).or_default();
            entry.revision += 1;
            entry.tokens.clear();
            entry.snapshot = Some(ObservationSnapshot::Checking {
                entry_id: context.entry_id.clone(), observation_revision: entry.revision,
            });
            generation
        };
        self.state_changed.notify_waiters();
        let state = Arc::clone(&self.state);
        let reader = self.reader.clone();
        let scan_requested = Arc::clone(&self.scan_requested);
        let state_changed = Arc::clone(&self.state_changed);
        let parent = OperationContext::current()
            .or_else(|| diagnostic_operation::context(&self.diagnostics, OperationKind::ScanContext));
        let task = tokio::spawn(crate::native_work::inherit(async move {
            if let Some(previous) = previous {
                // Observe cancellation before starting another scan; the adapter has requested kill/reap.
                // The cancelled generation cannot publish and has no remaining outcome to surface.
                let _ = previous.await;
            }
            let probe = GitProbe::default();
            loop {
                {
                    let wake = scan_requested.notified();
                    tokio::pin!(wake);
                    wake.as_mut().enable();
                }
                let started_scan = {
                    let mut state = state.lock();
                    if state.generation != generation { return; }
                    state.started_scan += 1;
                    state.started_scan
                };
                let operation = parent.as_ref().map(|parent| parent.child().with_kind(OperationKind::ScanContext));
                let superseded = diagnostic_operation::scoped(operation.as_ref(), async {
                    let mut trace = operation.as_ref().map(|context| OperationTrace::new(context.clone(), Component::Observation));
                    let result = scan(&context, &probe, &reader).await;
                    let (event, code, superseded) = {
                        let mut state = state.lock();
                        if state.generation != generation || state.selected_id.as_deref() != Some(&context.entry_id) {
                            (Event::Superseded, None, true)
                        } else {
                            let entry = state.entries.get_mut(&context.entry_id).unwrap();
                            entry.publish(&context.entry_id, result);
                            let code = match entry.snapshot.as_ref() {
                                Some(ObservationSnapshot::Unavailable { error_code, .. }) => Some((*error_code).into()),
                                _ => None,
                            };
                            state.completed_start = started_scan;
                            state.completed_scan_sequence += 1;
                            state_changed.notify_waiters();
                            (if code.is_some() { Event::Failed } else { Event::Completed }, code, false)
                        }
                    };
                    if let Some(trace) = &mut trace { trace.finish(event, code, DiagnosticDetails::default()); }
                    superseded
                }).await;
                if superseded { return; }
                // A fresh-opening ticket coalesces with the next scan, never the one already started.
                if state.lock().requested_start > started_scan { continue; }
                tokio::select! {
                    _ = tokio::time::sleep(SCAN_INTERVAL) => {},
                    _ = scan_requested.notified() => {},
                }
            }
        }));
        *task_slot = Some(task);
    }

    /// Selection authority independent of the periodically changing file observation revision.
    pub(crate) fn selection_generation(&self, entry_id: &str) -> Option<u64> {
        let state = self.state.lock();
        (state.observing && state.selected_id.as_deref() == Some(entry_id)).then_some(state.generation)
    }

    /// Captures one revision-bound native token under the selected scan generation.
    pub(crate) fn authorize_review(&self, entry_id: &str, revision: u64, path_id: &str, category: ReviewCategory) -> Result<(u64, StatusPath), ReviewResult> {
        let state = self.state.lock();
        if state.selected_id.as_deref() != Some(entry_id) || !state.observing {
            return Err(ReviewResult::StaleSelection);
        }
        let entry = state.entries.get(entry_id).ok_or(ReviewResult::StaleObservation)?;
        if entry.revision != revision { return Err(ReviewResult::StaleObservation); }
        let path = entry.tokens.get(path_id).filter(|path| category.authorizes(path))
            .ok_or(ReviewResult::StaleObservation)?;
        Ok((state.generation, path.clone()))
    }

    pub(crate) fn validate_review(&self, generation: u64, entry_id: &str, revision: u64, path_id: &str, category: ReviewCategory) -> Result<(), ReviewResult> {
        let state = self.state.lock();
        if state.generation != generation || state.selected_id.as_deref() != Some(entry_id) || !state.observing {
            return Err(ReviewResult::StaleSelection);
        }
        let entry = state.entries.get(entry_id).ok_or(ReviewResult::StaleObservation)?;
        if entry.revision != revision || !entry.tokens.get(path_id).is_some_and(|path| category.authorizes(path)) {
            return Err(ReviewResult::StaleObservation);
        }
        Ok(())
    }

    pub(crate) fn handoff_authority(&self, selection: &crate::companion::HandoffSelection) -> Option<StatusPath> {
        let state = self.state.lock();
        if !state.observing || state.selected_id.as_deref() != Some(&selection.entry_id) { return None; }
        let entry = state.entries.get(&selection.entry_id)?;
        if entry.revision != selection.observation_revision { return None; }
        let path = entry.tokens.get(&selection.path_id)?;
        if !selection.category.authorizes(path) || entry.stable_paths.get(&path.native_path) != Some(&selection.stable_path_id) { return None; }
        Some(path.clone())
    }

    pub(crate) fn review_provenance(&self, entry_id: &str, revision: u64, path_id: &str, category: ReviewCategory) -> Option<crate::companion::IssuedReview> {
        let state = self.state.lock();
        if !state.observing || state.selected_id.as_deref() != Some(entry_id) { return None; }
        let entry = state.entries.get(entry_id)?;
        if entry.revision != revision { return None; }
        let path = entry.tokens.get(path_id)?.clone();
        if !category.authorizes(&path) { return None; }
        let stable_path_id = entry.stable_paths.get(&path.native_path)?.clone();
        Some(crate::companion::IssuedReview {
            selection: crate::companion::HandoffSelection { entry_id: entry_id.to_owned(), stable_path_id, observation_revision: revision, path_id: path_id.to_owned(), category }, path,
        })
    }

    pub(crate) fn surface_snapshot(&self, entry_id: &str, context_epoch: &str) -> Option<ObservationSnapshot> {
        let state = self.state.lock();
        if state.context_epoch != context_epoch || state.selected_id.as_deref() != Some(entry_id) { return None; }
        state.entries.get(entry_id)?.snapshot.clone()
    }

    pub(crate) fn snapshot(&self, entry_id: &str) -> ObservationSnapshot {
        let state = self.state.lock();
        if state.selected_id.as_deref() == Some(entry_id) {
            if let Some(snapshot) = state.entries.get(entry_id).and_then(|entry| entry.snapshot.as_ref()) {
                return snapshot.clone();
            }
        }
        ObservationSnapshot::Unavailable {
            entry_id: entry_id.to_owned(),
            observation_revision: state.entries.get(entry_id).map_or(0, |entry| entry.revision),
            error_code: ObservationErrorCode::Inaccessible,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_reader(reader: GitStatusReader) -> Self {
        let mut controller = Self::default();
        controller.reader = reader;
        controller
    }
}

impl Drop for ObservationController {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}

async fn verify_context(context: &SelectedContext, probe: &GitProbe) -> Result<(), ObservationErrorCode> {
    if NativeIdentity::capture(&context.root, &context.git_dir)? != context.identity {
        return Err(ObservationErrorCode::Inaccessible);
    }
    let facts = probe.probe(&context.root).await?;
    if facts.root != context.root || facts.git_dir != context.git_dir || facts.kind != context.kind
        || NativeIdentity::capture(&context.root, &context.git_dir)? != context.identity {
        return Err(ObservationErrorCode::Inaccessible);
    }
    Ok(())
}

async fn scan(
    context: &SelectedContext, probe: &GitProbe, reader: &GitStatusReader,
) -> Result<Option<Vec<StatusPath>>, ObservationErrorCode> {
    verify_context(context, probe).await?;
    if context.kind == RepositoryKind::Bare {
        return Ok(None);
    }
    let paths = reader.read(&context.root).await?;
    // Recheck after reading too: replaced roots and metadata must never publish an apparently clean result.
    verify_context(context, probe).await?;
    Ok(Some(paths))
}

#[cfg(all(test, unix))]
#[path = "../tests/integration/observation_internal.rs"]
mod integration_tests;
