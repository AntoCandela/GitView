//! Exercises persistence ordering and owned restore/recovery cancellation at deterministic gates.

use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use super::RepositoryService;
use crate::git::GitProbe;
use crate::observation::ObservationSnapshot;
use crate::test_support::{allow_new_probes, block, executable, gated_git, git, release, wait_for_probe, wait_for_reaped_child, working_tree};
use crate::workspace::{Availability, EntryKind, HeadLabel, MutationOutcome, OpenOutcome, SelectOutcome, WorkspaceSnapshot};
use serde_json::{json, Value};

fn seed(path: &Path, roots: &[PathBuf], active: Option<&Path>) {
    let repositories: Vec<_> = roots.iter().map(|root| json!({ "root": root })).collect();
    fs::write(path, serde_json::to_vec(&json!({
        "version": 1, "repositories": repositories, "activeRoot": active,
    })).unwrap()).unwrap();
}

fn other_repository(at: &Path) -> PathBuf {
    fs::create_dir(at).unwrap();
    git(at, &["init", "-b", "other"]);
    fs::canonicalize(at).unwrap()
}

async fn wait_for(service: &RepositoryService, predicate: impl Fn(&WorkspaceSnapshot) -> bool) -> WorkspaceSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = service.snapshot().await;
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("workspace did not reach its expected transition")
}

fn opened(outcome: OpenOutcome) -> String {
    match outcome {
        OpenOutcome::Opened { entry_id, .. } => entry_id,
        other => panic!("expected admission, got {other:?}"),
    }
}

#[tokio::test]
async fn restored_context_gets_session_authority_without_persisting_review_epochs() {
    let (temp, root) = working_tree();
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let id = opened(service.open_chosen(&root).await);
    service.select(&id).await;
    let before = service.snapshot().await;
    service.shutdown().await;
    let document: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert!(document.get("contextEpoch").is_none());
    let restored = RepositoryService::with_workspace_file(file).await;
    let snapshot = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert!(snapshot.active_context_id.is_some());
    assert_ne!(snapshot.context_epoch, before.context_epoch);
    assert_eq!(serde_json::to_value(&snapshot).unwrap()["contextEpoch"], snapshot.context_epoch);
    restored.shutdown().await;
}

#[tokio::test]
async fn companion_open_waits_for_gated_restoration_then_completes_without_reopening() {
    use crate::companion::{BeginCompanionReviewResult, ReviewCaller};
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, std::slice::from_ref(&root), Some(&root));
    block(&root);
    let service = RepositoryService::load_workspace(file, GitProbe::with_executable(&gated_git(temp.path()))).await;
    wait_for_probe(&root).await;
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let before = service.snapshot().await.context_epoch;
    let opening = service.begin_companion_review(&epoch);
    tokio::pin!(opening);
    assert!(tokio::time::timeout(Duration::from_millis(40), &mut opening).await.is_err());
    allow_new_probes(&root);
    release(&root);
    let BeginCompanionReviewResult::Ready { surface } = tokio::time::timeout(Duration::from_secs(10), opening).await.unwrap() else { panic!("recovered opening did not become ready") };
    assert_eq!(surface.open_epoch, epoch);
    assert_eq!(surface.workspace.context_epoch, before);
    assert!(matches!(surface.observation, Some(ObservationSnapshot::Ready { .. })));
}

#[tokio::test]
async fn accepted_verified_selection_refreshes_workspace_facts_without_a_renderer_request() {
    let (temp, root) = working_tree();
    let service = RepositoryService::with_probe_and_diagnostics(GitProbe::with_executable(&gated_git(temp.path())), Default::default());
    let id = opened(service.open_chosen(&root).await);
    git(&root, &["branch", "-m", "updated-native-head"]);
    block(&root);
    let selected = tokio::time::timeout(Duration::from_secs(1), service.select(&id)).await.unwrap();
    assert!(matches!(selected, SelectOutcome::Selected { .. }));
    wait_for_probe(&root).await;
    allow_new_probes(&root);
    release(&root);
    let ready = wait_for(&service, |snapshot| snapshot.entries[0].availability == Availability::Available).await;
    assert_eq!(ready.active_context_id.as_deref(), Some(id.as_str()));
    assert_eq!(ready.entries[0].head, HeadLabel::Branch { name: "updated-native-head".into() });
}

