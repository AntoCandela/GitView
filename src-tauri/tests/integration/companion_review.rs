//! Exercises native handoff authority, application fencing and source lifecycle.

use super::*;
use crate::application::RepositoryService;
use crate::test_support::{commit, git, working_tree};
use crate::workspace::OpenOutcome;
use std::sync::Arc;

fn opened() -> ReviewCoordinator {
    let controller = ReviewCoordinator::new("context-1".into());
    controller.set_available(true);
    controller.set_visibility(ReviewCaller::Companion, true);
    controller
}

#[tokio::test]
async fn superseded_handoff_cannot_claim_and_retirement_keeps_a_higher_null_revision() {
    let controller = opened();
    let scope = controller.capture(ReviewCaller::Companion).unwrap();
    let (a, a_completion) = controller.create_handoff(&scope, None, None).unwrap();
    let discovered_a = controller.pending();
    let (b, _) = controller.create_handoff(&scope, None, None).unwrap();
    assert!(matches!(a_completion.await.unwrap(), ReviewHandoffResult::Failed { code: CompanionCode::StaleSurface, .. }));
    assert!(matches!(controller.claim(&a.request_id, &a.context_epoch), ClaimReviewHandoffResult::Stale { .. }));
    assert!(controller.pending().revision > discovered_a.revision);
    assert!(matches!(controller.claim(&b.request_id, &b.context_epoch), ClaimReviewHandoffResult::Claimed { .. }));
    assert!(matches!(controller.ack(&b.request_id, &b.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Applied));
    let retired = controller.pending();
    assert!(retired.pending.is_none());
    assert!(retired.revision > discovered_a.revision);
    assert!(matches!(controller.ack(&b.request_id, &b.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Stale { .. }));
}

#[tokio::test(start_paused = true)]
async fn claimed_target_is_busy_until_deadline_and_expired_claim_cannot_ack() {
    let controller = opened();
    let scope = controller.capture(ReviewCaller::Companion).unwrap();
    let (a, completed) = controller.create_handoff(&scope, None, None).unwrap();
    assert!(matches!(controller.claim(&a.request_id, &a.context_epoch), ClaimReviewHandoffResult::Claimed { remaining_ms: 5000, .. }));
    assert!(matches!(controller.create_handoff(&scope, None, None), Err(CompanionCode::Busy)));
    tokio::time::advance(HANDOFF_TIMEOUT).await;
    assert!(matches!(controller.claim(&a.request_id, &a.context_epoch), ClaimReviewHandoffResult::Stale { .. }));
    assert!(matches!(completed.await.unwrap(), ReviewHandoffResult::Failed { code: CompanionCode::DeliveryTimeout, .. }));
    assert!(controller.pending().pending.is_none());
    assert!(controller.is_current(&scope));
    assert!(matches!(controller.ack(&a.request_id, &a.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Stale { .. }));
}

#[tokio::test]
async fn explicit_source_dismissal_cancels_claim_and_never_authorizes_reopened_source() {
    let controller = opened();
    let scope = controller.capture(ReviewCaller::Companion).unwrap();
    let (pending, completed) = controller.create_handoff(&scope, None, None).unwrap();
    controller.claim(&pending.request_id, &pending.context_epoch);
    controller.set_visibility(ReviewCaller::Companion, false);
    assert!(!controller.is_current(&scope));
    assert!(matches!(completed.await.unwrap(), ReviewHandoffResult::Failed { code: CompanionCode::StaleSurface, .. }));
    controller.set_visibility(ReviewCaller::Companion, true);
    assert!(!controller.is_current(&scope));
    assert!(matches!(controller.ack(&pending.request_id, &pending.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Stale { .. }));
}

async fn service_with_review() -> (tempfile::TempDir, std::path::PathBuf, Arc<RepositoryService>, SurfaceScope, HandoffSelection) {
    let (temp, root) = working_tree();
    std::fs::write(root.join("tracked.txt"), b"original\n").unwrap();
    commit(&root);
    std::fs::write(root.join("tracked.txt"), b"changed\n").unwrap();
    let service = Arc::new(RepositoryService::new());
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("admission failed") };
    service.select(&entry_id).await;
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let BeginCompanionReviewResult::Ready { surface } = service.begin_companion_review(&epoch).await else { panic!("fresh opening failed") };
    let Some(ObservationSnapshot::Ready { observation_revision, files, .. }) = surface.observation else { panic!("missing observation") };
    let file = files.iter().find(|file| file.display_path == "tracked.txt").unwrap();
    let scope = service.capture_surface_scope(ReviewCaller::Companion).unwrap();
    let selection = HandoffSelection { entry_id, stable_path_id: file.stable_path_id.clone(), observation_revision, path_id: file.path_id.clone(), category: ReviewCategory::Unstaged };
    (temp, root, service, scope, selection)
}

#[tokio::test]
async fn forged_provenance_is_rejected_before_main_reveal() {
    let (_temp, _root, service, scope, mut selection) = service_with_review().await;
    selection.stable_path_id = "forged".into();
    let result = service.request_review_handoff(RequestReviewHandoff { open_epoch: scope.open_epoch, context_epoch: scope.context_epoch, selection: Some(selection) }, |_, _| async { panic!("forged target revealed main") }).await;
    assert!(matches!(result, ReviewHandoffResult::Failed { code: CompanionCode::InvalidRequest, .. }));
    assert!(service.pending_review_handoff().pending.is_none());
}

#[tokio::test]
async fn last_issued_review_revalidates_a_ceased_category_without_substitution() {
    let (_temp, root, service, scope, selection) = service_with_review().await;
    assert!(matches!(service.review_file_for_surface(&scope, &selection.entry_id, selection.observation_revision, &selection.path_id, selection.category).await, Ok(crate::diff::ReviewResult::Text { .. })));
    git(&root, &["add", "tracked.txt"]);
    let receiver = Arc::clone(&service);
    let expected_stable = selection.stable_path_id.clone();
    let result = service.request_review_handoff(RequestReviewHandoff { open_epoch: scope.open_epoch, context_epoch: scope.context_epoch.clone(), selection: Some(selection) }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(matches!(&pending.target, Some(ReviewHandoffTarget::NoRemaining { selection, .. }) if selection.stable_path_id == expected_stable && selection.category == ReviewCategory::Unstaged));
        assert!(matches!(receiver.claim_review_handoff(&request_id, &pending.context_epoch), ClaimReviewHandoffResult::Claimed { .. }));
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Changed);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Changed { .. }));
    assert!(service.pending_review_handoff().pending.is_none());
}

#[tokio::test]
async fn reveal_failure_retires_target_without_dismissing_source() {
    let (_temp, _root, service, scope, _) = service_with_review().await;
    let result = service.request_review_handoff(RequestReviewHandoff { open_epoch: scope.open_epoch.clone(), context_epoch: scope.context_epoch.clone(), selection: None }, |_, _| async { Err(CompanionCode::WindowUnavailable) }).await;
    assert!(matches!(result, ReviewHandoffResult::Failed { code: CompanionCode::WindowUnavailable, .. }));
    assert!(service.surface_scope_is_current(&scope));
    assert!(service.pending_review_handoff().pending.is_none());
}

#[tokio::test]
async fn context_epoch_changes_for_reselection_and_removal_but_not_name_edits() {
    let (_temp, _root, service, scope, selection) = service_with_review().await;
    service.rename(&selection.entry_id, "renamed").await;
    assert_eq!(service.snapshot().await.context_epoch, scope.context_epoch);
    service.select(&selection.entry_id).await;
    let reselected = service.snapshot().await.context_epoch;
    assert_ne!(reselected, scope.context_epoch);
    assert!(!service.surface_scope_is_current(&scope));
    service.remove(&selection.entry_id).await;
    assert_ne!(service.snapshot().await.context_epoch, reselected);
}

#[tokio::test]
async fn current_row_tokens_authorize_handoff_before_a_review_was_issued() {
    let (_temp, _root, service, scope, selection) = service_with_review().await;
    let receiver = Arc::clone(&service);
    let expected = selection.clone();
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch, context_epoch: scope.context_epoch, selection: Some(selection),
    }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(matches!(&pending.target, Some(ReviewHandoffTarget::Live { entry_id, selection, .. })
            if entry_id == &expected.entry_id && selection.stable_path_id == expected.stable_path_id && selection.category == expected.category));
        receiver.claim_review_handoff(&request_id, &pending.context_epoch);
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Applied);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Applied { .. }));
}

