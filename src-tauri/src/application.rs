//! Coordinates repository admission, private workspace persistence and background restoration.
//!
//! Workspace tickets order completions; no Git or storage I/O holds state/selection locks,
//! and background tasks own shared state rather than the service itself.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::diagnostic_operation::{self, DiagnosticOutcome, OperationTrace};
use crate::diagnostics::{Component, DiagnosticDetails, DiagnosticSink, Event, OperationContext, OperationKind};
use crate::browsing::{self, BrowsingController, RepositoryFileResult, RepositoryFilesRequest, RepositoryFilesResult};
use crate::diff::committed::{self, CommitReviewController, CommitReviewResult};
use crate::diff::{self, ReviewCategory, ReviewResult};
use crate::git::{GitError, GitProbe};
use crate::history::{self, HistoryController, HistoryErrorCode, HistoryPageResult};
use crate::inspection::{self, CommitFilesResult, ContextController, ContextOptionsResult};
use crate::observation::{ObservationController, ObservationSnapshot};
use crate::git::process::{GitProcess, ProbeDeadline};
use crate::workspace::{MutationOutcome, NativeIdentity, OpenOutcome, RefreshPublication, RefreshTicket, SelectOutcome, WorkspaceRejectionCode, WorkspaceSnapshot, WorkspaceStore};
use crate::workspace::persistence::{self, PersistenceError};

/// Application boundary for native picker paths and opaque workspace entry IDs.
#[derive(Default)]
pub struct RepositoryService {
    probe: GitProbe,
    inspection_process: GitProcess,
    workspace: Arc<WorkspaceStore>,
    observation: Arc<ObservationController>,
    history: HistoryController,
    contexts: ContextController,
    committed_reviews: CommitReviewController,
    browsing: BrowsingController,
    // Order selection plus scan reset as one transition, including same-ID reselection.
    selection: Arc<tokio::sync::Mutex<()>>,
    persistence: Option<WorkspacePersistence>,
    restoration: OwnedTask,
    recovery: Arc<RecoveryController>,
    diagnostics: DiagnosticSink,
}

struct WorkspacePersistence {
    path: PathBuf,
    gate: Arc<tokio::sync::Mutex<()>>,
    // The tail owns prior saves; dropping its handle detaches rather than interrupts writes.
    task: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Task handles stay outside the data captured by their futures, avoiding service cycles.
#[derive(Default)]
struct OwnedTask(parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>);

impl Drop for OwnedTask {
    fn drop(&mut self) {
        if let Some(task) = self.0.get_mut().take() {
            task.abort();
        }
    }
}

#[derive(Default)]
struct RecoveryController {
    task: parking_lot::Mutex<Option<(String, tokio::task::JoinHandle<()>)>>,
}

#[derive(Clone, Copy)]
enum RecoveryStart {
    AfterRestoreFailure,
    UserSelection,
}

impl RecoveryController {
    fn cancel(&self) {
        if let Some((_, task)) = self.task.lock().take() {
            task.abort();
        }
    }