#[tokio::test]
async fn selection_refresh_stops_when_both_hidden_and_resumes_with_visible_demand() {
    use crate::companion::ReviewCaller;
    let (temp, root) = working_tree();
    let service = RepositoryService::with_probe_and_diagnostics(GitProbe::with_executable(&gated_git(temp.path())), Default::default());
    let id = opened(service.open_chosen(&root).await);
    block(&root);
    service.select(&id).await;
    wait_for_probe(&root).await;
    service.set_companion_available(true);
    service.set_surface_visibility(ReviewCaller::Main, false);
    wait_for_reaped_child(&root).await;
    assert_eq!(service.snapshot().await.entries[0].availability, Availability::Checking);
    allow_new_probes(&root);
    release(&root);
    service.set_surface_visibility(ReviewCaller::Main, true);
    service.reconcile_surface_demand().await;
    let ready = wait_for(&service, |snapshot| snapshot.entries[0].availability == Availability::Available).await;
    assert_eq!(ready.active_context_id.as_deref(), Some(id.as_str()));
}

#[tokio::test]
async fn explicit_refresh_supersedes_the_owned_selection_refresh() {
    let (temp, root) = working_tree();
    let service = RepositoryService::with_probe_and_diagnostics(GitProbe::with_executable(&gated_git(temp.path())), Default::default());
    let id = opened(service.open_chosen(&root).await);
    block(&root);
    service.select(&id).await;
    wait_for_probe(&root).await;
    allow_new_probes(&root);
    let current = service.refresh(&id).await;
    wait_for_reaped_child(&root).await;
    assert_eq!(current.entries[0].availability, Availability::Available);
}

#[tokio::test]
async fn pending_selected_restore_is_responsive_and_never_reselects_after_new_user_intent() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    // The selected context is second in list order but must be probed first.
    seed(&file, &[other.clone(), root.clone()], Some(&root));
    block(&root);
    let service = RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await;
    wait_for_probe(&root).await;
    let initial = tokio::time::timeout(Duration::from_secs(1), service.snapshot()).await.unwrap();
    assert!(initial.restoring);
    assert!(initial.entries.iter().all(|entry| entry.kind == EntryKind::Unknown));
    assert_eq!(initial.active_context_id.as_deref(), Some(initial.entries[1].id.as_str()));
    assert!(matches!(service.observe_selected_context(&initial.entries[1].id).await, ObservationSnapshot::Checking { .. }));

    let selected = tokio::time::timeout(Duration::from_secs(1), service.select(&initial.entries[0].id)).await.unwrap();
    assert!(matches!(selected, SelectOutcome::Selected { .. }));
    let newer = wait_for(&service, |snapshot| snapshot.entries[0].availability == Availability::Available).await;
    assert_eq!(newer.active_context_id.as_deref(), Some(initial.entries[0].id.as_str()));
    assert!(matches!(service.observe_selected_context(&initial.entries[1].id).await, ObservationSnapshot::Unavailable { .. }));
    allow_new_probes(&root);
    release(&root);
    let complete = wait_for(&service, |snapshot| !snapshot.restoring).await;
    assert_eq!(complete.active_context_id, newer.active_context_id);
    let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(saved["activeRoot"], json!(other));
    assert_eq!(saved["repositories"], json!([{ "root": other }, { "root": root }]));
}

#[tokio::test]
async fn user_reselection_invalidates_a_stale_initial_head_result() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, std::slice::from_ref(&root), Some(&root));
    block(&root);
    // Capture the confirmation-pass HEAD before blocking; a later selection must reject it.
    let git_executable = executable(temp.path(), r#"
if [ "$1 $2 $3" = "symbolic-ref --quiet --short" ] && [ -f .gitview-block ]; then
    if [ -f .gitview-first-symbolic ]; then
        git "$@" || exit $?
        : > .gitview-entered
        while [ ! -f .gitview-release ]; do sleep 0.01; done
        exit 0
    fi
    : > .gitview-first-symbolic
fi
exec git "$@"
"#);
    let service = RepositoryService::load_workspace(file, GitProbe::with_executable(&git_executable)).await;
    wait_for_probe(&root).await;
    let initial = service.snapshot().await;
    git(&root, &["branch", "-m", "newer"]);
    allow_new_probes(&root);
    service.select(&initial.entries[0].id).await;
    let updated = wait_for(&service, |snapshot| snapshot.entries[0].head == HeadLabel::Branch { name: "newer".into() }).await;
    release(&root);
    let complete = wait_for(&service, |snapshot| !snapshot.restoring).await;
    assert_eq!(complete.entries[0], updated.entries[0]);
    assert_eq!(complete.active_context_id, updated.active_context_id);
}