#[tokio::test]
async fn only_the_last_issued_category_retains_expired_token_provenance() {
    let (_temp, root, service, scope, old) = service_with_review().await;
    service.review_file_for_surface(&scope, &old.entry_id, old.observation_revision, &old.path_id, old.category).await.unwrap();
    git(&root, &["add", "tracked.txt"]);
    let BeginCompanionReviewResult::Ready { surface } = service.begin_companion_review(&scope.open_epoch).await else { panic!("fresh status failed") };
    let Some(ObservationSnapshot::Ready { observation_revision, files, .. }) = surface.observation else { panic!("status unavailable") };
    let staged = files.iter().find(|file| file.display_path == "tracked.txt").unwrap();
    assert!(matches!(service.review_file_for_surface(&scope, &old.entry_id, observation_revision, &staged.path_id, ReviewCategory::Staged).await, Ok(crate::diff::ReviewResult::Text { .. })));
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch, context_epoch: scope.context_epoch, selection: Some(old),
    }, |_, _| async { panic!("overwritten historical authority revealed main") }).await;
    assert!(matches!(result, ReviewHandoffResult::Failed { code: CompanionCode::InvalidRequest }));
}

#[tokio::test]
async fn reviewed_path_leaving_status_is_no_remaining_but_a_lost_context_is_unavailable() {
    let (_temp, root, service, scope, selection) = service_with_review().await;
    service.review_file_for_surface(&scope, &selection.entry_id, selection.observation_revision, &selection.path_id, selection.category).await.unwrap();
    std::fs::write(root.join("tracked.txt"), b"original\n").unwrap();
    let receiver = Arc::clone(&service);
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch.clone(), context_epoch: scope.context_epoch.clone(), selection: Some(selection.clone()),
    }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(matches!(pending.target, Some(ReviewHandoffTarget::NoRemaining { .. })));
        receiver.claim_review_handoff(&request_id, &pending.context_epoch);
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Changed);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Changed { .. }));
    std::fs::rename(&root, root.with_file_name("relocated")).unwrap();
    let receiver = Arc::clone(&service);
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch, context_epoch: scope.context_epoch, selection: Some(selection),
    }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(matches!(pending.target, Some(ReviewHandoffTarget::Unavailable { .. })));
        receiver.claim_review_handoff(&request_id, &pending.context_epoch);
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Unavailable);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Unavailable { .. }));
}

