//! Exercises automatic selected-context observations through real read-only Git operations.

use std::fs;
use std::path::Path;
use std::time::Duration;

use gitview_lib::application::RepositoryService;
use gitview_lib::diff::{ReviewCategory, ReviewResult};
use gitview_lib::observation::{ObservationSnapshot, ObservationErrorCode};
use gitview_lib::workspace::OpenOutcome;

#[path = "../support/mod.rs"]
mod support;
use support::git;


fn repository(root: &Path) {
    fs::create_dir(root).unwrap();
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("tracked.txt"), b"original\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "-m", "initial"]);
}

async fn admit(service: &RepositoryService, root: &Path) -> String {
    match service.open_chosen(root).await {
        OpenOutcome::Opened { entry_id, .. } => entry_id,
        result => panic!("unexpected admission: {result:?}"),
    }
}

async fn wait_for(service: &RepositoryService, id: &str, predicate: impl Fn(&ObservationSnapshot) -> bool) -> ObservationSnapshot {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let snapshot = service.observe_selected_context(id).await;
            if predicate(&snapshot) { return snapshot; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("automatic observation did not converge")
}

#[tokio::test]
async fn external_edits_preserve_status_authority_until_categories_change_and_restore_with_fresh_tokens() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    repository(&root);
    let service = RepositoryService::new();
    let id = admit(&service, &root).await;
    service.select(&id).await;
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if files.is_empty())).await;
    let index = fs::read(root.join(".git/index")).unwrap();
    let head = fs::read(root.join(".git/HEAD")).unwrap();
    let branch = fs::read(root.join(".git/refs/heads/main")).unwrap();

    fs::write(root.join("tracked.txt"), b"first edit\n").unwrap();
    // Leave the monitor without renderer reads; completion-paced scans can take longer than one interval.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let first = wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if !files.is_empty())).await;
    let ObservationSnapshot::Ready { files: first_files, observation_revision: first_revision, .. } = first else { unreachable!() };
    assert_eq!(first_files[0].display_path, "tracked.txt");
    assert!(first_files[0].unstaged.is_some());

    fs::write(root.join("tracked.txt"), b"second edit\n").unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    let second = service.observe_selected_context(&id).await;
    let ObservationSnapshot::Ready { files: second_files, observation_revision: second_revision, .. } = second else { unreachable!() };
    assert_eq!(first_revision, second_revision);
    assert_eq!(first_files[0].stable_path_id, second_files[0].stable_path_id);
    assert_eq!(first_files[0].path_id, second_files[0].path_id);
    let review = service.review_file(&id, first_revision, &first_files[0].path_id, ReviewCategory::Unstaged).await;
    #[cfg(unix)]
    {
        let ReviewResult::Text { hunks, .. } = review
            else { panic!("unchanged status authority did not permit current content") };
        assert!(hunks.iter().flat_map(|hunk| &hunk.lines).any(|line| line.text == "second edit"));
    }
    #[cfg(not(unix))]
    assert!(matches!(review, ReviewResult::Unsupported { reason: gitview_lib::diff::UnsupportedReason::Other, .. }));

    fs::write(root.join("tracked.txt"), b"original\n").unwrap();
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if files.is_empty())).await;
    fs::write(root.join("tracked.txt"), b"reappeared\n").unwrap();
    let reappeared = wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if !files.is_empty())).await;
    let ObservationSnapshot::Ready { files, .. } = reappeared else { unreachable!() };
    assert_eq!(files[0].stable_path_id, first_files[0].stable_path_id);
    assert_ne!(files[0].path_id, first_files[0].path_id);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), branch);
    assert_eq!(fs::read(root.join("tracked.txt")).unwrap(), b"reappeared\n");
}

#[tokio::test]
async fn missing_root_recovers_but_replacement_at_same_path_is_never_clean() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let moved = temp.path().join("moved");
    repository(&root);
    let service = RepositoryService::new();
    let id = admit(&service, &root).await;
    service.select(&id).await;
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { .. })).await;
    fs::rename(&root, &moved).unwrap();
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Unavailable { .. })).await;
    fs::rename(&moved, &root).unwrap();
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if files.is_empty())).await;
    fs::rename(&root, &moved).unwrap();
    repository(&root);
    wait_for(&service, &id, |s| matches!(s, ObservationSnapshot::Unavailable { error_code: ObservationErrorCode::Inaccessible, .. })).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert!(matches!(service.observe_selected_context(&id).await, ObservationSnapshot::Unavailable { .. }));
}

#[tokio::test]
async fn linked_bare_and_unselected_contexts_have_distinct_honest_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("repository");
    let linked = temp.path().join("linked");
    let bare = temp.path().join("bare");
    repository(&root);
    git(&root, &["worktree", "add", "-b", "linked", linked.to_str().unwrap()]);
    fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    let service = RepositoryService::new();
    let main_id = admit(&service, &root).await;
    let linked_id = admit(&service, &linked).await;
    let bare_id = admit(&service, &bare).await;
    fs::write(linked.join("tracked.txt"), b"linked edit\n").unwrap();
    service.select(&main_id).await;
    wait_for(&service, &main_id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if files.is_empty())).await;
    assert!(matches!(service.observe_selected_context(&linked_id).await, ObservationSnapshot::Unavailable { .. }));
    service.select(&linked_id).await;
    wait_for(&service, &linked_id, |s| matches!(s, ObservationSnapshot::Ready { files, .. } if files.len() == 1 && files[0].display_path == "tracked.txt")).await;
    service.select(&bare_id).await;
    wait_for(&service, &bare_id, |s| matches!(s, ObservationSnapshot::Bare { .. })).await;
    assert!(matches!(service.observe_selected_context("unknown-id").await, ObservationSnapshot::Unavailable { .. }));
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_service_observation_reports_filtered_repositories_as_unavailable() {
    for operation in ["clean", "process"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repository");
        repository(&root);
        fs::write(root.join(".gitattributes"), b"tracked.txt filter=side_effect\n").unwrap();
        support::commit(&root);
        let marker = temp.path().join("observation-filter-ran");
        let filter = format!("touch {}; cat", support::quote(&marker));
        git(&root, &["config", &format!("filter.side_effect.{operation}"), &filter]);
        fs::write(root.join("tracked.txt"), b"modified\n").unwrap();
        let service = RepositoryService::new();
        let entry = admit(&service, &root).await;
        service.select(&entry).await;

        wait_for(&service, &entry, |snapshot| matches!(snapshot,
            ObservationSnapshot::Unavailable { error_code: ObservationErrorCode::UnsupportedConfiguration, .. })).await;

        assert!(!marker.exists());
        service.shutdown().await;
    }
}
