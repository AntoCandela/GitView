//! Exercises selected-context scan ordering and child lifecycle.

use std::fs;
use std::time::Duration;

use crate::application::RepositoryService;
use crate::browsing::{RepositoryFileResult, RepositoryFilesRequest, RepositoryFilesResult};
use crate::git::status::GitStatusReader;
use crate::test_support::{executable, git, quote, wait_for_probe, wait_for_reaped_child, working_tree};
use crate::workspace::OpenOutcome;
use crate::diagnostics::{Code, DiagnosticStore, Query, ReadOnlyDiagnostics};
use super::*;

async fn selected_service(root: &std::path::Path, executable: &std::path::Path) -> (RepositoryService, String) {
    let service = RepositoryService::with_status_reader(GitStatusReader::with_executable(executable));
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(root).await else { panic!("fixture admission failed") };
    service.select(&entry_id).await;
    (service, entry_id)
}

fn gated_status(at: &std::path::Path) -> std::path::PathBuf {
    executable(at, &format!(r#"
case " $* " in *" status "*)
    printf '%s\n' "$$" > .gitview-child-pid
    printf 'scan\n' >> {}
    : > .gitview-entered
    git "$@" > .gitview-status-capture
    while [ ! -f .gitview-release ]; do sleep 0.01; done
    cat .gitview-status-capture
    exit 0
;; esac
exec git "$@"
"#, quote(&at.join("scans"))))
}

async fn publish_validated_scan(controller: &ObservationController, context: &SelectedContext) -> ObservationSnapshot {
    let result = scan(context, &GitProbe::default(), &GitStatusReader::default()).await;
    let mut state = controller.state.lock();
    let entry = state.entries.entry(context.entry_id.clone()).or_default();
    entry.publish(&context.entry_id, result);
    entry.snapshot.clone().unwrap()
}

#[tokio::test]
async fn unchanged_validated_status_keeps_review_authority_but_category_path_and_error_changes_invalidate_it() {
    let (_temp, root) = working_tree();
    fs::write(root.join("tracked.txt"), b"original\n").unwrap();
    crate::test_support::commit(&root);
    fs::write(root.join("tracked.txt"), b"first edit\n").unwrap();
    let facts = GitProbe::default().probe(&root).await.unwrap();
    let identity = NativeIdentity::capture(&facts.root, &facts.git_dir).unwrap();
    let context = SelectedContext { entry_id: "fixture".to_owned(), root: facts.root, git_dir: facts.git_dir, kind: facts.kind, identity };
    let controller = ObservationController::default();
    {
        let mut state = controller.state.lock();
        state.selected_id = Some(context.entry_id.clone());
        state.observing = true;
    }
    let first = publish_validated_scan(&controller, &context).await;
    let ObservationSnapshot::Ready { observation_revision: revision, files, .. } = &first else { panic!("initial edit not observed") };
    let path = &files[0];
    let (generation, _) = controller.authorize_review(&context.entry_id, *revision, &path.path_id, ReviewCategory::Unstaged).unwrap();
    for _ in 0..3 {
        assert_eq!(publish_validated_scan(&controller, &context).await, first);
        assert_eq!(controller.validate_review(generation, &context.entry_id, *revision, &path.path_id, ReviewCategory::Unstaged), Ok(()));
    }
    fs::write(root.join("tracked.txt"), b"second edit, same status\n").unwrap();
    assert_eq!(publish_validated_scan(&controller, &context).await, first);
    assert!(controller.authorize_review(&context.entry_id, *revision, &path.path_id, ReviewCategory::Unstaged).is_ok());

    git(&root, &["add", "tracked.txt"]);
    let ObservationSnapshot::Ready { observation_revision: staged_revision, files: staged_files, .. } = publish_validated_scan(&controller, &context).await
        else { panic!("staged category not observed") };
    let staged = &staged_files[0];
    assert!(staged_revision > *revision);
    assert_eq!(staged.stable_path_id, path.stable_path_id);
    assert_ne!(staged.path_id, path.path_id);
    assert_eq!(controller.validate_review(generation, &context.entry_id, *revision, &path.path_id, ReviewCategory::Unstaged), Err(ReviewResult::StaleObservation));
    assert!(controller.authorize_review(&context.entry_id, staged_revision, &staged.path_id, ReviewCategory::Staged).is_ok());

    fs::write(root.join("new.txt"), b"new path\n").unwrap();
    let ObservationSnapshot::Ready { observation_revision: added_revision, files: added_files, .. } = publish_validated_scan(&controller, &context).await
        else { panic!("new path not observed") };
    assert!(added_files.iter().any(|file| file.display_path == "new.txt"));
    assert_eq!(controller.validate_review(generation, &context.entry_id, staged_revision, &staged.path_id, ReviewCategory::Staged), Err(ReviewResult::StaleObservation));
    let tracked = added_files.iter().find(|file| file.display_path == "tracked.txt").unwrap();
    fs::remove_file(root.join("tracked.txt")).unwrap();
    let ObservationSnapshot::Ready { observation_revision: deleted_revision, files: deleted_files, .. } = publish_validated_scan(&controller, &context).await
        else { panic!("deleted status not observed") };
    assert!(deleted_files.iter().any(|file| file.display_path == "tracked.txt" && file.unstaged == Some(crate::git::status::ChangeKind::Deleted)));
    assert_eq!(controller.validate_review(generation, &context.entry_id, added_revision, &tracked.path_id, ReviewCategory::Staged), Err(ReviewResult::StaleObservation));

    controller.state.lock().entries.get_mut(&context.entry_id).unwrap().publish(&context.entry_id, Err(ObservationErrorCode::Inaccessible));
    assert!(matches!(controller.snapshot(&context.entry_id), ObservationSnapshot::Unavailable { .. }));
    let ObservationSnapshot::Ready { observation_revision: recovered_revision, .. } = publish_validated_scan(&controller, &context).await
        else { panic!("validated ready state did not recover") };
    assert!(recovered_revision > deleted_revision);
    let deleted = deleted_files.iter().find(|file| file.display_path == "tracked.txt").unwrap();
    assert_eq!(controller.authorize_review(&context.entry_id, deleted_revision, &deleted.path_id, ReviewCategory::Staged).unwrap_err(), ReviewResult::StaleObservation);
}

#[tokio::test]
async fn slow_scans_coalesce_and_cached_reads_do_not_queue_work() {
    let (temp, root) = working_tree();
    let (service, id) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    for _ in 0..30 {
        assert!(matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Checking { .. }));
    }
    tokio::time::sleep(Duration::from_millis(2200)).await;
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    fs::write(root.join(".gitview-release"), b"").unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Ready { .. }) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
}

