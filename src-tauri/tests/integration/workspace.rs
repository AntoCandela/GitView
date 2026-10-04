//! Exercises workspace identity, selection and availability through real temporary repositories.

use std::fs;

use gitview_lib::application::RepositoryService;
use gitview_lib::git::{GitProbe, Head};
use gitview_lib::git::GitError;
use gitview_lib::workspace::{
    Availability, EntryKind, HeadLabel, OpenOutcome, SelectOutcome,
};

#[path = "../support/mod.rs"]
mod support;
use support::{commit, git, unborn_working_tree as working_tree};

fn opened(outcome: OpenOutcome) -> (String, gitview_lib::workspace::WorkspaceSnapshot) {
    match outcome {
        OpenOutcome::Opened { entry_id, snapshot } => (entry_id, snapshot),
        other => panic!("expected an admitted repository, got {other:?}"),
    }
}

#[tokio::test]
async fn subdirectory_and_symlink_reuse_canonical_worktree_identity() {
    let (_temp, root) = working_tree();
    let nested = root.join("src");
    fs::create_dir(&nested).unwrap();
    let store = RepositoryService::new();
    let (id, first) = opened(store.open_chosen(&nested).await);
    assert_eq!(first.entries[0].kind, EntryKind::WorkingTree);
    assert_eq!(
        first.entries[0].head,
        HeadLabel::Unborn {
            name: "main".into()
        }
    );
    assert_eq!(first.active_context_id, None);

    #[cfg(unix)]
    let alias = {
        let alias = _temp.path().join("alias");
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        alias
    };
    #[cfg(not(unix))]
    let alias = root.clone();
    match store.open_chosen(&alias).await {
        OpenOutcome::Reused { entry_id, snapshot } => {
            assert_eq!(entry_id, id);
            assert_eq!(snapshot.entries.len(), 1);
            assert_eq!(snapshot.active_context_id, None);
        }
        other => panic!("alias was not reused: {other:?}"),
    }
}