#[tokio::test]
async fn dropping_service_cancels_and_reaps_restoration_without_a_reference_cycle() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, std::slice::from_ref(&root), Some(&root));
    let original = fs::read(&file).unwrap();
    block(&root);
    let service = RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await;
    wait_for_probe(&root).await;
    let workspace = Arc::downgrade(&service.workspace);
    drop(service);
    wait_for_reaped_child(&root).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while workspace.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    }).await.expect("restoration retained the workspace after service drop");
    assert_eq!(fs::read(file).unwrap(), original);
}

#[tokio::test]
async fn selection_changes_and_service_drop_cancel_and_reap_selected_unknown_recovery() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    seed(&file, &[root.clone(), other], Some(&root));
    let parked = temp.path().join("parked");
    fs::rename(&root, &parked).unwrap();
    let service = RepositoryService::load_workspace(file, GitProbe::with_executable(&gated_git(temp.path()))).await;
    let failed = wait_for(&service, |snapshot| !snapshot.restoring).await;
    assert_eq!(failed.entries[0].availability, Availability::Unavailable);
    block(&parked);
    fs::rename(&parked, &root).unwrap();
    wait_for_probe(&root).await;
    service.select(&failed.entries[1].id).await;
    wait_for_reaped_child(&root).await;
    let switched = service.snapshot().await;
    assert_eq!(switched.active_context_id.as_deref(), Some(failed.entries[1].id.as_str()));
    assert_eq!(switched.entries[0].kind, EntryKind::Unknown);
    assert!(matches!(service.observe_selected_context(&failed.entries[0].id).await, ObservationSnapshot::Unavailable { .. }));
    fs::remove_file(root.join(".gitview-entered")).unwrap();
    fs::remove_file(root.join(".gitview-child-pid")).unwrap();
    service.select(&failed.entries[0].id).await;
    wait_for_probe(&root).await;
    let workspace = Arc::downgrade(&service.workspace);
    drop(service);
    wait_for_reaped_child(&root).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while workspace.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    }).await.expect("recovery retained the workspace after service drop");
}

#[tokio::test]
async fn delayed_savers_capture_newest_complete_choices_without_holding_selection_lock() {
    let (temp, root) = working_tree();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::with_workspace_file(file.clone()).await);
    let first_id = opened(service.open_chosen(&root).await);
    let second_id = opened(service.open_chosen(&other).await);
    let gate = service.persistence.as_ref().unwrap().gate.lock().await;
    let first = {
        let service = Arc::clone(&service);
        let id = first_id.clone();
        tokio::spawn(async move { service.select(&id).await })
    };
    wait_for(&service, |snapshot| snapshot.active_context_id.as_deref() == Some(&first_id)).await;
    let second = {
        let service = Arc::clone(&service);
        let id = second_id.clone();
        tokio::spawn(async move { service.select(&id).await })
    };
    let newest = tokio::time::timeout(Duration::from_secs(1), wait_for(&service, |snapshot| {
        snapshot.active_context_id.as_deref() == Some(&second_id)
    })).await.unwrap();
    // Both completed choices are waiting to save, not holding the state/selection boundary.
    tokio::time::timeout(Duration::from_secs(1), service.observe_selected_context(&second_id)).await.unwrap();
    drop(gate);
    assert!(matches!(first.await.unwrap(), SelectOutcome::Selected { .. }));
    assert!(matches!(second.await.unwrap(), SelectOutcome::Selected { .. }));
    let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(saved["activeRoot"], json!(other));
    assert_eq!(saved["repositories"], json!([
        { "root": newest.entries[0].location_label }, { "root": newest.entries[1].location_label },
    ]));
}