    fn start(
        &self, entry_id: String, probe: GitProbe, workspace: Arc<WorkspaceStore>,
        observation: Arc<ObservationController>, selection: Arc<tokio::sync::Mutex<()>>,
        reason: RecoveryStart,
    ) {
        let mut current = self.task.lock();
        if matches!(reason, RecoveryStart::AfterRestoreFailure)
            && current.as_ref().is_some_and(|(id, task)| id == &entry_id && !task.is_finished()) {
            return;
        }
        let previous = current.take().map(|(_, task)| {
            task.abort();
            task
        });
        let selected_id = entry_id.clone();
        let parent = OperationContext::current();
        let task = tokio::spawn(async move {
            if let Some(previous) = previous {
                // Cancellation drops the probe and delegates its child reap to the native adapter.
                let _ = previous.await;
            }
            if matches!(reason, RecoveryStart::AfterRestoreFailure) {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            loop {
                let attempt = parent.as_ref().map(|parent| parent.child().with_kind(OperationKind::RecoverContext));
                let finished = diagnostic_operation::scoped(attempt.as_ref(), async {
                    let Some(ticket) = workspace.begin_selected_recovery(&selected_id).await else {
                        return true;
                    };
                    recheck(&probe, &workspace, ticket).await;
                    let _selection = selection.lock().await;
                    if let Some(context) = workspace.selected_context(&selected_id).await {
                        if !observation.is_observing(&selected_id) {
                            observation.select(context);
                        }
                        return true;
                    }
                    if workspace.selected_unverified_id().await.as_deref() != Some(&selected_id) {
                        return true;
                    }
                    observation.unverified_unavailable(&selected_id);
                    false
                }).await;
                if finished { return; }
                // Completion-based retries coalesce slow probes rather than queueing ticks.
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        *current = Some((entry_id, task));
    }
}

impl Drop for RecoveryController {
    fn drop(&mut self) {
        if let Some((_, task)) = self.task.get_mut().take() {
            task.abort();
        }
    }
}

async fn recheck(probe: &GitProbe, workspace: &WorkspaceStore, ticket: RefreshTicket) -> RefreshPublication {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Application));
    let result = async {
        let deadline = ProbeDeadline::new();
        let mut facts = probe.probe_with_deadline(&ticket.root, deadline).await?;
        let mut identity = NativeIdentity::capture(&facts.root, &facts.git_dir)?;
        if let Some(verified) = &ticket.verified {
            if verified.git_dir != facts.git_dir || verified.identity != identity {
                return Err(GitError::RepositoryChanged);
            }
        } else {
            // A restored location has no trusted identity yet; confirm both passes agree.
            let confirmed = probe.probe_with_deadline(&ticket.root, deadline).await?;
            let confirmed_identity = NativeIdentity::capture(&confirmed.root, &confirmed.git_dir)?;
            if facts.root != confirmed.root || facts.git_dir != confirmed.git_dir
                || facts.kind != confirmed.kind || identity != confirmed_identity {
                return Err(GitError::RepositoryChanged);
            }
            facts = confirmed;
            identity = confirmed_identity;
        }
        Ok((facts, identity))
    }.await;
    let publication = workspace.complete_refresh(ticket, result).await;
    if let Some(trace) = &mut trace {
        let (event, code) = match &publication {
            RefreshPublication::Verified => (Event::Completed, None),
            RefreshPublication::Unavailable(error) => (Event::Failed, Some((*error).into())),
            RefreshPublication::Superseded => (Event::Superseded, None),
        };
        trace.finish(event, code, DiagnosticDetails::default());
    }
    publication
}

async fn activate_verified_selection(
    workspace: &WorkspaceStore, observation: &ObservationController,
) {
    if let Some(entry_id) = workspace.active_context_id().await {
        if !observation.is_observing(&entry_id) {
            if let Some(context) = workspace.selected_context(&entry_id).await {
                observation.select(context);
            }
        }
    }
}

impl RepositoryService {
    /// Creates an explicitly ephemeral workspace, without reading or writing saved choices.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an ephemeral workspace whose operations use the host-owned capture lifetime.
    pub fn with_diagnostics(diagnostics: DiagnosticSink) -> Self {
        Self::with_probe_and_diagnostics(GitProbe::default(), diagnostics)
    }

    fn with_probe_and_diagnostics(probe: GitProbe, diagnostics: DiagnosticSink) -> Self {
        Self {
            probe,
            inspection_process: GitProcess::default(),
            workspace: Arc::default(),
            observation: Arc::new(ObservationController::with_diagnostics(diagnostics.clone())),
            history: HistoryController::default(),
            contexts: ContextController::default(),
            committed_reviews: CommitReviewController::default(),
            browsing: BrowsingController::default(),
            selection: Arc::default(),
            persistence: None,
            restoration: OwnedTask::default(),
            recovery: Arc::default(),
            diagnostics,
        }
    }

    pub(crate) fn diagnostic_sink(&self) -> DiagnosticSink { self.diagnostics.clone() }

    async fn trace_operation<T: DiagnosticOutcome>(
        &self, kind: OperationKind, future: impl Future<Output = T>,
    ) -> T {
        let context = diagnostic_operation::context(&self.diagnostics, kind);
        diagnostic_operation::scoped(context.as_ref(), async {
            let mut trace = context.as_ref().map(|context| OperationTrace::new(context.clone(), Component::Application));
            let outcome = future.await;
            if let Some(trace) = &mut trace { trace.outcome(&outcome); }
            outcome
        }).await
    }

    /// Stops and awaits owned producers before the host drains accepted diagnostics.
    /// Accepted choice writes finish before capture is drained, even if their requester cancelled.
    /// Detached child-reap evidence remains best effort at runtime exit, never a durability promise.
    pub async fn shutdown(&self) {
        let restoration = self.restoration.0.lock().take();
        let recovery = self.recovery.task.lock().take().map(|(_, task)| task);
        let observation = self.observation.stop();
        let tasks = [restoration, recovery, observation];
        // Stop every producer before awaiting any handle, and never await while its mutex is held.
        for task in tasks.iter().flatten() { task.abort(); }
        for task in tasks.into_iter().flatten() { let _ = task.await; }
        let persistence = self.persistence.as_ref().and_then(|persistence| persistence.task.lock().take());
        if let Some(task) = persistence { let _ = task.await; }
    }

    /// Loads native-managed choices and starts bounded verification without caching Git facts.
    ///
    /// An unreadable or unsupported document leaves a usable ephemeral session and disables
    /// writes for this lifetime, preserving the original bytes until repaired and restarted.
    pub async fn with_workspace_file(path: PathBuf) -> Self {
        Self::load_workspace(path, GitProbe::default()).await
    }

    /// Initializes capture before any persisted choice can spawn restoration or monitoring.
    pub async fn with_workspace_file_and_diagnostics(path: PathBuf, diagnostics: DiagnosticSink) -> Self {
        Self::load_workspace_with_diagnostics(path, GitProbe::default(), diagnostics).await
    }

    async fn load_workspace(path: PathBuf, probe: GitProbe) -> Self {
        Self::load_workspace_with_diagnostics(path, probe, DiagnosticSink::default()).await
    }

    async fn load_workspace_with_diagnostics(path: PathBuf, probe: GitProbe, diagnostics: DiagnosticSink) -> Self {
        let restoration = OperationContext::current().map(|parent| parent.child().with_kind(OperationKind::RestoreWorkspace))
            .or_else(|| diagnostic_operation::context(&diagnostics, OperationKind::RestoreWorkspace));
        let document = diagnostic_operation::scoped(restoration.as_ref(), async {
            let mut trace = restoration.as_ref().map(|context| OperationTrace::new(context.clone(), Component::Persistence));
            let result = persistence::load(&path).await;
            if let Some(trace) = &mut trace {
                trace.finish(if result.is_ok() { Event::Completed } else { Event::Failed },
                    result.as_ref().err().map(|error| error.code.into()), DiagnosticDetails::default());
            }
            result
        }).await;
        let mut service = Self::with_probe_and_diagnostics(probe, diagnostics);
        let document = match document {
            Ok(document) => document,
            Err(error) => {
                service.workspace.set_persistence_error(Some(error)).await;
                return service;
            }
        };
        service.persistence = Some(WorkspacePersistence {
            path, gate: Arc::default(), task: parking_lot::Mutex::new(None),
        });
        let tickets = service.workspace.restore(document).await;
        if tickets.is_empty() {
            return service;
        }
        if let Some(entry_id) = service.workspace.active_context_id().await {
            service.observation.select_unverified(&entry_id);
        }
        let workspace = Arc::clone(&service.workspace);
        let observation = Arc::clone(&service.observation);
        let selection = Arc::clone(&service.selection);
        let recovery = Arc::clone(&service.recovery);
        let probe = service.probe.clone();
        let task = tokio::spawn(async move {
            for ticket in tickets {
                let context = restoration.as_ref().map(|parent| parent.child().with_kind(OperationKind::RestoreWorkspace));
                diagnostic_operation::scoped(context.as_ref(), async {
                    recheck(&probe, &workspace, ticket).await;
                    let _selection = selection.lock().await;
                    activate_verified_selection(&workspace, &observation).await;
                    if let Some(entry_id) = workspace.selected_unavailable_unverified_id().await {
                        observation.unverified_unavailable(&entry_id);
                        recovery.start(entry_id, probe.clone(), Arc::clone(&workspace),
                            Arc::clone(&observation), Arc::clone(&selection), RecoveryStart::AfterRestoreFailure);
                    }
                }).await;
            }
            workspace.finish_restoration().await;
        });
        *service.restoration.0.lock() = Some(task);
        service
    }

    /// Keeps failed host storage resolution visible while sharing its diagnostic capture health.
    pub(crate) async fn storage_unavailable(diagnostics: DiagnosticSink) -> Self {
        let service = Self::with_diagnostics(diagnostics);
        service.workspace.set_persistence_error(Some(PersistenceError::storage_unavailable())).await;
        service
    }

    async fn save_choices(&self) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let context = OperationContext::current().map(|parent| parent.child().with_kind(OperationKind::PersistWorkspace))
            .or_else(|| diagnostic_operation::context(&self.diagnostics, OperationKind::PersistWorkspace));
        let workspace = Arc::clone(&self.workspace);
        let path = persistence.path.clone();
        let gate = Arc::clone(&persistence.gate);
        let (completed, completion) = tokio::sync::oneshot::channel();
        {
            let mut tail = persistence.task.lock();
            let previous = tail.take();
            *tail = Some(tokio::spawn(async move {
                if let Some(previous) = previous { let _ = previous.await; }
                let _gate = gate.lock().await;
                // Capture after serialization, and own it through replacement and error publication.
                let document = workspace.document().await;
                let error = diagnostic_operation::scoped(context.as_ref(), async {
                    let mut trace = context.as_ref().map(|context| OperationTrace::new(context.clone(), Component::Persistence));
                    let error = persistence::save(&path, &document).await.err();
                    if let Some(trace) = &mut trace {
                        trace.finish(if error.is_none() { Event::Completed } else { Event::Failed },
                            error.as_ref().map(|error| error.code.into()), DiagnosticDetails::default());
                    }
                    error
                }).await;
                workspace.set_persistence_error(error).await;
                let _ = completed.send(());
            }));
        }
        // Cancellation drops only this wait; the owned save still publishes its result.
        let _ = completion.await;
    }

    /// Returns an owned, coherent view without waiting for pending Git probes.
    pub async fn snapshot(&self) -> WorkspaceSnapshot {
        self.workspace.snapshot().await
    }

    /// Reports picker cancellation without admitting an entry or changing selection.
    pub async fn cancelled(&self) -> OpenOutcome {
        OpenOutcome::Cancelled {
            snapshot: self.snapshot().await,
        }
    }

    /// Reports a sanitized failure alongside the current, unchanged workspace.
    pub async fn rejected(&self, error: GitError) -> OpenOutcome {
        OpenOutcome::Rejected {
            code: error,
            snapshot: self.snapshot().await,
        }
    }

    /// Verifies a native directory twice before admitting or reusing its identity.
    ///
    /// Both passes share one 30-second budget. Admission leaves selection unchanged;
    /// a superseded reopen may reuse the ID without replacing newer entry facts.
    pub async fn open_chosen(&self, selected: &Path) -> OpenOutcome {
        self.trace_operation(OperationKind::OpenRepository, self.open_with_deadline(selected, ProbeDeadline::new())).await
    }

    async fn open_with_deadline(&self, selected: &Path, deadline: ProbeDeadline) -> OpenOutcome {
        let ticket = self.workspace.begin_open().await;
        let initial_facts = match self.probe.probe_with_deadline(selected, deadline).await {
            Ok(facts) => facts,
            Err(error) => return self.rejected(error).await,
        };
        let initial_identity = match NativeIdentity::capture(&initial_facts.root, &initial_facts.git_dir) {
            Ok(identity) => identity,
            Err(error) => return self.rejected(error).await,
        };
        // Recheck identity just before admission, without restarting the operation budget.
        let verified_facts = match self.probe.probe_with_deadline(selected, deadline).await {
            Ok(facts) => facts,
            Err(GitError::NotRepository | GitError::RepositoryChanged | GitError::Unavailable) => {
                return self.rejected(GitError::RepositoryChanged).await;
            }
            Err(error) => return self.rejected(error).await,
        };
        let verified_identity = match NativeIdentity::capture(&verified_facts.root, &verified_facts.git_dir) {
            Ok(identity) => identity,
            Err(error) => return self.rejected(error).await,
        };
        if initial_facts.git_dir != verified_facts.git_dir
            || initial_facts.root != verified_facts.root
            || initial_facts.kind != verified_facts.kind
            || initial_identity != verified_identity
        {
            return self.rejected(GitError::RepositoryChanged).await;
        }
        let outcome = self.workspace.admit(ticket, verified_facts, verified_identity).await;
        match outcome {
            OpenOutcome::Opened { entry_id, .. } => {
                self.save_choices().await;
                OpenOutcome::Opened { entry_id, snapshot: self.snapshot().await }
            }
            OpenOutcome::Reused { entry_id, .. } => {
                {
                    let _selection = self.selection.lock().await;
                    activate_verified_selection(&self.workspace, &self.observation).await;
                }
                self.save_choices().await;
                OpenOutcome::Reused { entry_id, snapshot: self.snapshot().await }
            }
            other => other,
        }
    }

    /// Activates an admitted ID, marks it checking and invalidates its older probes.
    ///
    /// Unknown IDs leave state unchanged. Immediate native scanning starts in the background.
    pub async fn select(&self, entry_id: &str) -> SelectOutcome {
        self.trace_operation(OperationKind::SelectContext, self.select_entry(entry_id)).await
    }

    async fn select_entry(&self, entry_id: &str) -> SelectOutcome {
        let outcome = {
            let _selection = self.selection.lock().await;
            let outcome = self.workspace.select(entry_id).await;
            if matches!(outcome, SelectOutcome::Selected { .. }) {
                if let Some(context) = self.workspace.selected_context(entry_id).await {
                    self.recovery.cancel();
                    self.observation.select(context);
                } else {
                    self.observation.select_unverified(entry_id);
                    self.recovery.start(entry_id.to_owned(), self.probe.clone(), Arc::clone(&self.workspace),
                        Arc::clone(&self.observation), Arc::clone(&self.selection), RecoveryStart::UserSelection);
                }
            }
            outcome
        };
        if matches!(outcome, SelectOutcome::Selected { .. }) {
            self.save_choices().await;
            SelectOutcome::Selected { snapshot: self.snapshot().await }
        } else {
            outcome
        }
    }

    /// Persists an app-only, trimmed nonblank display name without changing the native location.
    pub async fn rename(&self, entry_id: &str, display_name: &str) -> MutationOutcome {
        self.trace_operation(OperationKind::RenameRepository, self.rename_entry(entry_id, display_name)).await
    }

    async fn rename_entry(&self, entry_id: &str, display_name: &str) -> MutationOutcome {
        self.save_mutation(self.workspace.rename(entry_id, display_name).await).await
    }

    /// Removes only the workspace choice and moves an active selection to the first available peer.
    ///
    /// An inactive removal preserves selection; no available replacement leaves it null.
    /// Native repository files are never renamed, removed or otherwise written.
    pub async fn remove(&self, entry_id: &str) -> MutationOutcome {
        self.trace_operation(OperationKind::RemoveRepository, self.remove_entry(entry_id)).await
    }

    async fn remove_entry(&self, entry_id: &str) -> MutationOutcome {
        let outcome = {
            let _selection = self.selection.lock().await;
            let active_before = self.workspace.active_context_id().await;
            let outcome = self.workspace.remove(entry_id).await;
            if matches!(outcome, MutationOutcome::Updated { .. }) {
                self.observation.remove(entry_id);
                if active_before.as_deref() == Some(entry_id) {
                    self.recovery.cancel();
                    if let Some(replacement_id) = self.workspace.active_context_id().await {
                        if let Some(context) = self.workspace.selected_context(&replacement_id).await {
                            self.observation.select(context);
                        }
                    }
                }
            }
            outcome
        };
        self.save_mutation(outcome).await
    }

    async fn save_mutation(&self, outcome: MutationOutcome) -> MutationOutcome {
        match outcome {
            MutationOutcome::Updated { .. } => {
                self.save_choices().await;
                MutationOutcome::Updated { snapshot: self.snapshot().await }
            }
            other => other,
        }
    }

    /// Returns the latest complete snapshot for the selected ID without starting or queueing I/O.
    ///
    /// An unknown or inactive ID is unavailable; the caller cannot use this read to select it.
    pub async fn observe_selected_context(&self, entry_id: &str) -> ObservationSnapshot {
        self.trace_operation(OperationKind::ObserveContext, async {
            let _selection = self.selection.lock().await;
            self.observation.snapshot(entry_id)
        }).await
    }

    /// Reviews only the selected context's current path token and category.
    ///
    /// No renderer path or Git revision is accepted. Selection and observation are
    /// rechecked after I/O, including same-entry reselection, before returning payload.
    pub async fn review_file(&self, entry_id: &str, observation_revision: u64, path_id: &str, category: ReviewCategory) -> ReviewResult {
        self.trace_operation(OperationKind::ReviewFile, async {
            let (context, generation, path) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return ReviewResult::StaleSelection;
                };
                if context.kind != crate::git::RepositoryKind::WorkingTree { return ReviewResult::StaleSelection; }
                let (generation, path) = match self.observation.authorize_review(entry_id, observation_revision, path_id, category) {
                    Ok(authority) => authority,
                    Err(stale) => return stale,
                };
                (context, generation, path)
            };
            let identity = diff::identity(&context, path_id, &path, category);
            // Keep the large Git-probe state machine off nested IPC/task-local future frames.
            let result = Box::pin(diff::read_review(&self.inspection_process, &context, &path, identity)).await;
            let _selection = self.selection.lock().await;
            let Some(current) = self.workspace.selected_context(entry_id).await else {
                return ReviewResult::StaleSelection;
            };
            if current.root != context.root || current.git_dir != context.git_dir || current.identity != context.identity {
                return ReviewResult::StaleSelection;
            }
            match self.observation.validate_review(generation, entry_id, observation_revision, path_id, category) {
                Ok(()) => result,
                Err(stale) => stale,
            }
        }).await
    }

