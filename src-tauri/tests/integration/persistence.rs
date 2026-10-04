//! Exercises private workspace choices through production construction and real Git repositories.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gitview_lib::application::RepositoryService;
use gitview_lib::observation::ObservationSnapshot;
use gitview_lib::workspace::{Availability, EntryKind, HeadLabel, MutationOutcome, OpenOutcome, SelectOutcome, WorkspaceSnapshot};
use gitview_lib::workspace::persistence::PersistenceErrorCode;
use serde_json::{json, Value};

#[path = "../support/mod.rs"]
mod support;
use support::git;


fn repository(at: &Path, branch: &str) -> PathBuf {
    fs::create_dir(at).unwrap();
    git(at, &["init", "-b", branch]);
    git(at, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "--allow-empty", "-m", "initial"]);
    fs::canonicalize(at).unwrap()
}

fn opened(outcome: OpenOutcome) -> (String, WorkspaceSnapshot) {
    match outcome {
        OpenOutcome::Opened { entry_id, snapshot } => (entry_id, snapshot),
        other => panic!("expected admission, got {other:?}"),
    }
}

async fn wait_for(service: &RepositoryService, predicate: impl Fn(&WorkspaceSnapshot) -> bool) -> WorkspaceSnapshot {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let snapshot = service.snapshot().await;
            if predicate(&snapshot) {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("workspace did not reach the expected state")
}

fn saved(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[tokio::test]
async fn restart_restores_order_selected_linked_context_and_fresh_facts() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("main"), "main");
    let linked = temp.path().join("linked");
    git(&root, &["worktree", "add", "-b", "feature", linked.to_str().unwrap()]);
    let linked = fs::canonicalize(linked).unwrap();
    let bare = temp.path().join("archive.git");
    // Git's repository operands are CLI paths, not Windows verbatim filesystem paths.
    git(temp.path(), &["clone", "--bare", "main", "archive.git"]);
    let bare = fs::canonicalize(bare).unwrap();
    let file = temp.path().join("private").join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let (main_id, _) = opened(service.open_chosen(&root).await);
    let (linked_id, _) = opened(service.open_chosen(&linked).await);
    let (bare_id, _) = opened(service.open_chosen(&bare).await);
    assert!(matches!(service.select(&linked_id).await, SelectOutcome::Selected { .. }));
    let before = saved(&file);
    assert_eq!(before, json!({
        "version": 1,
        "repositories": [{"root": root}, {"root": linked}, {"root": bare}],
        "activeRoot": linked,
    }));
    drop(service);
    let before_bytes = fs::read(&file).unwrap();
    git(&linked, &["branch", "-m", "fresh-feature"]);

    let restored = RepositoryService::with_workspace_file(file.clone()).await;
    let initial = restored.snapshot().await;
    assert!(initial.restoring);
    assert_eq!(initial.entries.iter().map(|entry| entry.kind.clone()).collect::<Vec<_>>(), vec![EntryKind::Unknown; 3]);
    assert!(initial.entries.iter().all(|entry| entry.head == HeadLabel::Unknown));
    assert_eq!(initial.active_context_id.as_deref(), Some(initial.entries[1].id.as_str()));
    assert_ne!(initial.entries[0].id, main_id);
    assert_ne!(initial.entries[1].id, linked_id);
    assert_ne!(initial.entries[2].id, bare_id);
    let complete = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert_eq!(complete.entries.iter().map(|entry| entry.location_label.as_str()).collect::<Vec<_>>(),
        vec![root.to_str().unwrap(), linked.to_str().unwrap(), bare.to_str().unwrap()]);
    assert!(complete.entries.iter().all(|entry| entry.availability == Availability::Available));
    assert_eq!(complete.entries[1].head, HeadLabel::Branch { name: "fresh-feature".into() });
    assert_eq!(complete.entries[2].kind, EntryKind::Bare);
    assert_eq!(complete.active_context_id, initial.active_context_id);
    assert_eq!(fs::read(&file).unwrap(), before_bytes);

    let linked_nested = linked.join("nested");
    fs::create_dir(&linked_nested).unwrap();
    let OpenOutcome::Reused { entry_id, snapshot } = restored.open_chosen(&linked_nested).await else {
        panic!("restored linked context was not reused");
    };
    assert_eq!(entry_id, complete.entries[1].id);
    assert_eq!(snapshot.entries.len(), 3);
    #[cfg(unix)]
    {
        let alias = temp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        assert!(matches!(restored.open_chosen(&alias).await, OpenOutcome::Reused { entry_id, .. } if entry_id == complete.entries[0].id));
    }
    assert!(matches!(restored.select(&complete.entries[2].id).await, SelectOutcome::Selected { .. }));
    assert_eq!(saved(&file)["activeRoot"], json!(bare));
}