#[tokio::test]
async fn cancelled_pending_open_reaps_its_child_and_preserves_saved_bytes() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, &[], None);
    let original = fs::read(&file).unwrap();
    let service = Arc::new(RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await);
    block(&root);
    let opening = {
        let service = Arc::clone(&service);
        let root = root.clone();
        tokio::spawn(async move { service.open_chosen(&root).await })
    };
    wait_for_probe(&root).await;
    opening.abort();
    assert!(opening.await.unwrap_err().is_cancelled());
    wait_for_reaped_child(&root).await;
    assert!(service.snapshot().await.entries.is_empty());
    assert_eq!(fs::read(file).unwrap(), original);
}

#[tokio::test]
async fn overlapping_opens_and_selection_save_the_complete_newest_admission_order() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await);
    let gate = service.persistence.as_ref().unwrap().gate.lock().await;
    block(&root);
    let first = {
        let service = Arc::clone(&service);
        let root = root.clone();
        tokio::spawn(async move { service.open_chosen(&root).await })
    };
    wait_for_probe(&root).await;
    let second = {
        let service = Arc::clone(&service);
        let other = other.clone();
        tokio::spawn(async move { service.open_chosen(&other).await })
    };
    let first_admitted = wait_for(&service, |snapshot| snapshot.entries.len() == 1).await;
    assert_eq!(first_admitted.entries[0].location_label, other.to_str().unwrap());
    allow_new_probes(&root);
    release(&root);
    let both_admitted = wait_for(&service, |snapshot| snapshot.entries.len() == 2).await;
    let selected = {
        let service = Arc::clone(&service);
        let entry_id = both_admitted.entries[0].id.clone();
        tokio::spawn(async move { service.select(&entry_id).await })
    };
    wait_for(&service, |snapshot| snapshot.active_context_id.is_some()).await;
    drop(gate);
    opened(first.await.unwrap());
    opened(second.await.unwrap());
    assert!(matches!(selected.await.unwrap(), SelectOutcome::Selected { .. }));
    let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(saved, json!({
        "version": 1, "repositories": [{ "root": other }, { "root": root }], "activeRoot": other,
    }));
}

#[tokio::test]
async fn unresolved_storage_remains_an_ephemeral_warning_after_successful_navigation() {
    let (_temp, root) = working_tree();
    let store = crate::diagnostics::DiagnosticStore::unavailable(crate::diagnostics::Code::StorageUnavailable);
    let service = RepositoryService::storage_unavailable(store.sink()).await;
    let initial = service.snapshot().await;
    assert!(!initial.restoring);
    assert_eq!(initial.persistence_error.unwrap().code, crate::workspace::persistence::PersistenceErrorCode::StorageUnavailable);
    let entry_id = opened(service.open_chosen(&root).await);
    let SelectOutcome::Selected { snapshot } = service.select(&entry_id).await else {
        panic!("storage resolution failure prevented session navigation");
    };
    assert_eq!(snapshot.active_context_id, Some(entry_id));
    assert_eq!(snapshot.persistence_error.unwrap().code, crate::workspace::persistence::PersistenceErrorCode::StorageUnavailable);
    assert!(matches!(store.health().state, crate::diagnostics::HealthState::Degraded));
    assert!(store.health().dropped > 0, "failed capture must remain visible in the host-owned health");
    service.shutdown().await;
}

#[tokio::test]
async fn removed_choice_is_not_resurrected_by_an_older_pending_reopen() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await);
    let entry_id = opened(service.open_chosen(&root).await);
    block(&root);
    let reopening = {
        let service = Arc::clone(&service);
        let root = root.clone();
        tokio::spawn(async move { service.open_chosen(&root).await })
    };
    wait_for_probe(&root).await;
    assert!(matches!(service.remove(&entry_id).await, MutationOutcome::Updated { .. }));
    let removed_bytes = fs::read(&file).unwrap();
    allow_new_probes(&root);
    release(&root);
    assert!(matches!(reopening.await.unwrap(), OpenOutcome::Cancelled { .. }));
    assert!(service.snapshot().await.entries.is_empty());
    assert_eq!(fs::read(&file).unwrap(), removed_bytes);
    assert!(root.join(".git").join("HEAD").is_file());
    assert_ne!(opened(service.open_chosen(&root).await), entry_id);
}