#[tokio::test]
async fn linked_worktrees_and_bare_repositories_have_distinct_contexts() {
    let (temp, root) = working_tree();
    commit(&root);
    // Linked worktrees share objects, but their per-worktree Git directories define separate IDs.
    let linked = temp.path().join("linked");
    git(
        &root,
        &["worktree", "add", "-b", "feature", linked.to_str().unwrap()],
    );
    let bare = temp.path().join("archive.git");
    git(
        temp.path(),
        &[
            "clone",
            "--bare",
            root.to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );
    let store = RepositoryService::new();
    let (main_id, _) = opened(store.open_chosen(&root).await);
    let (linked_id, linked_snapshot) = opened(store.open_chosen(&linked).await);
    let (bare_id, bare_snapshot) = opened(store.open_chosen(&bare).await);

    assert_ne!(main_id, linked_id);
    assert_ne!(linked_id, bare_id);
    assert_eq!(
        linked_snapshot.entries[1].head,
        HeadLabel::Branch {
            name: "feature".into()
        }
    );
    assert_eq!(bare_snapshot.entries[2].kind, EntryKind::Bare);
    assert_eq!(bare_snapshot.active_context_id, None);
    match store.select(&linked_id).await {
        SelectOutcome::Selected { snapshot } => {
            assert_eq!(snapshot.active_context_id, Some(linked_id))
        }
        other => panic!("linked worktree could not be selected: {other:?}"),
    }
    match store.select(&bare_id).await {
        SelectOutcome::Selected { snapshot } => {
            assert_eq!(snapshot.active_context_id, Some(bare_id))
        }
        other => panic!("bare repository could not be selected: {other:?}"),
    }
}

#[tokio::test]
async fn detached_head_is_reported_without_changing_repository() {
    let (_temp, root) = working_tree();
    commit(&root);
    git(&root, &["checkout", "--detach"]);
    let facts = GitProbe::default().probe(&root).await.unwrap();
    assert!(
        matches!(&facts.head, Head::Detached(oid) if oid.len() == 8 && oid.bytes().all(|byte| byte.is_ascii_hexdigit()))
    );
}

#[tokio::test]
async fn cancellation_and_rejection_preserve_entries_and_active_selection() {
    let (temp, root) = working_tree();
    let store = RepositoryService::new();
    let (id, _) = opened(store.open_chosen(&root).await);
    let SelectOutcome::Selected { snapshot: before } = store.select(&id).await else {
        panic!("selection failed");
    };
    let OpenOutcome::Cancelled {
        snapshot: cancelled,
    } = store.cancelled().await
    else {
        panic!("cancellation not reported");
    };
    let invalid = temp.path().join("not-a-repository");
    fs::create_dir(&invalid).unwrap();
    let OpenOutcome::Rejected {
        code,
        snapshot: rejected,
        ..
    } = store.open_chosen(&invalid).await
    else {
        panic!("invalid directory admitted");
    };
    assert_eq!(code, GitError::NotRepository);
    assert_eq!(cancelled, before);
    assert_eq!(rejected, before);
}

#[tokio::test]
async fn refresh_marks_only_target_unavailable_and_recovers_when_repository_returns() {
    let (temp, root) = working_tree();
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    git(&other, &["init", "-b", "main"]);
    let store = RepositoryService::new();
    let (first_id, _) = opened(store.open_chosen(&root).await);
    let (second_id, _) = opened(store.open_chosen(&other).await);
    let SelectOutcome::Selected {
        snapshot: selected_other,
    } = store.select(&second_id).await
    else {
        panic!("available context could not be selected");
    };
    assert_eq!(selected_other.active_context_id, Some(second_id.clone()));
    assert_eq!(selected_other.entries[1].availability, Availability::Checking);
    store.refresh(&second_id).await;
    let relocated = temp.path().join("relocated");
    fs::rename(&root, &relocated).unwrap();
    let unavailable = store.refresh(&first_id).await;
    assert_eq!(
        unavailable.entries[0].availability,
        Availability::Unavailable
    );
    assert_eq!(unavailable.entries[1].availability, Availability::Available);
    let SelectOutcome::Selected {
        snapshot: selected_unavailable,
    } = store.select(&first_id).await
    else {
        panic!("existing unavailable entry was incorrectly treated as missing");
    };
    assert_eq!(
        selected_unavailable.active_context_id,
        Some(first_id.clone())
    );
    assert_eq!(
        selected_unavailable.entries[0].availability,
        Availability::Checking
    );
    let checked_unavailable = store.refresh(&first_id).await;
    assert_eq!(checked_unavailable.entries[0].availability, Availability::Unavailable);
    assert_eq!(checked_unavailable.active_context_id.as_deref(), Some(first_id.as_str()));
    assert!(matches!(
        store.select(&second_id).await,
        SelectOutcome::Selected { .. }
    ));
    fs::rename(&relocated, &root).unwrap();
    let restored = store.refresh(&first_id).await;
    assert_eq!(restored.entries[0].availability, Availability::Available);
    assert_eq!(restored.active_context_id, Some(second_id));
}

#[tokio::test]
async fn missing_or_non_directory_picker_result_is_rejected_without_losing_workspace() {
    let (temp, root) = working_tree();
    let store = RepositoryService::new();
    let (_, before) = opened(store.open_chosen(&root).await);
    let missing = temp.path().join("no-longer-here");
    let OpenOutcome::Rejected {
        code,
        snapshot,
    } = store.open_chosen(&missing).await
    else {
        panic!("missing picker path was admitted");
    };
    assert_eq!(code, GitError::NotRepository);
    assert_eq!(snapshot, before);

    let file = temp.path().join("a-file");
    fs::write(&file, b"not a folder").unwrap();
    let OpenOutcome::Rejected { code, snapshot, .. } = store.open_chosen(&file).await else {
        panic!("non-directory picker path was admitted");
    };
    assert_eq!(code, GitError::NotRepository);
    assert_eq!(snapshot, before);
}

#[tokio::test]
async fn selecting_unknown_id_does_not_change_active_context() {
    let (_temp, root) = working_tree();
    let store = RepositoryService::new();
    let (id, _) = opened(store.open_chosen(&root).await);
    let SelectOutcome::Selected { snapshot: before } = store.select(&id).await else {
        panic!("known entry could not be selected");
    };
    let SelectOutcome::NotFound { snapshot } = store.select("repository-not-in-store").await else {
        panic!("unknown entry was treated as present");
    };
    assert_eq!(snapshot, before);
}

#[tokio::test]
async fn concurrent_admissions_reuse_one_canonical_entry() {
    let (_temp, root) = working_tree();
    let store = RepositoryService::new();
    let (left, right) = tokio::join!(store.open_chosen(&root), store.open_chosen(&root));
    let (opened_id, reused_id) = match (left, right) {
        (OpenOutcome::Opened { entry_id: opened_id, .. }, OpenOutcome::Reused { entry_id: reused_id, .. })
        | (OpenOutcome::Reused { entry_id: reused_id, .. }, OpenOutcome::Opened { entry_id: opened_id, .. }) => {
            (opened_id, reused_id)
        }
        other => panic!("concurrent admissions were not deduplicated: {other:?}"),
    };
    assert_eq!(opened_id, reused_id);
    let snapshot = store.snapshot().await;
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].id, opened_id);
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.active_context_id, None);
}