#[tokio::test]
async fn switching_cancels_child_and_late_old_results_never_publish() {
    let (temp, root) = working_tree();
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    git(&other, &["init", "-b", "other"]);
    fs::write(root.join("old.txt"), b"old context").unwrap();
    let (service, id) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    let OpenOutcome::Opened { entry_id: other_id, .. } = service.open_chosen(&other).await else { panic!("second admission failed") };
    fs::write(other.join(".gitview-release"), b"").unwrap();
    tokio::time::timeout(Duration::from_secs(1), service.select(&other_id)).await.unwrap();
    wait_for_reaped_child(&root).await;
    assert!(matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Unavailable { .. }));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let ObservationSnapshot::Ready { entry_id, files, .. } = service.observe_selected_context(&other_id).await {
                assert_eq!(entry_id, other_id);
                assert!(!files.iter().any(|file| file.display_path == "old.txt"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn exhausted_persistent_identity_registry_reports_resource_limit_after_paths_disappear() {
    let (temp, root) = working_tree();
    let output = temp.path().join("status-output");
    let mut initial = Vec::new();
    for index in 0..MAX_REGISTERED_PATHS {
        initial.extend_from_slice(format!("? path-{index}\0").as_bytes());
    }
    fs::write(&output, initial).unwrap();
    let executable = executable(temp.path(), &format!(r#"
case " $* " in *" status "*) cat {}; exit 0;; esac
exec git "$@"
"#, quote(&output)));
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let context = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::SelectContext);
    let (service, id) = context.scope(selected_service(&root, &executable)).await;
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let ObservationSnapshot::Ready { files, .. } = service.observe_selected_context(&id).await {
                assert_eq!(files.len(), MAX_REGISTERED_PATHS);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    fs::write(&output, b"? previously-unseen\0").unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let ObservationSnapshot::Unavailable { error_code, .. } = service.observe_selected_context(&id).await {
                assert_eq!(error_code, ObservationErrorCode::ResourceLimit);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    service.shutdown().await;
    store.flush(Duration::from_secs(2)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { component: Some(Component::Observation), limit: 200, ..Default::default() }).unwrap().events;
    assert!(rows.iter().any(|row| row.event == Event::Failed && row.code == Some(Code::OutputLimit)));
    assert!(!rows.iter().any(|row| row.event == Event::Completed && rows.iter().any(|failure| failure.event == Event::Failed && failure.operation_id == row.operation_id)));
    store.shutdown(Duration::from_secs(2)).unwrap();
}

#[tokio::test]
async fn same_id_reselection_resets_revision_and_drop_reaps_the_new_scan() {
    let (temp, root) = working_tree();
    let (service, id) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    let ObservationSnapshot::Checking { observation_revision: before, .. } = service.observe_selected_context(&id).await else { panic!("blocked scan must remain checking") };
    service.select(&id).await;
    let ObservationSnapshot::Checking { observation_revision: after, .. } = service.observe_selected_context(&id).await else { panic!("same-ID selection did not reset observation") };
    assert!(after > before);
    tokio::time::timeout(Duration::from_secs(5), async {
        while fs::read_to_string(temp.path().join("scans")).unwrap().lines().count() != 2 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    drop(service);
    wait_for_reaped_child(&root).await;
}

#[tokio::test]
async fn companion_open_during_old_scan_waits_for_a_new_start_even_when_status_is_unchanged() {
    use crate::companion::{BeginCompanionReviewResult, ReviewCaller};
    let (temp, root) = working_tree();
    fs::write(root.join(".git/info/exclude"), b".gitview-*\n").unwrap();
    let (service, _) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let opening = service.begin_companion_review(&epoch);
    tokio::pin!(opening);
    assert!(tokio::time::timeout(Duration::from_millis(40), &mut opening).await.is_err());
    fs::write(root.join(".gitview-release"), b"").unwrap();
    let BeginCompanionReviewResult::Ready { surface } = tokio::time::timeout(Duration::from_secs(5), opening).await.unwrap() else { panic!("fresh opening failed") };
    assert!(matches!(surface.observation, Some(ObservationSnapshot::Ready { files, .. }) if files.is_empty()));
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\nscan\n");
}

#[tokio::test]
async fn both_hidden_stops_scans_but_main_reveal_retains_unchanged_file_authority() {
    use crate::companion::{CompanionCode, ReviewCaller};
    let (temp, root) = working_tree();
    fs::write(root.join(".git/info/exclude"), b".gitview-*\n").unwrap();
    fs::write(root.join("unchanged.txt"), b"retained working text\n").unwrap();
    crate::test_support::commit(&root);
    let (service, id) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    let RepositoryFilesResult::Files { listing_id, files, .. } = service.list_repository_files(&id, RepositoryFilesRequest::default()).await
        else { panic!("repository listing unavailable") };
    let file = files.iter().find(|file| file.display_path == "unchanged.txt").unwrap();
    let expected = RepositoryFileResult::Text {
        entry_id: id.clone(), listing_id: listing_id.clone(), file_id: file.id.clone(),
        display_path: "unchanged.txt".into(), content: "retained working text\n".into(),
    };
    assert_eq!(service.review_repository_file(&id, &listing_id, &file.id).await, expected);
    let context_epoch = service.snapshot().await.context_epoch;
    service.set_companion_available(true);
    service.set_surface_visibility(ReviewCaller::Companion, true);
    service.reconcile_surface_demand().await;
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    let scope = service.capture_surface_scope(ReviewCaller::Companion).unwrap();
    service.set_surface_visibility(ReviewCaller::Companion, false);
    assert!(matches!(service.run_surface_request(&scope, std::future::pending::<()>()).await, Err(CompanionCode::StaleSurface)));
    assert!(matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Checking { .. }));
    service.set_surface_visibility(ReviewCaller::Main, false);
    service.reconcile_surface_demand().await;
    wait_for_reaped_child(&root).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    assert_eq!(service.review_repository_file(&id, &listing_id, &file.id).await, RepositoryFileResult::StaleSelection);
    fs::write(root.join(".gitview-release"), b"").unwrap();
    service.set_surface_visibility(ReviewCaller::Main, true);
    // Renderer preview reads can overtake the host's asynchronous scan reconciliation.
    assert_eq!(service.review_repository_file(&id, &listing_id, &file.id).await, expected);
    assert_eq!(service.snapshot().await.context_epoch, context_epoch);
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    service.reconcile_surface_demand().await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Ready { .. }) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\nscan\n");
    assert_eq!(service.snapshot().await.context_epoch, context_epoch);
    assert_eq!(service.review_repository_file(&id, &listing_id, &file.id).await, expected);
    service.shutdown().await;
}

#[tokio::test]
async fn reopening_with_unchanged_status_finishes_without_a_semantic_revision_bump() {
    use crate::companion::{BeginCompanionReviewResult, ReviewCaller};
    let (_temp, root) = working_tree();
    let service = RepositoryService::new();
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("admission failed") };
    service.select(&entry_id).await;
    service.set_companion_available(true);
    let first_epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let BeginCompanionReviewResult::Ready { surface: first } = service.begin_companion_review(&first_epoch).await else { panic!("first open failed") };
    service.set_surface_visibility(ReviewCaller::Companion, false);
    let second_epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    assert_ne!(first_epoch, second_epoch);
    let BeginCompanionReviewResult::Ready { surface: second } = service.begin_companion_review(&second_epoch).await else { panic!("unchanged open failed") };
    assert_eq!(first.observation, second.observation);
    assert_eq!(first.workspace.context_epoch, second.workspace.context_epoch);
}

#[tokio::test]
async fn hiding_an_open_waiter_cancels_it_while_visible_main_keeps_its_scan() {
    use crate::companion::{BeginCompanionReviewResult, CompanionCode, ReviewCaller};
    let (temp, root) = working_tree();
    let (service, _) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    service.set_companion_available(true);
    let epoch = service.set_surface_visibility(ReviewCaller::Companion, true);
    let opening = service.begin_companion_review(&epoch);
    tokio::pin!(opening);
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut opening).await.is_err());
    service.set_surface_visibility(ReviewCaller::Companion, false);
    assert!(matches!(opening.await, BeginCompanionReviewResult::Stale { code: CompanionCode::StaleSurface }));
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    service.shutdown().await;
    wait_for_reaped_child(&root).await;
}

async fn assert_hidden_main_has_no_fallback_scan_demand(initially_available: bool) {
    use crate::companion::ReviewCaller;
    let (temp, root) = working_tree();
    let (service, id) = selected_service(&root, &gated_status(temp.path())).await;
    wait_for_probe(&root).await;
    service.set_companion_available(initially_available);
    service.set_surface_visibility(ReviewCaller::Main, false);
    service.set_companion_available(false);
    assert!(matches!(service.capture_surface_scope(ReviewCaller::Main), Err(crate::companion::CompanionCode::NotVisible)));
    service.reconcile_surface_demand().await;
    wait_for_reaped_child(&root).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\n");
    fs::write(root.join(".gitview-release"), b"").unwrap();
    service.set_surface_visibility(ReviewCaller::Main, true);
    service.reconcile_surface_demand().await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while !matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Ready { .. }) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.unwrap();
    assert_eq!(fs::read_to_string(temp.path().join("scans")).unwrap(), "scan\nscan\n");
    service.shutdown().await;
}

#[tokio::test]
async fn hidden_main_with_companion_disabled_stops_native_scans_until_main_is_visible() {
    assert_hidden_main_has_no_fallback_scan_demand(false).await;
}

#[tokio::test]
async fn companion_becoming_unavailable_does_not_restore_hidden_main_scan_demand() {
    assert_hidden_main_has_no_fallback_scan_demand(true).await;
}

#[tokio::test]
async fn hidden_same_id_reselection_rejects_the_retained_listing_after_reveal() {
    use crate::companion::ReviewCaller;
    let (_temp, root) = working_tree();
    fs::write(root.join("unchanged.txt"), b"unchanged\n").unwrap();
    crate::test_support::commit(&root);
    let service = RepositoryService::new();
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("admission failed") };
    service.select(&entry_id).await;
    let RepositoryFilesResult::Files { listing_id, files, .. } = service.list_repository_files(&entry_id, RepositoryFilesRequest::default()).await
        else { panic!("repository listing unavailable") };
    let file = files.iter().find(|file| file.display_path == "unchanged.txt").unwrap();
    assert!(matches!(service.review_repository_file(&entry_id, &listing_id, &file.id).await, RepositoryFileResult::Text { .. }));
    let context_epoch = service.snapshot().await.context_epoch;

    service.set_surface_visibility(ReviewCaller::Main, false);
    service.select(&entry_id).await;
    assert_ne!(service.snapshot().await.context_epoch, context_epoch);
    assert!(service.review_surface_snapshot(ReviewCaller::Main).await.observation.is_none());
    service.set_surface_visibility(ReviewCaller::Main, true);
    assert_eq!(service.review_repository_file(&entry_id, &listing_id, &file.id).await, RepositoryFileResult::StaleSelection);
    service.reconcile_surface_demand().await;

    assert_eq!(service.review_repository_file(&entry_id, &listing_id, &file.id).await, RepositoryFileResult::StaleSelection);
    assert!(matches!(service.list_repository_files(&entry_id, RepositoryFilesRequest {
        listing_id: Some(listing_id), ..Default::default()
    }).await, RepositoryFilesResult::StaleSelection));
    let RepositoryFilesResult::Files { listing_id, files, .. } = service.list_repository_files(&entry_id, RepositoryFilesRequest::default()).await
        else { panic!("replacement selection could not list files") };
    let file = files.iter().find(|file| file.display_path == "unchanged.txt").unwrap();
    assert!(matches!(service.review_repository_file(&entry_id, &listing_id, &file.id).await,
        RepositoryFileResult::Text { content, .. } if content == "unchanged\n"));
    service.shutdown().await;
}