#[tokio::test]
async fn rename_survives_initial_verification_and_removal_discards_a_pending_refresh_completion() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, std::slice::from_ref(&root), Some(&root));
    block(&root);
    let service = Arc::new(RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await);
    wait_for_probe(&root).await;
    let entry_id = service.snapshot().await.entries[0].id.clone();
    let MutationOutcome::Updated { snapshot } = service.rename(&entry_id, "Checking label").await else {
        panic!("restoring entry could not be renamed");
    };
    assert_eq!(snapshot.entries[0].repository_label, "Checking label");
    allow_new_probes(&root);
    release(&root);
    let verified = wait_for(&service, |snapshot| !snapshot.restoring).await;
    assert_eq!(verified.entries[0].repository_label, "Checking label");
    assert_eq!(verified.entries[0].availability, Availability::Available);
    fs::remove_file(root.join(".gitview-entered")).unwrap();
    fs::remove_file(root.join(".gitview-release")).unwrap();
    block(&root);
    let refreshing = {
        let service = Arc::clone(&service);
        let entry_id = entry_id.clone();
        tokio::spawn(async move { service.refresh(&entry_id).await })
    };
    wait_for_probe(&root).await;
    let MutationOutcome::Updated { snapshot } = service.remove(&entry_id).await else {
        panic!("choice with a pending refresh could not be removed");
    };
    assert!(snapshot.entries.is_empty());
    assert_eq!(snapshot.active_context_id, None);
    let removed_bytes = fs::read(&file).unwrap();
    allow_new_probes(&root);
    release(&root);
    let complete = refreshing.await.unwrap();
    assert!(complete.entries.is_empty());
    assert!(matches!(service.observe_selected_context(&entry_id).await, ObservationSnapshot::Unavailable { .. }));
    assert_eq!(fs::read(file).unwrap(), removed_bytes);
}

#[tokio::test]
async fn removing_selected_unknown_cancels_recovery_and_starts_the_available_replacement() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    seed(&file, &[root.clone(), other.clone()], Some(&root));
    let parked = temp.path().join("parked");
    fs::rename(&root, &parked).unwrap();
    let service = RepositoryService::load_workspace(file.clone(), GitProbe::with_executable(&gated_git(temp.path()))).await;
    let failed = wait_for(&service, |snapshot| !snapshot.restoring).await;
    block(&parked);
    fs::rename(&parked, &root).unwrap();
    wait_for_probe(&root).await;
    let MutationOutcome::Updated { snapshot } = service.remove(&failed.entries[0].id).await else {
        panic!("unverified active choice could not be removed");
    };
    wait_for_reaped_child(&root).await;
    assert_eq!(snapshot.active_context_id.as_deref(), Some(failed.entries[1].id.as_str()));
    assert!(matches!(service.observe_selected_context(&failed.entries[0].id).await, ObservationSnapshot::Unavailable { .. }));
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(),
        json!({ "version": 1, "repositories": [{ "root": other }], "activeRoot": other }));
    assert!(root.join(".git").join("HEAD").is_file());
}

#[tokio::test]
async fn queued_rename_save_cannot_overwrite_a_newer_removal() {
    let (temp, root) = working_tree();
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::with_workspace_file(file.clone()).await);
    let entry_id = opened(service.open_chosen(&root).await);
    let gate = service.persistence.as_ref().unwrap().gate.lock().await;
    let rename = {
        let service = Arc::clone(&service);
        let entry_id = entry_id.clone();
        tokio::spawn(async move { service.rename(&entry_id, "Queued label").await })
    };
    wait_for(&service, |snapshot| snapshot.entries[0].repository_label == "Queued label").await;
    let remove = {
        let service = Arc::clone(&service);
        let entry_id = entry_id.clone();
        tokio::spawn(async move { service.remove(&entry_id).await })
    };
    wait_for(&service, |snapshot| snapshot.entries.is_empty()).await;
    drop(gate);
    assert!(matches!(rename.await.unwrap(), MutationOutcome::Updated { .. }));
    assert!(matches!(remove.await.unwrap(), MutationOutcome::Updated { .. }));
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(),
        json!({ "version": 1, "repositories": [], "activeRoot": null }));
    assert!(root.join(".git").join("HEAD").is_file());
}

