//! Protects repository files and Git metadata from writes by admission, selection and refresh.

use std::fs;

use gitview_lib::application::RepositoryService;
use gitview_lib::workspace::{OpenOutcome, SelectOutcome};

#[path = "../support/mod.rs"]
mod support;
use support::git;


#[tokio::test]
async fn opening_selecting_and_refreshing_never_change_files_index_head_or_refs() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-b", "main"]);
    let document = root.join("notes.txt");
    fs::write(&document, "initial\n").unwrap();
    git(root, &["add", "notes.txt"]);
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.org",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initial",
        ],
    );
    // Distinct committed, staged and working contents expose writes to either the index or worktree.
    fs::write(&document, "staged\n").unwrap();
    git(root, &["add", "notes.txt"]);
    fs::write(&document, "working copy\n").unwrap();
    let index = root.join(".git/index");
    let head = root.join(".git/HEAD");
    let reference = root.join(".git/refs/heads/main");
    let before = (
        fs::read(&document).unwrap(),
        fs::read(&index).unwrap(),
        fs::read(&head).unwrap(),
        fs::read(&reference).unwrap(),
    );

    let service = RepositoryService::new();
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(root).await else {
        panic!("repository not opened")
    };
    assert!(matches!(
        service.select(&entry_id).await,
        SelectOutcome::Selected { .. }
    ));
    let snapshot = service.refresh(&entry_id).await;
    assert_eq!(
        snapshot.active_context_id.as_deref(),
        Some(entry_id.as_str())
    );

    assert_eq!(fs::read(&document).unwrap(), before.0);
    assert_eq!(fs::read(&index).unwrap(), before.1);
    assert_eq!(fs::read(&head).unwrap(), before.2);
    assert_eq!(fs::read(&reference).unwrap(), before.3);
}