#[tokio::test]
async fn first_launch_and_nullable_selection_do_not_synthesize_a_choice() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let initial = service.snapshot().await;
    assert!(initial.entries.is_empty());
    assert!(!initial.restoring);
    assert_eq!(initial.active_context_id, None);
    assert_eq!(initial.persistence_error, None);
    assert!(!file.exists());
    opened(service.open_chosen(&root).await);
    assert_eq!(saved(&file)["activeRoot"], Value::Null);
    drop(service);
    let restored = RepositoryService::with_workspace_file(file).await;
    let complete = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert_eq!(complete.active_context_id, None);
    assert_eq!(complete.entries[0].availability, Availability::Available);
    assert!(matches!(restored.observe_selected_context(&complete.entries[0].id).await, ObservationSnapshot::Unavailable { .. }));
}

#[tokio::test]
async fn selected_missing_location_is_retained_and_recovers_without_renderer_refresh() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let (id, _) = opened(service.open_chosen(&root).await);
    service.select(&id).await;
    service.shutdown().await;
    drop(service);
    let parked = temp.path().join("parked");
    fs::rename(&root, &parked).unwrap();
    let original = fs::read(&file).unwrap();

    let restored = RepositoryService::with_workspace_file(file.clone()).await;
    let failed = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert_eq!(failed.entries[0].kind, EntryKind::Unknown);
    assert_eq!(failed.entries[0].head, HeadLabel::Unknown);
    assert_eq!(failed.entries[0].availability, Availability::Unavailable);
    assert_eq!(failed.active_context_id.as_deref(), Some(failed.entries[0].id.as_str()));
    fs::rename(&parked, &root).unwrap();
    let recovered = wait_for(&restored, |snapshot| snapshot.entries[0].availability == Availability::Available).await;
    assert_eq!(recovered.entries[0].id, failed.entries[0].id);
    assert_eq!(recovered.entries[0].kind, EntryKind::WorkingTree);
    assert_eq!(recovered.entries[0].head, HeadLabel::Branch { name: "main".into() });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(restored.observe_selected_context(&recovered.entries[0].id).await, ObservationSnapshot::Ready { .. }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    assert_eq!(fs::read(file).unwrap(), original);
}

#[tokio::test]
async fn corrupt_and_unsupported_files_remain_unchanged_after_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    for (name, bytes, code) in [
        ("corrupt.json", b"{broken".as_slice(), PersistenceErrorCode::LoadFailed),
        ("future.json", b"{\"version\":2,\"repositories\":[],\"activeRoot\":null}".as_slice(), PersistenceErrorCode::UnsupportedVersion),
    ] {
        let file = temp.path().join(name);
        fs::write(&file, bytes).unwrap();
        let service = RepositoryService::with_workspace_file(file.clone()).await;
        assert_eq!(service.snapshot().await.persistence_error.unwrap().code, code);
        let (id, opened) = opened(service.open_chosen(&root).await);
        assert_eq!(opened.persistence_error.unwrap().code, code);
        let SelectOutcome::Selected { snapshot } = service.select(&id).await else {
            panic!("failed load prevented in-memory navigation");
        };
        assert_eq!(snapshot.active_context_id, Some(id.clone()));
        assert_eq!(snapshot.persistence_error.unwrap().code, code);
        let MutationOutcome::Updated { snapshot: renamed } = service.rename(&id, "Private label").await else {
            panic!("failed load prevented session renaming");
        };
        assert_eq!(renamed.persistence_error.unwrap().code, code);
        let MutationOutcome::Updated { snapshot: removed } = service.remove(&id).await else {
            panic!("failed load prevented session removal");
        };
        assert!(removed.entries.is_empty());
        assert_eq!(removed.persistence_error.unwrap().code, code);
        assert_eq!(fs::read(&file).unwrap(), bytes);
    }
}

