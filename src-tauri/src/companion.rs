//! Owns surface epochs, bounded review provenance and acknowledged handoff state without host APIs.

use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;
use crate::diff::ReviewCategory;
use crate::git::status::StatusPath;
use crate::locale::Locale;
use crate::observation::ObservationSnapshot;
use crate::workspace::WorkspaceSnapshot;

pub(crate) const HANDOFF_TIMEOUT: Duration = Duration::from_secs(5);

/// This value is derived from the invoking native webview, never a payload field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCaller { Main, Companion }
impl ReviewCaller { fn index(self) -> usize { match self { Self::Main => 0, Self::Companion => 1 } } }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanionCode { Disabled, NotVisible, StaleSurface, StaleContext, Unavailable, WindowUnavailable, DeliveryTimeout, Busy, InvalidRequest }
impl CompanionCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled", Self::NotVisible => "not_visible", Self::StaleSurface => "stale_surface",
            Self::StaleContext => "stale_context", Self::Unavailable => "unavailable", Self::WindowUnavailable => "window_unavailable",
            Self::DeliveryTimeout => "delivery_timeout", Self::Busy => "busy", Self::InvalidRequest => "invalid_request",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SurfaceNotice {
    Invalidate { workspace_revision: u64, context_epoch: String, open_epoch: String },
    Visibility { visible: bool, open_epoch: String },
    Presentation { revision: u64 },
    Handoff { revision: u64, request_id: Option<String> },
}
pub type SurfaceListener = Arc<dyn Fn(SurfaceNotice) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceTheme { Cream, Paper, Mist, Stone, Graphite, Midnight }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IconTheme { Classic, Material, Catppuccin }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodeTheme { Match, Plain, GithubLight, CatppuccinLatte, SolarizedLight, GithubDark }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode { Changes, Full }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineMode { Scroll, Wrap }
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewPresentation { pub mode: ReviewMode, pub theme: CodeTheme, pub line_mode: LineMode }
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuLabels { pub open_git_view: String, pub quit: String }
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresentationInput {
    pub locale: Locale, pub appearance_theme: AppearanceTheme, pub icon_theme: IconTheme,
    pub review: ReviewPresentation, pub persistence_error: bool, pub menu_labels: MenuLabels,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresentationSnapshot {
    pub revision: u64, pub locale: Locale, pub appearance_theme: AppearanceTheme, pub icon_theme: IconTheme,
    pub review: ReviewPresentation, pub persistence_error: bool, pub menu_labels: MenuLabels,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceHandoff { pub revision: u64, pub pending_request_id: Option<String> }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSurfaceSnapshot {
    pub workspace: WorkspaceSnapshot, pub visible: bool, pub open_epoch: String,
    pub observation: Option<ObservationSnapshot>, pub presentation: Option<PresentationSnapshot>, pub handoff: SurfaceHandoff,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BeginCompanionReviewResult { Ready { surface: ReviewSurfaceSnapshot }, Unavailable { code: CompanionCode }, Stale { code: CompanionCode } }
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffSelection {
    pub entry_id: String, pub stable_path_id: String, pub observation_revision: u64, pub path_id: String, pub category: ReviewCategory,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestReviewHandoff { pub open_epoch: String, pub context_epoch: String, pub selection: Option<HandoffSelection> }
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffOutcome { Applied, Changed, Unavailable }
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ReviewHandoffResult { Applied { request_id: String }, Changed { request_id: String }, Unavailable { request_id: String }, Failed { code: CompanionCode } }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSelection { pub stable_path_id: String, pub category: ReviewCategory, pub display_path: String }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ReviewHandoffTarget {
    Live { entry_id: String, selection: ReviewSelection, path_id: String, observation_revision: u64 },
    NoRemaining { entry_id: String, selection: ReviewSelection },
    Unavailable { entry_id: String, code: CompanionCode },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoffPhase { Pending, Claimed }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReviewHandoff {
    pub request_id: String, pub context_epoch: String, pub entry_id: Option<String>, pub target: Option<ReviewHandoffTarget>,
    pub source_open_epoch: String, pub phase: HandoffPhase,
}
#[derive(Clone, Debug, Serialize)]
pub struct PendingReviewHandoffSnapshot { pub revision: u64, pub pending: Option<PendingReviewHandoff> }
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ClaimReviewHandoffResult {
    Claimed { request_id: String, context_epoch: String, handoff_revision: u64, target: Option<ReviewHandoffTarget>, remaining_ms: u64 },
    Stale { code: CompanionCode }, Busy { code: CompanionCode },
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReviewHandoffResult { Applied, Stale { code: CompanionCode } }

/// Native admission facts plus a cancellation subscription; no renderer can construct authority from it.
#[derive(Clone, Debug)]
pub struct SurfaceScope {
    pub caller: ReviewCaller, pub context_epoch: String, pub open_epoch: String,
    cancellation: watch::Receiver<u64>,
}
impl SurfaceScope { pub(crate) async fn cancelled(&self) { let mut receiver = self.cancellation.clone(); let _ = receiver.changed().await; } }
#[derive(Clone)]
pub(crate) struct IssuedReview { pub selection: HandoffSelection, pub path: StatusPath }
struct Surface {
    visible: bool, generation: u64, cancellation: watch::Sender<u64>, listener: Option<SurfaceListener>, last_review: Option<IssuedReview>,
}
impl Surface {
    fn new(visible: bool) -> Self { Self { visible, generation: 0, cancellation: watch::channel(0).0, listener: None, last_review: None } }
    fn epoch(&self) -> String { format!("open-{}", self.generation) }
    fn cancel(&mut self) { self.cancellation.send_modify(|generation| *generation += 1); }
}
struct Delivery {
    pending: PendingReviewHandoff, deadline: Instant, completion: oneshot::Sender<ReviewHandoffResult>,
    expiry_task: tokio::task::JoinHandle<()>,
}
struct ReviewState {
    context_epoch: String, available: bool, surfaces: [Surface; 2], presentation: Option<PresentationSnapshot>,
    handoff_revision: u64, next_request: u64, pending: Option<Delivery>,
}
impl ReviewState {
    fn validate(&self, scope: &SurfaceScope, context: bool) -> Result<(), CompanionCode> {
        let surface = &self.surfaces[scope.caller.index()];
        if scope.open_epoch != surface.epoch() { return Err(CompanionCode::StaleSurface); }
        if scope.caller == ReviewCaller::Companion && !self.available { return Err(CompanionCode::Disabled); }
        if !surface.visible && (self.available || scope.caller == ReviewCaller::Companion) { return Err(CompanionCode::NotVisible); }
        if context && scope.context_epoch != self.context_epoch { return Err(CompanionCode::StaleContext); }
        Ok(())
    }
    fn retire(&mut self, result: ReviewHandoffResult) {
        if let Some(delivery) = self.pending.take() {
            delivery.expiry_task.abort();
            self.handoff_revision += 1;
            let _ = delivery.completion.send(result);
        }
    }
    fn expire(&mut self) -> bool {
        if self.pending.as_ref().is_some_and(|delivery| Instant::now() >= delivery.deadline) {
            self.retire(ReviewHandoffResult::Failed { code: CompanionCode::DeliveryTimeout });
            return true;
        }
        false
    }
    fn handoff_notice(&self) -> SurfaceNotice {
        SurfaceNotice::Handoff { revision: self.handoff_revision, request_id: self.pending.as_ref().map(|delivery| delivery.pending.request_id.clone()) }
    }
}

/// The workspace owns this single coordinator; host callbacks supply real visibility.
pub(crate) struct ReviewCoordinator { state: Arc<Mutex<ReviewState>> }
impl ReviewCoordinator {
    pub(crate) fn new(context_epoch: String) -> Self {
        Self { state: Arc::new(Mutex::new(ReviewState { context_epoch, available: false, surfaces: [Surface::new(true), Surface::new(false)], presentation: None, handoff_revision: 0, next_request: 0, pending: None })) }
    }
    fn notify(&self, notice: SurfaceNotice) {
        let listeners = { let state = self.state.lock(); [state.surfaces[0].listener.clone(), state.surfaces[1].listener.clone()] };
        for listener in listeners.into_iter().flatten() { listener(notice.clone()); }
    }
    pub(crate) fn subscribe(&self, caller: ReviewCaller, listener: Option<SurfaceListener>) { self.state.lock().surfaces[caller.index()].listener = listener; }
    pub(crate) fn context_epoch(&self) -> String { self.state.lock().context_epoch.clone() }
    pub(crate) fn workspace_changed(&self, workspace_revision: u64) {
        let notices = {
            let state = self.state.lock();
            state.surfaces.each_ref().map(|surface| surface.listener.clone().map(|listener| (listener,
                SurfaceNotice::Invalidate { workspace_revision, context_epoch: state.context_epoch.clone(), open_epoch: surface.epoch() })))
        };
        for (listener, notice) in notices.into_iter().flatten() { listener(notice); }
    }
    pub(crate) fn demand(&self) -> bool {
        let state = self.state.lock(); !state.available || state.surfaces.iter().any(|surface| surface.visible)
    }
    pub(crate) fn set_available(&self, available: bool) {
        let (notice, visibility) = {
            let mut state = self.state.lock();
            let changed = state.available != available;
            state.available = available;
            if changed && !available {
                for surface in &mut state.surfaces { surface.last_review = None; }
                state.surfaces[ReviewCaller::Companion.index()].cancel();
                state.surfaces[ReviewCaller::Companion.index()].generation += 1;
                state.retire(ReviewHandoffResult::Failed { code: CompanionCode::Disabled });
            }
            let visibility = state.surfaces.each_ref().map(|surface| surface.listener.clone().map(|listener| (listener,
                SurfaceNotice::Visibility { visible: surface.visible, open_epoch: surface.epoch() })));
            (state.handoff_notice(), visibility)
        };
        self.notify(notice);
        for (listener, notice) in visibility.into_iter().flatten() { listener(notice); }
    }
    pub(crate) fn set_visibility(&self, caller: ReviewCaller, visible: bool) -> String {
        let (epoch, listener, handoff) = {
            let mut state = self.state.lock();
            let surface = &mut state.surfaces[caller.index()];
            if surface.visible == visible { return surface.epoch(); }
            surface.visible = visible;
            surface.generation += 1;
            surface.cancel();
            let epoch = surface.epoch();
            let listener = surface.listener.clone();
            if caller == ReviewCaller::Companion && !visible { state.retire(ReviewHandoffResult::Failed { code: CompanionCode::StaleSurface }); }
            (epoch, listener, state.handoff_notice())
        };
        if let Some(listener) = listener { listener(SurfaceNotice::Visibility { visible, open_epoch: epoch.clone() }); }
        self.notify(handoff);
        epoch
    }
    pub(crate) fn context_changed(&self, context_epoch: &str, workspace_revision: u64) {
        let (notices, handoff) = {
            let mut state = self.state.lock();
            state.context_epoch = context_epoch.to_owned();
            for surface in &mut state.surfaces { surface.last_review = None; surface.cancel(); }
            state.retire(ReviewHandoffResult::Failed { code: CompanionCode::StaleContext });
            let notices = state.surfaces.each_ref().map(|surface| surface.listener.clone().map(|listener| (listener, SurfaceNotice::Invalidate {
                workspace_revision, context_epoch: context_epoch.to_owned(), open_epoch: surface.epoch(),
            })));
            (notices, state.handoff_notice())
        };
        for (listener, notice) in notices.into_iter().flatten() { listener(notice); }
        self.notify(handoff);
    }
    pub(crate) fn capture(&self, caller: ReviewCaller) -> Result<SurfaceScope, CompanionCode> {
        let state = self.state.lock();
        let surface = &state.surfaces[caller.index()];
        let scope = SurfaceScope { caller, context_epoch: state.context_epoch.clone(), open_epoch: surface.epoch(), cancellation: surface.cancellation.subscribe() };
        state.validate(&scope, true)?;
        Ok(scope)
    }
    pub(crate) fn validate(&self, scope: &SurfaceScope, context: bool) -> Result<(), CompanionCode> { self.state.lock().validate(scope, context) }
    pub(crate) fn is_current(&self, scope: &SurfaceScope) -> bool { self.validate(scope, true).is_ok() }
    pub(crate) fn open_epoch(&self, caller: ReviewCaller) -> String { self.state.lock().surfaces[caller.index()].epoch() }
    pub(crate) fn issued_review(&self, scope: &SurfaceScope, selection: &HandoffSelection) -> Option<IssuedReview> {
        let state = self.state.lock();
        state.validate(scope, true).ok()?;
        state.surfaces[scope.caller.index()].last_review.as_ref().filter(|issued| issued.selection == *selection).cloned()
    }
    pub(crate) fn record_review(&self, scope: &SurfaceScope, issued: IssuedReview) {
        let mut state = self.state.lock();
        if state.validate(scope, true).is_ok() { state.surfaces[scope.caller.index()].last_review = Some(issued); }
    }
    pub(crate) fn presentation(&self, presentation: PresentationInput) -> Result<PresentationSnapshot, CompanionCode> {
        for label in [&presentation.menu_labels.open_git_view, &presentation.menu_labels.quit] {
            if label.trim().is_empty() || label.len() > 512 || label.chars().any(char::is_control) { return Err(CompanionCode::InvalidRequest); }
        }
        let snapshot = {
            let mut state = self.state.lock();
            let revision = state.presentation.as_ref().map_or(1, |previous| previous.revision + 1);
            let snapshot = PresentationSnapshot { revision, locale: presentation.locale, appearance_theme: presentation.appearance_theme,
                icon_theme: presentation.icon_theme, review: presentation.review, persistence_error: presentation.persistence_error,
                menu_labels: presentation.menu_labels };
            state.presentation = Some(snapshot.clone());
            snapshot
        };
        self.notify(SurfaceNotice::Presentation { revision: snapshot.revision });
        Ok(snapshot)
    }
    pub(crate) fn surface_snapshot(&self, caller: ReviewCaller, workspace: WorkspaceSnapshot, observation: Option<ObservationSnapshot>) -> ReviewSurfaceSnapshot {
        let state = self.state.lock();
        let surface = &state.surfaces[caller.index()];
        let observation = (state.context_epoch == workspace.context_epoch).then_some(observation).flatten();
        ReviewSurfaceSnapshot { workspace, visible: surface.visible, open_epoch: surface.epoch(), observation, presentation: state.presentation.clone(), handoff: SurfaceHandoff {
            revision: state.handoff_revision, pending_request_id: state.pending.as_ref().map(|delivery| delivery.pending.request_id.clone()),
        } }
    }
    pub(crate) fn create_handoff(&self, scope: &SurfaceScope, entry_id: Option<String>, target: Option<ReviewHandoffTarget>) -> Result<(PendingReviewHandoff, oneshot::Receiver<ReviewHandoffResult>), CompanionCode> {
        let (pending, completion, notice) = {
            let mut state = self.state.lock();
            state.expire();
            state.validate(scope, true)?;
            if state.pending.as_ref().is_some_and(|delivery| delivery.pending.phase == HandoffPhase::Claimed) { return Err(CompanionCode::Busy); }
            state.retire(ReviewHandoffResult::Failed { code: CompanionCode::StaleSurface });
            state.next_request += 1;
            state.handoff_revision += 1;
            let pending = PendingReviewHandoff { request_id: format!("handoff-{}", state.next_request), context_epoch: scope.context_epoch.clone(), entry_id, target, source_open_epoch: scope.open_epoch.clone(), phase: HandoffPhase::Pending };
            let (sender, completion) = oneshot::channel();
            let deadline = Instant::now() + HANDOFF_TIMEOUT;
            let weak = Arc::downgrade(&self.state);
            let request_id = pending.request_id.clone();
            let expiry_task = tokio::spawn(async move {
                tokio::time::sleep_until(deadline).await;
                let Some(shared) = weak.upgrade() else { return; };
                let (notice, listeners) = {
                    let mut state = shared.lock();
                    if !state.pending.as_ref().is_some_and(|delivery| delivery.pending.request_id == request_id) || !state.expire() { return; }
                    (state.handoff_notice(), [state.surfaces[0].listener.clone(), state.surfaces[1].listener.clone()])
                };
                for listener in listeners.into_iter().flatten() { listener(notice.clone()); }
            });
            state.pending = Some(Delivery { pending: pending.clone(), deadline, completion: sender, expiry_task });
            (pending, completion, state.handoff_notice())
        };
        self.notify(notice);
        Ok((pending, completion))
    }
    pub(crate) fn pending(&self) -> PendingReviewHandoffSnapshot {
        let (snapshot, notice) = {
            let mut state = self.state.lock();
            let expired = state.expire();
            (PendingReviewHandoffSnapshot { revision: state.handoff_revision, pending: state.pending.as_ref().map(|delivery| delivery.pending.clone()) }, expired.then(|| state.handoff_notice()))
        };
        if let Some(notice) = notice { self.notify(notice); }
        snapshot
    }
    pub(crate) fn claim(&self, request_id: &str, context_epoch: &str) -> ClaimReviewHandoffResult {
        let mut state = self.state.lock();
        if state.expire() {
            let notice = state.handoff_notice(); drop(state); self.notify(notice);
            return ClaimReviewHandoffResult::Stale { code: CompanionCode::DeliveryTimeout };
        }
        let revision = state.handoff_revision;
        let Some(delivery) = state.pending.as_mut() else { return ClaimReviewHandoffResult::Stale { code: CompanionCode::StaleSurface }; };
        if delivery.pending.request_id != request_id || delivery.pending.context_epoch != context_epoch { return ClaimReviewHandoffResult::Stale { code: CompanionCode::StaleContext }; }
        if delivery.pending.phase == HandoffPhase::Claimed { return ClaimReviewHandoffResult::Busy { code: CompanionCode::Busy }; }
        delivery.pending.phase = HandoffPhase::Claimed;
        ClaimReviewHandoffResult::Claimed { request_id: request_id.to_owned(), context_epoch: context_epoch.to_owned(), handoff_revision: revision, target: delivery.pending.target.clone(), remaining_ms: delivery.deadline.saturating_duration_since(Instant::now()).as_millis() as u64 }
    }
    pub(crate) fn ack(&self, request_id: &str, context_epoch: &str, outcome: HandoffOutcome) -> AckReviewHandoffResult {
        let notice = {
            let mut state = self.state.lock();
            if state.expire() {
                let notice = state.handoff_notice(); drop(state); self.notify(notice);
                return AckReviewHandoffResult::Stale { code: CompanionCode::DeliveryTimeout };
            }
            if !state.pending.as_ref().is_some_and(|delivery| delivery.pending.request_id == request_id && delivery.pending.context_epoch == context_epoch && delivery.pending.phase == HandoffPhase::Claimed) {
                return AckReviewHandoffResult::Stale { code: CompanionCode::StaleSurface };
            }
            let request_id = request_id.to_owned();
            state.retire(match outcome { HandoffOutcome::Applied => ReviewHandoffResult::Applied { request_id }, HandoffOutcome::Changed => ReviewHandoffResult::Changed { request_id }, HandoffOutcome::Unavailable => ReviewHandoffResult::Unavailable { request_id } });
            state.handoff_notice()
        };
        self.notify(notice);
        AckReviewHandoffResult::Applied
    }
    pub(crate) fn fail(&self, request_id: &str, code: CompanionCode) {
        let notice = {
            let mut state = self.state.lock();
            if !state.pending.as_ref().is_some_and(|delivery| delivery.pending.request_id == request_id) { return; }
            state.retire(ReviewHandoffResult::Failed { code });
            state.handoff_notice()
        };
        self.notify(notice);
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/integration/companion_review.rs"]
mod integration_tests;