#[tokio::test]
async fn resumed_scan_cannot_complete_a_ticket_from_the_cancelled_producer() {
    let (temp, root) = working_tree();
    let facts = GitProbe::default().probe(&root).await.unwrap();
    let identity = NativeIdentity::capture(&facts.root, &facts.git_dir).unwrap();
    let context = || SelectedContext {
        entry_id: "fixture".into(), root: facts.root.clone(), git_dir: facts.git_dir.clone(),
        kind: facts.kind, identity: identity.clone(),
    };
    let controller = ObservationController::with_reader(GitStatusReader::with_executable(&gated_status(temp.path())));
    controller.select(context(), "context-1".into());
    wait_for_probe(&root).await;
    let old_ticket = controller.request_fresh_scan("fixture").unwrap();
    controller.set_demand(false);
    wait_for_reaped_child(&root).await;
    controller.set_demand(true);
    controller.select(context(), "context-1".into());
    let new_ticket = controller.request_fresh_scan("fixture").unwrap();
    assert!(!tokio::time::timeout(Duration::from_secs(1), controller.await_fresh_scan(old_ticket)).await.unwrap());
    fs::write(root.join(".gitview-release"), b"").unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(5), controller.await_fresh_scan(new_ticket)).await.unwrap());
    assert!(matches!(controller.snapshot("fixture"), ObservationSnapshot::Ready { .. }));
    if let Some(task) = controller.stop() { let _ = task.await; }
}