#[tokio::test]
async fn failed_save_retries_on_next_successful_choice_and_clears_warning() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    fs::create_dir(&file).unwrap();
    let (id, failed) = opened(service.open_chosen(&root).await);
    assert_eq!(failed.persistence_error.unwrap().code, PersistenceErrorCode::SaveFailed);
    fs::remove_dir(&file).unwrap();
    let SelectOutcome::Selected { snapshot } = service.select(&id).await else {
        panic!("available context could not be selected after a save failure");
    };
    assert_eq!(snapshot.persistence_error, None);
    assert_eq!(saved(&file), json!({ "version": 1, "repositories": [{ "root": root }], "activeRoot": root }));
    drop(service);
    let restarted = RepositoryService::with_workspace_file(file).await;
    let restored = wait_for(&restarted, |snapshot| !snapshot.restoring).await;
    assert_eq!(restored.active_context_id.as_deref(), Some(restored.entries[0].id.as_str()));
}

#[tokio::test]
async fn cancelled_rejected_unknown_selection_and_fact_refresh_do_not_write_choices() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let (id, _) = opened(service.open_chosen(&root).await);
    service.select(&id).await;
    let bytes = fs::read(&file).unwrap();
    assert!(matches!(service.cancelled().await, OpenOutcome::Cancelled { .. }));
    assert!(matches!(service.open_chosen(temp.path()).await, OpenOutcome::Rejected { .. }));
    assert!(matches!(service.select("not-admitted").await, SelectOutcome::NotFound { .. }));
    git(&root, &["branch", "-m", "refreshed"]);
    assert_eq!(service.refresh(&id).await.entries[0].head, HeadLabel::Branch { name: "refreshed".into() });
    assert_eq!(fs::read(file).unwrap(), bytes);
}

#[tokio::test]
async fn restored_verified_identity_does_not_accept_a_replacement_at_the_same_location() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "original");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    opened(service.open_chosen(&root).await);
    drop(service);
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let restored = wait_for(&service, |snapshot| !snapshot.restoring).await;
    let bytes = fs::read(&file).unwrap();
    fs::rename(&root, temp.path().join("original")).unwrap();
    repository(&root, "replacement");
    let unavailable = service.refresh(&restored.entries[0].id).await;
    assert_eq!(unavailable.entries[0].availability, Availability::Unavailable);
    assert_eq!(unavailable.entries[0].head, HeadLabel::Branch { name: "original".into() });
    assert!(matches!(service.open_chosen(&root).await, OpenOutcome::Rejected { code: gitview_lib::git::GitError::RepositoryChanged, .. }));
    assert_eq!(fs::read(file).unwrap(), bytes);
}

#[tokio::test]
async fn app_display_name_survives_refresh_reuse_and_restart_without_renaming_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = repository(&temp.path().join("project"), "main");
    let tracked = root.join("unchanged.txt");
    fs::write(&tracked, b"repository content").unwrap();
    let head_before = fs::read(root.join(".git").join("HEAD")).unwrap();
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let (id, _) = opened(service.open_chosen(&root).await);
    let MutationOutcome::Updated { snapshot: renamed } = service.rename(&id, "  Work context  ").await else {
        panic!("valid display name was rejected");
    };
    assert_eq!(renamed.entries[0].id, id);
    assert_eq!(renamed.entries[0].repository_label, "Work context");
    assert_eq!(renamed.entries[0].location_label, root.to_str().unwrap());
    assert_eq!(saved(&file), json!({
        "version": 1, "repositories": [{ "root": root, "displayName": "Work context" }], "activeRoot": null,
    }));
    assert_eq!(fs::read(&tracked).unwrap(), b"repository content");
    assert_eq!(fs::read(root.join(".git").join("HEAD")).unwrap(), head_before);
    assert!(!temp.path().join("Work context").exists());
    let bytes = fs::read(&file).unwrap();
    assert!(matches!(service.rename(&id, " \t\n ").await, MutationOutcome::Rejected { code: gitview_lib::workspace::WorkspaceRejectionCode::InvalidDisplayName, .. }));
    assert!(matches!(service.rename("unknown", "Another").await, MutationOutcome::NotFound { .. }));
    assert!(matches!(service.remove("unknown").await, MutationOutcome::NotFound { .. }));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    git(&root, &["branch", "-m", "new-head"]);
    assert_eq!(service.refresh(&id).await.entries[0].repository_label, "Work context");
    let OpenOutcome::Reused { snapshot, .. } = service.open_chosen(&root).await else {
        panic!("renamed context was not reused");
    };
    assert_eq!(snapshot.entries[0].repository_label, "Work context");
    drop(service);
    let restored = RepositoryService::with_workspace_file(file).await;
    assert_eq!(restored.snapshot().await.entries[0].repository_label, "Work context");
    let complete = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert_eq!(complete.entries[0].repository_label, "Work context");
    assert_eq!(complete.entries[0].head, HeadLabel::Branch { name: "new-head".into() });
    assert_eq!(complete.entries[0].location_label, root.to_str().unwrap());
    assert_eq!(fs::read(tracked).unwrap(), b"repository content");
}