    /// Reads a pinned graph page; a null cursor explicitly refreshes its snapshot.
    ///
    /// A known local branch name pins only that branch's ancestry; null preserves all-ref history.
    /// Continuations require the same branch choice and retain their captured full object IDs.
    /// Cursors are native authority, independent of live file observation revisions.
    pub async fn history_page(&self, entry_id: &str, cursor: Option<&str>, branch: Option<&str>) -> HistoryPageResult {
        self.trace_operation(OperationKind::HistoryPage, async {
            let (context, generation, ticket) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return HistoryErrorCode::StaleSelection.result();
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return HistoryErrorCode::StaleSelection.result();
                };
                let ticket = match self.history.begin(entry_id, generation, cursor, branch) {
                    Ok(ticket) => ticket,
                    Err(code) => return code.result(),
                };
                (context, generation, ticket)
            };
            let candidate = history::read_page(&self.inspection_process, &context, &ticket).await;
            let _selection = self.selection.lock().await;
            let Some(current) = self.workspace.selected_context(entry_id).await else {
                return HistoryErrorCode::StaleSelection.result();
            };
            if current.root != context.root || current.git_dir != context.git_dir || current.identity != context.identity
                || self.observation.selection_generation(entry_id) != Some(generation) {
                return HistoryErrorCode::StaleSelection.result();
            }
            match candidate {
                Ok(candidate) => self.history.publish(ticket, candidate),
                Err(code) => code.result(),
            }
        }).await
    }

    /// Lists locally known branches and available native worktrees without admitting or checking out.
    pub async fn list_contexts(&self, entry_id: &str) -> ContextOptionsResult {
        self.trace_operation(OperationKind::ListContexts, async {
            let (context, generation, epoch) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return ContextOptionsResult::failure(HistoryErrorCode::StaleSelection);
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return ContextOptionsResult::failure(HistoryErrorCode::StaleSelection);
                };
                (context, generation, self.contexts.begin())
            };
            let result = Box::pin(inspection::read_contexts(&self.inspection_process, &self.probe, &context)).await;
            let _selection = self.selection.lock().await;
            if !self.inspection_current(&context, generation).await {
                return ContextOptionsResult::failure(HistoryErrorCode::StaleSelection);
            }
            match result {
                Ok((branches, worktrees)) => self.contexts.publish(epoch, entry_id, generation, branches, worktrees),
                Err(code) => ContextOptionsResult::failure(code),
            }
        }).await
    }

    async fn inspection_current(&self, context: &crate::workspace::SelectedContext, generation: u64) -> bool {
        self.workspace.selected_context(&context.entry_id).await.is_some_and(|current| {
            current.root == context.root && current.git_dir == context.git_dir && current.identity == context.identity
                && self.observation.selection_generation(&context.entry_id) == Some(generation)
        })
    }

    /// Reads one native-issued directory page; refreshes retain bounded older file authority.
    pub async fn list_repository_files(&self, entry_id: &str, request: RepositoryFilesRequest) -> RepositoryFilesResult {
        self.trace_operation(OperationKind::ListRepositoryFiles, async {
            let (context, generation) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return RepositoryFilesResult::StaleSelection;
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return RepositoryFilesResult::StaleSelection;
                };
                (context, generation)
            };
            let candidate = Box::pin(self.browsing.read_page(&self.inspection_process, &context, generation, request)).await;
            let _selection = self.selection.lock().await;
            if !self.inspection_current(&context, generation).await {
                drop(_selection);
                return RepositoryFilesResult::StaleSelection;
            }
            match candidate {
                Ok(page) => self.browsing.publish(page),
                Err(code) => RepositoryFilesResult::failure(code),
            }
        }).await
    }

    /// Reads an issued file ID from its original listing without accepting display paths.
    pub async fn review_repository_file(&self, entry_id: &str, listing_id: &str, file_id: &str) -> RepositoryFileResult {
        self.trace_operation(OperationKind::ReviewRepositoryFile, async {
            let deadline = ProbeDeadline::new();
            let (context, generation) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return RepositoryFileResult::StaleSelection;
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return RepositoryFileResult::StaleSelection;
                };
                (context, generation)
            };
            let result = match self.browsing.resolve(&context, generation, listing_id, file_id, deadline).await {
                Ok(native) => Box::pin(browsing::read_file(&self.inspection_process, &context, listing_id, native, deadline)).await,
                Err(crate::history::HistoryErrorCode::StaleSelection) => RepositoryFileResult::StaleSelection,
                Err(code) => browsing::review_error(code).into(),
            };
            let _selection = self.selection.lock().await;
            if !self.inspection_current(&context, generation).await
                || !self.browsing.current(&context, generation, listing_id) {
                return RepositoryFileResult::StaleSelection;
            }
            result
        }).await
    }

    /// Resolves only an issued native worktree ID, verifies its identity, then admits and selects it.
    /// Later selections supersede an in-flight admission; no lock is held during native I/O.
    pub async fn select_worktree(&self, entry_id: &str, worktree_id: &str) -> MutationOutcome {
        self.trace_operation(OperationKind::SelectWorktree, async {
            let (context, generation, epoch, target, ticket) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return MutationOutcome::NotFound { snapshot: self.snapshot().await };
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return MutationOutcome::NotFound { snapshot: self.snapshot().await };
                };
                let Some((epoch, target)) = self.contexts.resolve(entry_id, generation, worktree_id) else {
                    return MutationOutcome::NotFound { snapshot: self.snapshot().await };
                };
                (context, generation, epoch, target, self.workspace.begin_open().await)
            };
            let deadline = ProbeDeadline::new();
            let verified = Box::pin(async {
                history::reader::verify_context(&self.inspection_process, &context, deadline).await?;
                let target_context = crate::workspace::SelectedContext {
                    entry_id: String::new(), root: target.root.clone(), git_dir: target.git_dir.clone(),
                    kind: target.kind, identity: target.identity.clone(),
                };
                history::reader::verify_context(&self.inspection_process, &target_context, deadline).await?;
                let facts = self.probe.probe_with_deadline(&target.root, deadline).await.map_err(|_| HistoryErrorCode::Inaccessible)?;
                let identity = NativeIdentity::capture(&facts.root, &facts.git_dir).map_err(|_| HistoryErrorCode::Inaccessible)?;
                if facts.root != target.root || facts.git_dir != target.git_dir || facts.kind != target.kind || identity != target.identity {
                    return Err(HistoryErrorCode::Inaccessible);
                }
                history::reader::verify_context(&self.inspection_process, &target_context, deadline).await?;
                history::reader::verify_context(&self.inspection_process, &context, deadline).await?;
                Ok((facts, identity))
            }).await;
            let outcome = {
                let _selection = self.selection.lock().await;
                if !self.inspection_current(&context, generation).await || !self.contexts.selection_current(epoch) {
                    return MutationOutcome::Rejected { code: WorkspaceRejectionCode::RepositoryChanged, snapshot: self.snapshot().await };
                }
                let (facts, identity) = match verified {
                    Ok(verified) => verified,
                    Err(_) => return MutationOutcome::Rejected { code: WorkspaceRejectionCode::RepositoryUnavailable, snapshot: self.snapshot().await },
                };
                let admitted = self.workspace.admit(ticket, facts, identity).await;
                match admitted {
                    OpenOutcome::Opened { entry_id, .. } | OpenOutcome::Reused { entry_id, .. } => {
                        let selected = self.workspace.select(&entry_id).await;
                        if matches!(selected, SelectOutcome::Selected { .. }) {
                            self.recovery.cancel();
                            if let Some(context) = self.workspace.selected_context(&entry_id).await { self.observation.select(context); }
                            MutationOutcome::Updated { snapshot: self.snapshot().await }
                        } else { MutationOutcome::NotFound { snapshot: self.snapshot().await } }
                    }
                    OpenOutcome::Rejected { code, .. } => MutationOutcome::Rejected { code: code.into(), snapshot: self.snapshot().await },
                    OpenOutcome::Cancelled { .. } => MutationOutcome::Rejected { code: WorkspaceRejectionCode::SupersededSelection, snapshot: self.snapshot().await },
                }
            };
            self.save_mutation(outcome).await
        }).await
    }

    /// Reads actual changed files against a verified raw parent (first parent by default).
    pub async fn commit_files(&self, entry_id: &str, commit_oid: &str, parent_oid: Option<&str>) -> CommitFilesResult {
        self.trace_operation(OperationKind::CommitFiles, async {
            let (context, generation) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return CommitFilesResult::failure(HistoryErrorCode::StaleSelection);
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return CommitFilesResult::failure(HistoryErrorCode::StaleSelection);
                };
                (context, generation)
            };
            let result = Box::pin(inspection::read_commit_files(&self.inspection_process, &context, commit_oid, parent_oid)).await;
            let _selection = self.selection.lock().await;
            if !self.inspection_current(&context, generation).await {
                return CommitFilesResult::failure(HistoryErrorCode::StaleSelection);
            }
            self.committed_reviews.publish(entry_id, generation, result)
        }).await
    }

    /// Reviews an issued committed-file ID against its exact verified raw parent.
    /// Live status availability cannot block immutable object reads; reselection invalidates IDs.
    pub async fn review_commit_file(&self, entry_id: &str, commit_oid: &str, parent_oid: Option<&str>, file_id: &str) -> CommitReviewResult {
        self.trace_operation(OperationKind::ReviewCommitFile, async {
            let (context, generation, identity, kind) = {
                let _selection = self.selection.lock().await;
                let Some(context) = self.workspace.selected_context(entry_id).await else {
                    return CommitReviewResult::StaleSelection;
                };
                let Some(generation) = self.observation.selection_generation(entry_id) else {
                    return CommitReviewResult::StaleSelection;
                };
                let Some((identity, kind)) = self.committed_reviews.resolve(&context, generation, commit_oid, parent_oid, file_id) else {
                    return CommitReviewResult::StaleSelection;
                };
                (context, generation, identity, kind)
            };
            let result = Box::pin(committed::read_review(&self.inspection_process, &context, identity, kind)).await;
            let _selection = self.selection.lock().await;
            if !self.inspection_current(&context, generation).await
                || self.committed_reviews.resolve(&context, generation, commit_oid, parent_oid, file_id).is_none() {
                return CommitReviewResult::StaleSelection;
            }
            result
        }).await
    }

    #[cfg(test)]
    pub(crate) fn with_status_reader(reader: crate::git::status::GitStatusReader) -> Self {
        Self { observation: Arc::new(ObservationController::with_reader(reader)), ..Self::default() }
    }

    #[cfg(test)]
    pub(crate) fn with_inspection_executable(mut self, executable: &Path) -> Self {
        self.inspection_process = GitProcess::with_executable(executable);
        self
    }

    /// Checks the stored native root and applies facts only while its ticket is current.
    ///
    /// Unknown IDs return the current snapshot. Cancelling after the checking transition
    /// leaves it checking until a later reopen or refresh supplies an outcome.
    pub async fn refresh(&self, entry_id: &str) -> WorkspaceSnapshot {
        let context = diagnostic_operation::context(&self.diagnostics, OperationKind::RefreshAvailability);
        diagnostic_operation::scoped(context.as_ref(), self.refresh_entry(entry_id)).await
    }

    async fn refresh_entry(&self, entry_id: &str) -> WorkspaceSnapshot {
        let Some(ticket) = self.workspace.begin_refresh(entry_id).await else {
            if let Some(context) = OperationContext::current() {
                let mut trace = OperationTrace::new(context, Component::Application);
                trace.finish(Event::Completed, None, DiagnosticDetails::default());
            }
            return self.snapshot().await;
        };
        // Paths come exclusively from native state, never from refresh command arguments.
        recheck(&self.probe, &self.workspace, ticket).await;
        let _selection = self.selection.lock().await;
        activate_verified_selection(&self.workspace, &self.observation).await;
        self.snapshot().await
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/integration/application.rs"]
mod integration_tests;