#[tokio::test]
async fn revealed_main_without_ack_times_out_and_keeps_current_source_usable() {
    let (_temp, _root, service, scope, _) = service_with_review().await;
    let revision = service.pending_review_handoff().revision;
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch.clone(), context_epoch: scope.context_epoch.clone(), selection: None,
    }, |_, _| async { Ok(()) }).await;
    assert!(matches!(result, ReviewHandoffResult::Failed { code: CompanionCode::DeliveryTimeout }));
    assert!(service.surface_scope_is_current(&scope));
    let snapshot = service.review_surface_snapshot(ReviewCaller::Companion).await;
    assert!(snapshot.handoff.pending_request_id.is_none());
    assert!(snapshot.handoff.revision > revision);
}

#[tokio::test]
async fn context_replacement_retires_claim_and_cannot_reselect_the_old_destination() {
    let controller = opened();
    let scope = controller.capture(ReviewCaller::Companion).unwrap();
    let (pending, completion) = controller.create_handoff(&scope, Some("entry-a".into()), None).unwrap();
    controller.claim(&pending.request_id, &pending.context_epoch);
    controller.context_changed("context-2", 2);
    assert!(matches!(completion.await.unwrap(), ReviewHandoffResult::Failed { code: CompanionCode::StaleContext }));
    assert!(matches!(controller.ack(&pending.request_id, &pending.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Stale { .. }));
    assert_eq!(controller.capture(ReviewCaller::Companion).unwrap().context_epoch, "context-2");
    assert!(controller.pending().pending.is_none());
}

#[tokio::test(start_paused = true)]
async fn abandoned_delivery_expires_without_discovery_and_notifies_null_retirement() {
    let controller = opened();
    let notices = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&notices);
    controller.subscribe(ReviewCaller::Main, Some(Arc::new(move |notice| received.lock().push(notice))));
    let scope = controller.capture(ReviewCaller::Companion).unwrap();
    let (_, completion) = controller.create_handoff(&scope, None, None).unwrap();
    tokio::time::advance(HANDOFF_TIMEOUT).await;
    assert!(matches!(completion.await.unwrap(), ReviewHandoffResult::Failed { code: CompanionCode::DeliveryTimeout }));
    assert!(matches!(notices.lock().last(), Some(SurfaceNotice::Handoff { revision: 2, request_id: None })));
}

#[test]
fn presentation_payload_rejects_unknown_fields_and_enum_values() {
    let valid = serde_json::json!({
        "locale": "en-GB", "appearanceTheme": "cream", "iconTheme": "material",
        "review": { "mode": "changes", "theme": "match", "lineMode": "wrap" },
        "persistenceError": true, "menuLabels": { "openGitView": "Open in GitView", "quit": "Quit" }
    });
    let mut unknown = valid.clone();
    unknown["root"] = serde_json::json!("/private");
    assert!(serde_json::from_value::<PresentationInput>(unknown).is_err());
    let mut invalid = valid.clone();
    invalid["locale"] = serde_json::json!("unknown");
    assert!(serde_json::from_value::<PresentationInput>(invalid).is_err());
    let presentation = serde_json::from_value::<PresentationInput>(valid).unwrap();
    let controller = ReviewCoordinator::new("context".into());
    let snapshot = controller.presentation(presentation).unwrap();
    assert!(snapshot.persistence_error);
    assert_eq!(snapshot.icon_theme, IconTheme::Material);
}