#[tokio::test]
async fn removal_preserves_files_and_inactive_selection_then_chooses_first_available_or_null() {
    let temp = tempfile::tempdir().unwrap();
    let unavailable = repository(&temp.path().join("unavailable"), "missing");
    let active = repository(&temp.path().join("active"), "active");
    let replacement = repository(&temp.path().join("replacement"), "replacement");
    let inactive = repository(&temp.path().join("inactive"), "inactive");
    let file = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(file.clone()).await;
    let (missing_id, _) = opened(service.open_chosen(&unavailable).await);
    let (active_id, _) = opened(service.open_chosen(&active).await);
    let (replacement_id, _) = opened(service.open_chosen(&replacement).await);
    let (inactive_id, _) = opened(service.open_chosen(&inactive).await);
    service.select(&active_id).await;
    let parked = temp.path().join("parked");
    fs::rename(&unavailable, &parked).unwrap();
    service.refresh(&missing_id).await;
    let MutationOutcome::Updated { snapshot } = service.remove(&inactive_id).await else {
        panic!("inactive choice could not be removed");
    };
    assert_eq!(snapshot.active_context_id, Some(active_id.clone()));
    let MutationOutcome::Updated { snapshot } = service.remove(&active_id).await else {
        panic!("active choice could not be removed");
    };
    assert_eq!(snapshot.active_context_id, Some(replacement_id));
    assert_eq!(snapshot.entries.iter().map(|entry| entry.location_label.as_str()).collect::<Vec<_>>(),
        vec![unavailable.to_str().unwrap(), replacement.to_str().unwrap()]);
    assert_eq!(saved(&file), json!({
        "version": 1, "repositories": [{ "root": unavailable }, { "root": replacement }], "activeRoot": replacement,
    }));
    assert!(matches!(service.observe_selected_context(&active_id).await, ObservationSnapshot::Unavailable { .. }));
    drop(service);
    let restored = RepositoryService::with_workspace_file(file.clone()).await;
    let ready = wait_for(&restored, |snapshot| !snapshot.restoring).await;
    assert_eq!(ready.entries[0].availability, Availability::Unavailable);
    assert_eq!(ready.active_context_id.as_deref(), Some(ready.entries[1].id.as_str()));
    let MutationOutcome::Updated { snapshot: no_active } = restored.remove(&ready.entries[1].id).await else {
        panic!("replacement choice could not be removed");
    };
    assert_eq!(no_active.active_context_id, None);
    assert_eq!(saved(&file)["activeRoot"], Value::Null);
    let MutationOutcome::Updated { snapshot: empty } = restored.remove(&ready.entries[0].id).await else {
        panic!("last unavailable choice could not be removed");
    };
    assert!(empty.entries.is_empty());
    assert_eq!(saved(&file), json!({ "version": 1, "repositories": [], "activeRoot": null }));
    for root in [&parked, &active, &replacement, &inactive] {
        assert!(root.join(".git").join("HEAD").is_file(), "removal changed repository files");
        assert!(Command::new("git").current_dir(root).args(["rev-parse", "--verify", "HEAD"]).output().unwrap().status.success());
    }
    drop(restored);
    assert!(RepositoryService::with_workspace_file(file).await.snapshot().await.entries.is_empty());
}