#[tokio::test]
async fn cancelling_a_saver_keeps_its_write_serialized_with_newer_choices() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::with_workspace_file(file.clone()).await);
    let first_id = opened(service.open_chosen(&root).await);
    let second_id = opened(service.open_chosen(&other).await);
    let mut writer = crate::test_support::WorkspaceWriteGate::new(&file);
    let selecting = {
        let service = Arc::clone(&service);
        let first_id = first_id.clone();
        tokio::spawn(async move { service.select(&first_id).await })
    };
    writer.entered().await;
    selecting.abort();
    assert!(selecting.await.unwrap_err().is_cancelled());
    // The blocking writer survives cancellation, so its serialization ownership must too.
    assert!(service.persistence.as_ref().unwrap().gate.try_lock().is_err());
    let rename = {
        let service = Arc::clone(&service);
        let second_id = second_id.clone();
        tokio::spawn(async move { service.rename(&second_id, "Newest label").await })
    };
    wait_for(&service, |snapshot| snapshot.entries[1].repository_label == "Newest label").await;
    let remove = {
        let service = Arc::clone(&service);
        tokio::spawn(async move { service.remove(&first_id).await })
    };
    wait_for(&service, |snapshot| snapshot.entries.len() == 1
        && snapshot.active_context_id.as_deref() == Some(&second_id)).await;
    writer.release();
    writer.completed().await;
    assert!(matches!(rename.await.unwrap(), MutationOutcome::Updated { .. }));
    assert!(matches!(remove.await.unwrap(), MutationOutcome::Updated { .. }));
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(), json!({
        "version": 1, "repositories": [{ "root": other, "displayName": "Newest label" }],
        "activeRoot": other,
    }));
    assert!(service.snapshot().await.persistence_error.is_none());
    service.shutdown().await;
}

#[tokio::test]
async fn shutdown_finishes_cancelled_saves_and_publishes_failure_before_diagnostic_drain() {
    use crate::diagnostics::{Code, Component, DiagnosticStore, Event, OperationContext, OperationKind, Query, ReadOnlyDiagnostics};
    use crate::workspace::persistence::PersistenceErrorCode;

    let (temp, root) = working_tree();
    let file = temp.path().join("workspace.json");
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = Arc::new(RepositoryService::with_workspace_file_and_diagnostics(file.clone(), store.sink()).await);
    let entry_id = opened(service.open_chosen(&root).await);
    let parent = OperationContext::new(store.sink(), None, None);
    let mut writer = crate::test_support::WorkspaceWriteGate::new(&file);
    let rename = {
        let service = Arc::clone(&service);
        let entry_id = entry_id.clone();
        let parent = parent.clone();
        tokio::spawn(async move { parent.scope(service.rename(&entry_id, "Cancelled request")).await })
    };
    writer.entered().await;
    rename.abort();
    assert!(rename.await.unwrap_err().is_cancelled());
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let shutdown = service.shutdown();
    tokio::pin!(shutdown);
    assert!(std::future::poll_fn(|context| {
        std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
    }).await, "shutdown returned while an accepted workspace write was still blocked");
    writer.release();
    writer.completed().await;
    tokio::time::timeout(Duration::from_secs(10), shutdown).await.unwrap();
    assert_eq!(service.snapshot().await.persistence_error.unwrap().code, PersistenceErrorCode::SaveFailed);
    store.flush(Duration::from_secs(2)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query {
        component: Some(Component::Persistence), limit: 200, ..Default::default()
    }).unwrap().events;
    let save_rows: Vec<_> = rows.iter().filter(|row| row.operation_kind == OperationKind::PersistWorkspace
        && row.parent_operation_id == Some(parent.id())).collect();
    assert!(save_rows.iter().any(|row| row.event == Event::Failed && row.code == Some(Code::SaveFailed)));
    assert!(!save_rows.iter().any(|row| row.event == Event::Cancelled));
    fs::remove_dir(&file).unwrap();
    assert!(matches!(service.rename(&entry_id, "Retried label").await, MutationOutcome::Updated { .. }));
    assert!(service.snapshot().await.persistence_error.is_none());
    let saved: Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(saved["repositories"][0]["displayName"], "Retried label");
    service.shutdown().await;
    store.shutdown(Duration::from_secs(2)).unwrap();
}