#[cfg(unix)]
#[tokio::test]
async fn non_utf8_native_path_is_rejected_instead_of_lossily_serialized() {
    use std::os::unix::ffi::OsStringExt;
    let temp = tempfile::tempdir().unwrap();
    let invalid = temp
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'r', 0xff]));
    assert_eq!(
        GitProbe::default().probe(&invalid).await.err(),
        Some(GitError::UnsupportedPathEncoding)
    );
}

#[tokio::test]
async fn temporary_git_fixtures_isolate_mutations_and_clean_up_independently() {
    let (first_temp, first_root) = support::working_tree();
    let (second_temp, second_root) = support::working_tree();
    let first_directory = first_temp.path().to_owned();
    let second_directory = second_temp.path().to_owned();
    git(&first_root, &["branch", "-m", "first-only"]);
    git(&first_root, &["config", "fixture.private", "first-only"]);
    fs::write(first_root.join("first.txt"), b"first private bytes").unwrap();
    fs::write(second_root.join("second.txt"), b"second private bytes").unwrap();

    let first_probe = GitProbe::default();
    let second_probe = GitProbe::default();
    let (first, second) = tokio::join!(
        first_probe.probe(&first_root),
        second_probe.probe(&second_root),
    );
    assert_eq!(first.unwrap().head, Head::Branch("first-only".into()));
    assert_eq!(second.unwrap().head, Head::Branch("main".into()));
    assert!(!support::git_output(&second_root, &["config", "--get", "fixture.private"]).status.success());
    assert!(!second_root.join("first.txt").exists());
    assert!(!first_root.join("second.txt").exists());

    drop(first_temp);
    assert!(!first_directory.exists());
    assert_eq!(fs::read(second_root.join("second.txt")).unwrap(), b"second private bytes");
    drop(second_temp);
    assert!(!second_directory.exists());
}

#[test]
fn temporary_git_fixture_is_removed_when_a_scenario_panics() {
    let (temp, root) = support::working_tree();
    let directory = temp.path().to_owned();
    let failure = std::panic::catch_unwind(move || {
        let _owned_directory = temp;
        git(&root, &["branch", "-m", "failure-only"]);
        fs::write(root.join("unfinished.txt"), b"partial scenario bytes").unwrap();
        panic!("deliberate fixture cleanup scenario");
    });
    assert!(failure.is_err());
    assert!(!directory.exists());
}

#[tokio::test]
async fn admission_failures_serialize_only_closed_codes_and_preserve_state() {
    let service = RepositoryService::new();
    let before = service.snapshot().await;
    for (error, code) in [
        (GitError::GitUnavailable, "git_unavailable"),
        (GitError::NotRepository, "not_repository"),
        (GitError::Inaccessible, "inaccessible"),
        (GitError::UnsafeRepository, "unsafe_repository"),
        (GitError::ProbeTimeout, "probe_timeout"),
        (GitError::RepositoryChanged, "repository_changed"),
        (GitError::UnsupportedPathEncoding, "unsupported_path_encoding"),
        (GitError::Unavailable, "repository_unavailable"),
    ] {
        let outcome = service.rejected(error).await;
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["code"], code);
        assert!(json.get("message").is_none());
        let OpenOutcome::Rejected { snapshot, .. } = outcome else { panic!("expected rejection") };
        assert_eq!(snapshot, before);
        let mutation_code = gitview_lib::workspace::WorkspaceRejectionCode::from(error);
        assert_eq!(serde_json::to_value(mutation_code).unwrap(), code);
    }
}
