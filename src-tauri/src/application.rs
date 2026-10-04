//! Coordinates repository admission, private workspace persistence and background restoration.
//!
//! Workspace tickets order completions; no Git or storage I/O holds state/selection locks,
//! and background tasks own shared state rather than the service itself.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::companion::{self, AckReviewHandoffResult, BeginCompanionReviewResult, ClaimReviewHandoffResult, CompanionCode, HandoffOutcome, HandoffSelection, PendingReviewHandoffSnapshot, PresentationInput, PresentationSnapshot, RequestReviewHandoff, ReviewCaller, ReviewHandoffResult, ReviewHandoffTarget, ReviewSelection, ReviewSurfaceSnapshot, SurfaceListener, SurfaceScope};
use crate::diagnostic_operation::{self, DiagnosticOutcome, OperationTrace};
use crate::diagnostics::{Component, DiagnosticDetails, DiagnosticSink, Event, OperationContext, OperationKind};
use crate::browsing::{self, BrowsingController, RepositoryFileResult, RepositoryFilesRequest, RepositoryFilesResult};
use crate::diff::committed::{self, CommitReviewController, CommitReviewResult};
use crate::diff::{self, ReviewCategory, ReviewResult};
use crate::git::{GitError, GitProbe};
use crate::history::{self, HistoryController, HistoryErrorCode, HistoryPageResult};
use crate::inspection::{self, CommitFilesResult, ContextController, ContextOptionsResult};
use crate::observation::{ObservationController, ObservationSnapshot};
use crate::native_work::{NativeWork, WorkPermit};
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
    native_work: NativeWork,
    shutdown: tokio::sync::Mutex<bool>,
    handoff_preparation: tokio::sync::Mutex<()>,
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
    demand_suspended: std::sync::atomic::AtomicBool,
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

    fn cancel_entry(&self, entry_id: &str) {
        let mut current = self.task.lock();
        if current.as_ref().is_some_and(|(id, _)| id == entry_id) {
            if let Some((_, task)) = current.take() { task.abort(); }
        }
    }

    fn start(
        &self, entry_id: String, probe: GitProbe, workspace: Arc<WorkspaceStore>,
        observation: Arc<ObservationController>, selection: Arc<tokio::sync::Mutex<()>>,
        reason: RecoveryStart,
    ) {
        let mut current = self.task.lock();
        if self.demand_suspended.load(std::sync::atomic::Ordering::SeqCst) { return; }
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
        let task = tokio::spawn(crate::native_work::inherit(async move {
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
                            observation.select(context, workspace.review.context_epoch());
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
        }));
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
                observation.select(context, workspace.review.context_epoch());
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
            native_work: NativeWork::default(),
            shutdown: tokio::sync::Mutex::new(false),
            handoff_preparation: tokio::sync::Mutex::new(()),
        }
    }

    pub(crate) fn diagnostic_sink(&self) -> DiagnosticSink { self.diagnostics.clone() }

    async fn trace_operation<T: DiagnosticOutcome>(
        &self, kind: OperationKind, future: impl Future<Output = T>,
    ) -> T {
        let context = diagnostic_operation::context(&self.diagnostics, kind);
        self.native_work.scope(diagnostic_operation::scoped(context.as_ref(), async {
            let mut trace = context.as_ref().map(|context| OperationTrace::new(context.clone(), Component::Application));
            let outcome = future.await;
            if let Some(trace) = &mut trace { trace.outcome(&outcome); }
            outcome
        })).await
    }

    pub(crate) fn begin_shutdown(&self) { self.native_work.close(); }

    pub(crate) fn is_shutting_down(&self) -> bool { self.native_work.is_closing() }

    pub(crate) fn admit_native_request(&self) -> Result<WorkPermit, &'static str> {
        self.native_work.admit()
    }


    /// Supplies effective native companion access, not the saved preference alone.
    pub fn set_companion_available(&self, available: bool) {
        self.workspace.review.set_available(available);
        self.update_surface_demand();
    }

    /// Must run synchronously before a native hide; cancellation does not wait for a runtime task.
    pub fn set_surface_visibility(&self, caller: ReviewCaller, visible: bool) -> String {
        let epoch = self.workspace.review.set_visibility(caller, visible);
        self.update_surface_demand();
        epoch
    }

    fn update_surface_demand(&self) {
        let demand = self.workspace.review.demand();
        self.observation.set_demand(demand);
        self.recovery.demand_suspended.store(!demand, std::sync::atomic::Ordering::SeqCst);
        if !demand { self.recovery.cancel(); }
    }

    pub fn surface_open_epoch(&self, caller: ReviewCaller) -> String { self.workspace.review.open_epoch(caller) }
    pub fn capture_surface_scope(&self, caller: ReviewCaller) -> Result<SurfaceScope, CompanionCode> { self.workspace.review.capture(caller) }
    pub fn surface_scope_is_current(&self, scope: &SurfaceScope) -> bool { self.workspace.review.is_current(scope) }
    pub fn subscribe_review_surface(&self, caller: ReviewCaller, listener: SurfaceListener) { self.workspace.review.subscribe(caller, Some(listener)); }
    pub fn unsubscribe_review_surface(&self, caller: ReviewCaller) { self.workspace.review.subscribe(caller, None); }

    /// Coalesces both visible surfaces into the existing selected-context producer.
    pub async fn reconcile_surface_demand(&self) {
        let _selection = self.selection.lock().await;
        self.update_surface_demand();
        if !self.workspace.review.demand() { return; }
        activate_verified_selection(&self.workspace, &self.observation).await;
        if let Some(entry_id) = self.workspace.selected_pending_refresh_id().await {
            self.recovery.start(entry_id, self.probe.clone(), Arc::clone(&self.workspace),
                Arc::clone(&self.observation), Arc::clone(&self.selection), RecoveryStart::AfterRestoreFailure);
        }
    }

    /// Drops in-flight native work on invalidation and validates again before publishing its reply.
    pub async fn run_surface_request<T>(&self, scope: &SurfaceScope, future: impl Future<Output = T>) -> Result<T, CompanionCode> {
        self.workspace.review.validate(scope, true)?;
        let result = tokio::select! {
            biased;
            _ = scope.cancelled() => return Err(self.workspace.review.validate(scope, true).err().unwrap_or(CompanionCode::StaleSurface)),
            result = self.native_work.scope(future) => result,
        };
        self.workspace.review.validate(scope, true)?;
        Ok(result)
    }

    /// Selection invalidates its own context scope, but never its caller's native visibility scope.
    pub async fn select_context_for_surface(&self, caller: ReviewCaller, entry_id: &str) -> Result<SelectOutcome, CompanionCode> {
        let scope = self.capture_surface_scope(caller)?;
        let outcome = self.trace_operation(OperationKind::SelectContext, self.select_entry(entry_id, Some(&scope))).await;
        self.workspace.review.validate(&scope, false)?;
        Ok(outcome)
    }

    pub async fn review_surface_snapshot(&self, caller: ReviewCaller) -> ReviewSurfaceSnapshot {
        let _selection = self.selection.lock().await;
        let workspace = self.workspace.snapshot().await;
        let observation = workspace.active_context_id.as_deref().and_then(|entry_id| self.observation.surface_snapshot(entry_id, &workspace.context_epoch));
        self.workspace.review.surface_snapshot(caller, workspace, observation)
    }

    pub fn publish_companion_presentation(&self, presentation: PresentationInput) -> Result<PresentationSnapshot, CompanionCode> {
        self.workspace.review.presentation(presentation)
    }

    pub async fn begin_companion_review(&self, open_epoch: &str) -> BeginCompanionReviewResult {
        let scope = match self.capture_surface_scope(ReviewCaller::Companion) {
            Ok(scope) if scope.open_epoch == open_epoch => scope,
            Ok(_) => return BeginCompanionReviewResult::Stale { code: CompanionCode::StaleSurface },
            Err(code) => return BeginCompanionReviewResult::Unavailable { code },
        };
        let operation = async {
            self.reconcile_surface_demand().await;
            let workspace = self.workspace.snapshot().await;
            if let Some(entry_id) = workspace.active_context_id.as_deref() {
                let Some(ticket) = self.observation.fresh_scan_after_recovery(entry_id, &scope.context_epoch).await else {
                    return Err(CompanionCode::Unavailable);
                };
                if !self.observation.await_fresh_scan(ticket).await { return Err(CompanionCode::StaleContext); }
            }
            Ok(self.review_surface_snapshot(ReviewCaller::Companion).await)
        };
        match tokio::time::timeout_at(ProbeDeadline::new().instant(), self.run_surface_request(&scope, operation)).await {
            Ok(Ok(Ok(surface))) => BeginCompanionReviewResult::Ready { surface },
            Ok(Err(code)) | Ok(Ok(Err(code))) => match code {
                CompanionCode::StaleSurface | CompanionCode::StaleContext | CompanionCode::NotVisible => BeginCompanionReviewResult::Stale { code },
                _ => BeginCompanionReviewResult::Unavailable { code },
            },
            Err(_) => BeginCompanionReviewResult::Unavailable { code: CompanionCode::Unavailable },
        }
    }

    pub async fn review_file_for_surface(&self, scope: &SurfaceScope, entry_id: &str, revision: u64, path_id: &str, category: ReviewCategory) -> Result<ReviewResult, CompanionCode> {
        self.run_surface_request(scope, async {
            let issued = self.observation.review_provenance(entry_id, revision, path_id, category);
            let result = self.review_file(entry_id, revision, path_id, category).await;
            if matches!(result, ReviewResult::Text { .. } | ReviewResult::Unsupported { .. }) {
                if let Some(issued) = issued { self.workspace.review.record_review(scope, issued); }
            }
            result
        }).await
    }

    pub fn pending_review_handoff(&self) -> PendingReviewHandoffSnapshot { self.workspace.review.pending() }
    pub fn claim_review_handoff(&self, request_id: &str, context_epoch: &str) -> ClaimReviewHandoffResult { self.workspace.review.claim(request_id, context_epoch) }
    pub fn ack_review_handoff(&self, request_id: &str, context_epoch: &str, outcome: HandoffOutcome) -> AckReviewHandoffResult { self.workspace.review.ack(request_id, context_epoch, outcome) }

    /// Validates native provenance, reveals the existing main surface, then awaits exact claimed delivery.
    pub async fn request_review_handoff<F, Fut>(&self, request: RequestReviewHandoff, reveal: F) -> ReviewHandoffResult
    where F: FnOnce(String, String) -> Fut, Fut: Future<Output = Result<(), CompanionCode>> {
        let scope = match self.capture_surface_scope(ReviewCaller::Companion) {
            Ok(scope) => scope,
            Err(code) => return ReviewHandoffResult::Failed { code },
        };
        if scope.open_epoch != request.open_epoch { return ReviewHandoffResult::Failed { code: CompanionCode::StaleSurface }; }
        if scope.context_epoch != request.context_epoch { return ReviewHandoffResult::Failed { code: CompanionCode::StaleContext }; }
        let prepared = tokio::time::timeout_at(ProbeDeadline::new().instant(), self.run_surface_request(&scope, async {
            let _preparation = self.handoff_preparation.lock().await;
            if self.pending_review_handoff().pending.is_some_and(|pending| pending.phase == companion::HandoffPhase::Claimed) { return Err(CompanionCode::Busy); }
            let workspace = self.workspace.snapshot().await;
            let target = match request.selection {
                Some(selection) => Some(self.validate_handoff_target(&scope, &selection).await?),
                None => None,
            };
            self.workspace.review.create_handoff(&scope, workspace.active_context_id, target)
        })).await;
        let (pending, completion) = match prepared {
            Ok(Ok(Ok(delivery))) => delivery,
            Ok(Ok(Err(code))) | Ok(Err(code)) => return ReviewHandoffResult::Failed { code },
            Err(_) => return ReviewHandoffResult::Failed { code: CompanionCode::Unavailable },
        };
        let deadline = tokio::time::Instant::now() + companion::HANDOFF_TIMEOUT;
        let delivery = async {
            if let Err(code) = reveal(pending.request_id.clone(), pending.source_open_epoch.clone()).await {
                self.workspace.review.fail(&pending.request_id, code);
                return ReviewHandoffResult::Failed { code };
            }
            completion.await.unwrap_or(ReviewHandoffResult::Failed { code: CompanionCode::Unavailable })
        };
        match tokio::time::timeout_at(deadline, delivery).await {
            Ok(result) => result,
            Err(_) => {
                self.workspace.review.fail(&pending.request_id, CompanionCode::DeliveryTimeout);
                ReviewHandoffResult::Failed { code: CompanionCode::DeliveryTimeout }
            }
        }
    }

    async fn validate_handoff_target(&self, scope: &SurfaceScope, selection: &HandoffSelection) -> Result<ReviewHandoffTarget, CompanionCode> {
        let path = self.observation.handoff_authority(selection)
            .or_else(|| self.workspace.review.issued_review(scope, selection).map(|issued| issued.path))
            .ok_or(CompanionCode::InvalidRequest)?;
        let context = self.workspace.selected_context(&selection.entry_id).await.ok_or(CompanionCode::StaleContext)?;
        let ticket = self.observation.request_fresh_scan(&selection.entry_id).ok_or(CompanionCode::Unavailable)?;
        let fresh = tokio::time::timeout_at(ProbeDeadline::new().instant(), self.observation.await_fresh_scan(ticket)).await;
        if !matches!(fresh, Ok(true)) { return Err(CompanionCode::Unavailable); }
        self.workspace.review.validate(scope, true)?;
        let ObservationSnapshot::Ready { files, observation_revision, .. } = self.observation.snapshot(&selection.entry_id) else {
            return Ok(ReviewHandoffTarget::Unavailable { entry_id: selection.entry_id.clone(), code: CompanionCode::Unavailable });
        };
        let review_selection = ReviewSelection { stable_path_id: selection.stable_path_id.clone(), category: selection.category, display_path: path.display_path.clone() };
        if let Some(file) = files.iter().find(|file| file.stable_path_id == selection.stable_path_id) {
            let current = HandoffSelection { entry_id: selection.entry_id.clone(), stable_path_id: selection.stable_path_id.clone(), observation_revision, path_id: file.path_id.clone(), category: selection.category };
            if self.observation.handoff_authority(&current).is_some() {
                return Ok(ReviewHandoffTarget::Live { entry_id: selection.entry_id.clone(), selection: review_selection, path_id: file.path_id.clone(), observation_revision });
            }
            return Ok(ReviewHandoffTarget::NoRemaining { entry_id: selection.entry_id.clone(), selection: review_selection });
        }
        let native_path = path.native_path;
        let survives = crate::native_work::spawn_blocking(move || {
            if NativeIdentity::capture(&context.root, &context.git_dir).ok().as_ref() != Some(&context.identity) { return false; }
            let mut at = context.root.clone();
            for component in native_path.components() {
                if !matches!(component, std::path::Component::Normal(_)) { return false; }
                at.push(component);
                if std::fs::symlink_metadata(&at).map_or(true, |metadata| metadata.file_type().is_symlink()) { return false; }
            }
            std::fs::metadata(&at).is_ok_and(|metadata| metadata.is_file())
                && NativeIdentity::capture(&context.root, &context.git_dir).ok().as_ref() == Some(&context.identity)
        }).await.unwrap_or(false);
        self.workspace.review.validate(scope, true)?;
        Ok(if survives {
            ReviewHandoffTarget::NoRemaining { entry_id: selection.entry_id.clone(), selection: review_selection }
        } else {
            ReviewHandoffTarget::Unavailable { entry_id: selection.entry_id.clone(), code: CompanionCode::Unavailable }
        })
    }
    /// Seals native admission, cancels requests/producers, and awaits cleanup before diagnostic drain.
    /// Accepted choice writes finish even if their requester cancelled. Repeated callers share the drain.
    pub async fn shutdown(&self) {
        self.begin_shutdown();
        let mut completed = self.shutdown.lock().await;
        if *completed { return; }
        self.native_work.drain().await;
        let restoration = self.restoration.0.lock().take();
        let recovery = self.recovery.task.lock().take().map(|(_, task)| task);
        let observation = self.observation.stop();
        let tasks = [restoration, recovery, observation];
        for task in tasks.iter().flatten() { task.abort(); }
        for task in tasks.into_iter().flatten() { let _ = task.await; }
        // Cancellation can delegate a final child wait; retain the runtime through that cleanup.
        self.native_work.drain().await;
        if let Some(persistence) = &self.persistence {
            // Poll in place: cancelling a shutdown waiter must not detach the accepted save tail.
            std::future::poll_fn(|context| {
                let mut stored = persistence.task.lock();
                let Some(task) = stored.as_mut() else { return std::task::Poll::Ready(()); };
                match std::pin::Pin::new(task).poll(context) {
                    std::task::Poll::Pending => std::task::Poll::Pending,
                    std::task::Poll::Ready(_) => { stored.take(); std::task::Poll::Ready(()) },
                }
            }).await;
        }
        *completed = true;
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
            service.observation.select_unverified(&entry_id, service.workspace.review.context_epoch());
        }
        let workspace = Arc::clone(&service.workspace);
        let observation = Arc::clone(&service.observation);
        let selection = Arc::clone(&service.selection);
        let recovery = Arc::clone(&service.recovery);
        let probe = service.probe.clone();
        let native_work = service.native_work.clone();
        let task = tokio::spawn(async move { let _ = native_work.run(async {
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
        }).await; });
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
        self.trace_operation(OperationKind::SelectContext, self.select_entry(entry_id, None)).await
    }

    async fn select_entry(&self, entry_id: &str, scope: Option<&SurfaceScope>) -> SelectOutcome {
        let outcome = {
            let _selection = self.selection.lock().await;
            let outcome = self.workspace.select(entry_id, scope, false).await;
            if matches!(outcome, SelectOutcome::Selected { .. }) {
                self.recovery.cancel();
                if let Some(context) = self.workspace.selected_context(entry_id).await {
                    self.observation.select(context, self.workspace.review.context_epoch());
                } else {
                    self.observation.select_unverified(entry_id, self.workspace.review.context_epoch());
                }
                self.recovery.start(entry_id.to_owned(), self.probe.clone(), Arc::clone(&self.workspace),
                    Arc::clone(&self.observation), Arc::clone(&self.selection), RecoveryStart::UserSelection);
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
            let outcome = self.workspace.remove(entry_id).await;
            if matches!(outcome, MutationOutcome::Updated { .. }) {
                self.observation.remove(entry_id);
                self.recovery.cancel();
                if let Some(replacement_id) = self.workspace.active_context_id().await {
                    if let Some(context) = self.workspace.selected_context(&replacement_id).await {
                        self.observation.select(context, self.workspace.review.context_epoch());
                    } else {
                        self.observation.select_unverified(&replacement_id, self.workspace.review.context_epoch());
                    }
                    self.recovery.start(replacement_id, self.probe.clone(), Arc::clone(&self.workspace),
                        Arc::clone(&self.observation), Arc::clone(&self.selection), RecoveryStart::UserSelection);
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
                        let selected = self.workspace.select(&entry_id, None, true).await;
                        if matches!(selected, SelectOutcome::Selected { .. }) {
                            self.recovery.cancel();
                            if let Some(context) = self.workspace.selected_context(&entry_id).await { self.observation.select(context, self.workspace.review.context_epoch()); }
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
        self.native_work.scope(diagnostic_operation::scoped(context.as_ref(), self.refresh_entry(entry_id))).await
    }

    async fn refresh_entry(&self, entry_id: &str) -> WorkspaceSnapshot {
        let ticket = {
            let _selection = self.selection.lock().await;
            let Some(ticket) = self.workspace.begin_refresh(entry_id).await else {
                if let Some(context) = OperationContext::current() {
                    let mut trace = OperationTrace::new(context, Component::Application);
                    trace.finish(Event::Completed, None, DiagnosticDetails::default());
                }
                return self.snapshot().await;
            };
            if ticket.verified.is_some() { self.recovery.cancel_entry(entry_id); }
            ticket
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