#[tokio::test]
async fn dropping_service_detaches_accepted_save_without_retaining_the_service() {
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    let service = Arc::new(RepositoryService::with_workspace_file(file.clone()).await);
    let entry_id = opened(service.open_chosen(&root).await);
    let mut writer = crate::test_support::WorkspaceWriteGate::new(&file);
    let selecting = {
        let service = Arc::clone(&service);
        tokio::spawn(async move { service.select(&entry_id).await })
    };
    writer.entered().await;
    selecting.abort();
    assert!(selecting.await.unwrap_err().is_cancelled());
    let weak_service = Arc::downgrade(&service);
    let workspace = Arc::downgrade(&service.workspace);
    drop(service);
    assert!(weak_service.upgrade().is_none());
    writer.release();
    writer.completed().await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while workspace.upgrade().is_some() { tokio::task::yield_now().await; }
    }).await.expect("completed persistence task retained shared workspace state");
    assert_eq!(serde_json::from_slice::<Value>(&fs::read(file).unwrap()).unwrap(), json!({
        "version": 1, "repositories": [{ "root": root }], "activeRoot": root,
    }));
}

#[tokio::test]
async fn unverified_selection_canceled_while_hidden_recovers_on_companion_reveal() {
    use crate::companion::{BeginCompanionReviewResult, ReviewCaller};
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let file = temp.path().join("workspace.json");
    seed(&file, std::slice::from_ref(&root), None);
    let parked = temp.path().join("parked");
    fs::rename(&root, &parked).unwrap();
    let service = RepositoryService::load_workspace(file, GitProbe::with_executable(&gated_git(temp.path()))).await;
    let initial = wait_for(&service, |snapshot| !snapshot.restoring).await;
    assert_eq!(initial.entries[0].availability, Availability::Unavailable);
    let id = initial.entries[0].id.clone();
    block(&parked);
    fs::rename(&parked, &root).unwrap();
    service.select(&id).await;
    wait_for_probe(&root).await;
    service.set_surface_visibility(ReviewCaller::Main, false);
    wait_for_reaped_child(&root).await;
    let hidden = service.snapshot().await;
    assert_eq!(hidden.entries[0].kind, EntryKind::Unknown);
    assert_eq!(hidden.entries[0].availability, Availability::Checking);
    allow_new_probes(&root);
    release(&root);
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let opening = tokio::time::timeout(Duration::from_secs(10), service.begin_companion_review(&epoch)).await.unwrap();
    let BeginCompanionReviewResult::Ready { surface } = opening else { panic!("revealed unverified selection did not recover") };
    assert_eq!(surface.workspace.active_context_id.as_deref(), Some(id.as_str()));
    assert_eq!(surface.workspace.context_epoch, hidden.context_epoch);
    assert_eq!(surface.workspace.entries[0].availability, Availability::Available);
    assert!(matches!(surface.observation, Some(ObservationSnapshot::Ready { .. })));
    service.shutdown().await;
}

#[tokio::test]
async fn resumed_user_selection_does_not_wait_for_another_entry_initial_restoration() {
    use crate::companion::{BeginCompanionReviewResult, ReviewCaller};
    let (temp, root) = working_tree();
    let root = fs::canonicalize(root).unwrap();
    let other = other_repository(&temp.path().join("other"));
    let file = temp.path().join("workspace.json");
    seed(&file, &[root.clone(), other.clone()], None);
    let parked = temp.path().join("parked");
    fs::rename(&root, &parked).unwrap();
    block(&other);
    let service = RepositoryService::load_workspace(file, GitProbe::with_executable(&gated_git(temp.path()))).await;
    wait_for_probe(&other).await;
    let initial = service.snapshot().await;
    assert!(initial.restoring);
    assert_eq!(initial.entries[0].availability, Availability::Unavailable);
    let id = initial.entries[0].id.clone();
    block(&parked);
    fs::rename(&parked, &root).unwrap();
    service.select(&id).await;
    wait_for_probe(&root).await;
    service.set_surface_visibility(ReviewCaller::Main, false);
    wait_for_reaped_child(&root).await;
    allow_new_probes(&root);
    release(&root);
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let opening = tokio::time::timeout(Duration::from_secs(10), service.begin_companion_review(&epoch)).await.unwrap();
    let BeginCompanionReviewResult::Ready { surface } = opening else { panic!("selected recovery was blocked by unrelated restoration") };
    assert!(surface.workspace.restoring);
    assert_eq!(surface.workspace.active_context_id.as_deref(), Some(id.as_str()));
    assert_eq!(surface.workspace.entries[0].availability, Availability::Available);
    assert!(matches!(surface.observation, Some(ObservationSnapshot::Ready { .. })));
    allow_new_probes(&other);
    release(&other);
    service.shutdown().await;
}