#[tokio::test]
async fn staging_a_reviewed_deletion_preserves_no_remaining_unstaged_identity() {
    let (_temp, root, service, scope, first) = service_with_review().await;
    std::fs::remove_file(root.join("tracked.txt")).unwrap();
    let BeginCompanionReviewResult::Ready { surface } = service.begin_companion_review(&scope.open_epoch).await else { panic!("fresh deletion failed") };
    let Some(ObservationSnapshot::Ready { observation_revision, files, .. }) = surface.observation else { panic!("deletion unavailable") };
    let deleted = files.iter().find(|file| file.stable_path_id == first.stable_path_id).unwrap();
    let selection = HandoffSelection { observation_revision, path_id: deleted.path_id.clone(), ..first };
    service.review_file_for_surface(&scope, &selection.entry_id, observation_revision, &selection.path_id, selection.category).await.unwrap();
    git(&root, &["add", "tracked.txt"]);
    let receiver = Arc::clone(&service);
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: scope.open_epoch, context_epoch: scope.context_epoch, selection: Some(selection),
    }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(matches!(&pending.target, Some(ReviewHandoffTarget::NoRemaining { selection, .. }) if selection.category == ReviewCategory::Unstaged));
        receiver.claim_review_handoff(&request_id, &pending.context_epoch);
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Changed);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Changed { .. }));
}

#[tokio::test]
async fn workspace_fact_updates_notify_without_invalidating_review_context() {
    let (_temp, _root, service, scope, selection) = service_with_review().await;
    let notices = Arc::new(Mutex::new(Vec::new()));
    let received = Arc::clone(&notices);
    service.subscribe_review_surface(ReviewCaller::Main, Arc::new(move |notice| received.lock().push(notice)));
    service.rename(&selection.entry_id, "new label").await;
    let current = service.snapshot().await;
    assert!(service.surface_scope_is_current(&scope));
    assert!(notices.lock().iter().any(|notice| matches!(notice, SurfaceNotice::Invalidate { workspace_revision, context_epoch, .. }
        if *workspace_revision == current.revision && context_epoch == &scope.context_epoch)));
}

#[tokio::test]
async fn hidden_context_reselection_never_pairs_new_epoch_with_old_observation() {
    let (_temp, _root, service, scope, selection) = service_with_review().await;
    service.set_surface_visibility(ReviewCaller::Companion, false);
    service.set_surface_visibility(ReviewCaller::Main, false);
    service.reconcile_surface_demand().await;
    service.select(&selection.entry_id).await;
    let surface = service.review_surface_snapshot(ReviewCaller::Companion).await;
    assert_ne!(surface.workspace.context_epoch, scope.context_epoch);
    assert!(surface.observation.is_none());
}

#[tokio::test]
async fn empty_handoff_preserves_empty_workspace_and_requires_claim_before_ack() {
    let service = Arc::new(RepositoryService::new());
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let scope = service.capture_surface_scope(ReviewCaller::Companion).unwrap();
    let receiver = Arc::clone(&service);
    let result = service.request_review_handoff(RequestReviewHandoff {
        open_epoch: epoch, context_epoch: scope.context_epoch, selection: None,
    }, move |request_id, _| async move {
        let pending = receiver.pending_review_handoff().pending.unwrap();
        assert!(pending.entry_id.is_none());
        assert!(pending.target.is_none());
        assert!(matches!(receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Applied), AckReviewHandoffResult::Stale { .. }));
        receiver.claim_review_handoff(&request_id, &pending.context_epoch);
        receiver.ack_review_handoff(&request_id, &pending.context_epoch, HandoffOutcome::Applied);
        Ok(())
    }).await;
    assert!(matches!(result, ReviewHandoffResult::Applied { .. }));
    assert!(service.snapshot().await.active_context_id.is_none());
}

#[test]
fn unavailable_companion_visibility_cannot_contribute_native_demand() {
    let controller = ReviewCoordinator::new("context".into());
    assert!(controller.demand());
    controller.set_visibility(ReviewCaller::Main, false);
    controller.set_visibility(ReviewCaller::Companion, true);
    assert!(!controller.demand());
    assert!(matches!(controller.capture(ReviewCaller::Companion), Err(CompanionCode::Disabled)));
    controller.set_available(true);
    assert!(controller.demand());
    controller.set_available(false);
    assert!(!controller.demand());
    controller.set_visibility(ReviewCaller::Main, true);
    assert!(controller.demand());
}
