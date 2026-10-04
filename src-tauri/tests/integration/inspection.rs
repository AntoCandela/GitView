//! Exercises real local Git context discovery, pinned branch views and parent-specific file lists.

use super::*;
use crate::{application::RepositoryService, history::{HistoryPage, HistoryPageResult}, test_support::{self, ManualClock}, workspace::{MutationOutcome, OpenOutcome, WorkspaceRejectionCode}};
use std::{fs, path::Path, sync::Arc, time::Duration};

fn head(root: &Path) -> String { String::from_utf8(test_support::git_output(root, &["rev-parse", "HEAD"]).stdout).unwrap().trim_end().to_owned() }
fn object_commit(root: &Path, parents: &[&str]) -> String {
    let mut args = vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "commit-tree", "HEAD^{tree}"];
    for parent in parents { args.extend_from_slice(&["-p", parent]); }
    args.extend_from_slice(&["-m", "fixture"]);
    let output = test_support::git_output(root, &args);
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}
fn nonunicode_commit(root: &Path, parent: &str) -> (String, String) {
    use std::{io::Write, process::{Command, Stdio}};
    fs::write(root.join("content"), "content").unwrap();
    let blob = String::from_utf8(test_support::git_output(root, &["hash-object", "-w", "content"]).stdout).unwrap();
    // Git objects can contain native byte names even when macOS rejects those filesystem names.
    let mut child = Command::new("git").current_dir(root).args(["mktree", "-z"])
        .env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_COMMON_DIR").env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_CONFIG_GLOBAL", root.join(".gitview-empty-global-config"))
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input = format!("100644 blob {}\tunsupported-", blob.trim_end()).into_bytes();
    input.extend_from_slice(b"\xff\0");
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let tree = String::from_utf8(output.stdout).unwrap().trim_end().to_owned();
    let output = test_support::git_output(root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org",
        "commit-tree", &tree, "-p", parent, "-m", "native filename"]);
    assert!(output.status.success());
    (tree, String::from_utf8(output.stdout).unwrap().trim_end().to_owned())
}
async fn select(service: &RepositoryService, root: &Path, clock: &ManualClock) -> String {
    let OpenOutcome::Opened { entry_id, .. } = clock.finish(service.open_chosen(root)).await else { panic!("fixture must open") };
    clock.finish(service.select(&entry_id)).await;
    entry_id
}
fn page(result: HistoryPageResult) -> HistoryPage { match result { HistoryPageResult::Page { page } => page, other => panic!("expected history: {other:?}") } }
fn files(result: CommitFilesResult) -> (Option<String>, Vec<String>, Vec<(String, CommittedFileKind)>) {
    match result {
        CommitFilesResult::Files { parent_oid, parents, files, .. } => {
            for file in &files { assert_eq!(file.segments.join("/"), file.display_path); }
            (parent_oid, parents, files.into_iter().map(|file| (file.display_path, file.kind)).collect())
        }
        other => panic!("expected files: {other:?}"),
    }
}

#[tokio::test]
async fn branch_view_traverses_only_its_pinned_ancestry_without_mutating_git() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let main = head(&root);
    let mut topic = object_commit(&root, &[]);
    let topic_root = topic.clone();
    for _ in 0..104 { topic = object_commit(&root, &[&topic]); }
    test_support::git(&root, &["update-ref", "refs/heads/topic", &topic]);
    let head_bytes = fs::read(root.join(".git/HEAD")).unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let main_ref = fs::read(root.join(".git/refs/heads/main")).unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let first = page(clock.finish(service.history_page(&entry, None, Some("topic"))).await);
    assert_eq!(first.commits[0].oid, topic);
    assert!(first.commits.iter().all(|commit| commit.oid != main));
    let cursor = first.cursor.as_deref().unwrap();
    assert!(matches!(clock.finish(service.history_page(&entry, Some(cursor), Some("main"))).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    let new_tip = object_commit(&root, &[&topic]);
    test_support::git(&root, &["update-ref", "refs/heads/topic", &new_tip]);
    let continuation = page(clock.finish(service.history_page(&entry, Some(cursor), Some("topic"))).await);
    assert_eq!(continuation.refs, first.refs);
    assert_eq!(continuation.commits.last().unwrap().oid, topic_root);
    assert_eq!(continuation.commits.len(), 5);
    assert!(!continuation.has_more);
    assert_eq!(page(clock.finish(service.history_page(&entry, None, Some("topic"))).await).commits[0].oid, new_tip);
    assert!(matches!(clock.finish(service.history_page(&entry, None, Some("--all"))).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head_bytes);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), main_ref);
    service.shutdown().await;
}

#[tokio::test]
async fn discovered_unadmitted_worktree_is_selected_and_persisted_using_only_its_issued_id() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked\nworktree");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let workspace = temp.path().join("workspace.json");
    let service = RepositoryService::with_workspace_file(workspace.clone()).await;
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { branches, worktrees } = clock.finish(service.list_contexts(&entry)).await else { panic!("expected options") };
    assert_eq!(branches.iter().map(|branch| branch.name.as_str()).collect::<Vec<_>>(), ["main", "topic"]);
    assert_eq!(worktrees.iter().map(|worktree| (worktree.label.as_str(), worktree.branch.as_deref(), worktree.current)).collect::<Vec<_>>(), [("project", Some("main"), true), ("linked\nworktree", Some("topic"), false)]);
    assert_eq!(service.snapshot().await.entries.len(), 1);
    assert!(matches!(service.select_worktree(&entry, linked.to_str().unwrap()).await, MutationOutcome::NotFound { .. }));
    let id = &worktrees[1].id;
    let MutationOutcome::Updated { snapshot } = clock.finish(service.select_worktree(&entry, id)).await else { panic!("worktree must select") };
    let active = snapshot.active_context_id.unwrap();
    assert_ne!(active, entry);
    assert_eq!(snapshot.entries.iter().find(|entry| entry.id == active).unwrap().head, crate::workspace::HeadLabel::Branch { name: "topic".into() });
    assert_eq!(snapshot.entries.len(), 2);
    let document = crate::workspace::persistence::load(&workspace).await.unwrap();
    assert_eq!(document.active_root, Some(fs::canonicalize(&linked).unwrap()));
    assert!(matches!(service.select_worktree(&entry, id).await, MutationOutcome::NotFound { .. }));
    assert_eq!(String::from_utf8(test_support::git_output(&root, &["branch", "--show-current"]).stdout).unwrap(), "main\n");
    assert_eq!(String::from_utf8(test_support::git_output(&linked, &["branch", "--show-current"]).stdout).unwrap(), "topic\n");
    service.shutdown().await;
}

#[tokio::test]
async fn root_and_regular_commits_report_real_added_modified_deleted_and_type_changes() {
    use std::os::unix::fs::symlink;
    let clock = ManualClock::new();
    let (_temp, root) = test_support::unborn_working_tree();
    fs::create_dir(root.join("nested")).unwrap();
    fs::write(root.join("nested/change\nfile"), "before").unwrap();
    fs::write(root.join("delete"), "deleted later").unwrap();
    fs::write(root.join("type"), "regular").unwrap();
    test_support::commit(&root);
    let initial = head(&root);
    fs::write(root.join("nested/change\nfile"), "after").unwrap();
    fs::remove_file(root.join("delete")).unwrap();
    fs::remove_file(root.join("type")).unwrap();
    symlink("nested/change\nfile", root.join("type")).unwrap();
    fs::write(root.join("added"), "new").unwrap();
    test_support::commit(&root);
    let next = head(&root);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let root_files = files(clock.finish(service.commit_files(&entry, &initial, None)).await);
    assert_eq!(root_files, (None, vec![], vec![("delete".into(), CommittedFileKind::Added), ("nested/change\nfile".into(), CommittedFileKind::Added), ("type".into(), CommittedFileKind::Added)]));
    let regular = files(clock.finish(service.commit_files(&entry, &next, None)).await);
    assert_eq!(regular, (Some(initial.clone()), vec![initial], vec![("added".into(), CommittedFileKind::Added), ("delete".into(), CommittedFileKind::Deleted), ("nested/change\nfile".into(), CommittedFileKind::Modified), ("type".into(), CommittedFileKind::TypeChange)]));
    service.shutdown().await;
}

#[tokio::test]
async fn merge_file_list_uses_first_raw_parent_by_default_and_explicit_second_parent() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let base = head(&root);
    fs::write(root.join("left"), "left").unwrap();
    test_support::commit(&root);
    let left = head(&root);
    test_support::git(&root, &["checkout", "--detach", &base]);
    fs::write(root.join("right"), "right").unwrap();
    test_support::commit(&root);
    let right = head(&root);
    fs::write(root.join("left"), "left").unwrap();
    test_support::commit(&root);
    let merge = object_commit(&root, &[&left, &right]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let first = files(clock.finish(service.commit_files(&entry, &merge, None)).await);
    assert_eq!(first, (Some(left.clone()), vec![left.clone(), right.clone()], vec![("right".into(), CommittedFileKind::Added)]));
    let second = files(clock.finish(service.commit_files(&entry, &merge, Some(&right))).await);
    assert_eq!(second, (Some(right.clone()), vec![left, right], vec![("left".into(), CommittedFileKind::Added)]));
    assert!(matches!(clock.finish(service.commit_files(&entry, &merge, Some(&base))).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn commit_file_authority_rejects_options_abbreviations_noncommits_and_nonunicode_paths() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let initial = head(&root);
    let (tree, next) = nonunicode_commit(&root, &initial);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(service.commit_files(&entry, "--all", None).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    assert!(matches!(service.commit_files(&entry, &initial[..12], None).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    assert!(matches!(clock.finish(service.commit_files(&entry, &tree, None)).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    assert!(matches!(clock.finish(service.commit_files(&entry, &next, None)).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, message: ENCODING_MESSAGE }));
    service.shutdown().await;
}

#[tokio::test]
async fn shallow_commit_with_missing_raw_parent_is_unavailable_not_an_empty_tree_diff() {
    let clock = ManualClock::new();
    let (_source_temp, source) = test_support::working_tree();
    fs::write(source.join("new"), "content").unwrap();
    test_support::commit(&source);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shallow");
    let url = format!("file://{}", source.to_str().unwrap());
    test_support::git(temp.path(), &["clone", "--depth=1", &url, root.to_str().unwrap()]);
    let oid = head(&root);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.commit_files(&entry, &oid, None)).await, CommitFilesResult::Unavailable { code: HistoryErrorCode::MissingObjects, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn commit_files_and_unadmitted_worktree_io_are_discarded_on_same_entry_reselection() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let gate = test_support::executable(temp.path(), &format!(r#"
case "$*" in
  *" diff-tree "*|*" rev-parse --absolute-git-dir"*)
    if [ -f {block} ]; then
      git "$@" || exit $?
      : > {entered}
      while [ ! -f {release} ]; do sleep 0.01; done
      exit 0
    fi ;;
esac
exec git "$@"
"#, block = test_support::quote(&temp.path().join("block")), entered = test_support::quote(&temp.path().join("entered")), release = test_support::quote(&temp.path().join("release"))));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&gate));
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    let target = worktrees.iter().find(|worktree| !worktree.current).unwrap().id.clone();
    fs::write(temp.path().join("block"), "").unwrap();
    let child = Arc::clone(&service); let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.select_worktree(&child_entry, &target).await });
    clock.wait_for_file(&temp.path().join("entered")).await;
    clock.finish(service.select(&entry)).await;
    fs::write(temp.path().join("release"), "").unwrap();
    assert!(matches!(clock.finish(request).await.unwrap(), MutationOutcome::Rejected { code: WorkspaceRejectionCode::RepositoryChanged, .. }));
    assert_eq!(service.snapshot().await.entries.len(), 1);
    fs::remove_file(temp.path().join("entered")).unwrap();
    fs::remove_file(temp.path().join("release")).unwrap();
    let oid = head(&root); let child = Arc::clone(&service); let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.commit_files(&child_entry, &oid, None).await });
    clock.wait_for_file(&temp.path().join("entered")).await;
    clock.finish(service.select(&entry)).await;
    fs::write(temp.path().join("release"), "").unwrap();
    assert!(matches!(clock.finish(request).await.unwrap(), CommitFilesResult::Unavailable { code: HistoryErrorCode::StaleSelection, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn replaced_worktree_location_cannot_be_admitted_with_an_old_issued_id() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    let target = worktrees.iter().find(|worktree| !worktree.current).unwrap().id.clone();
    fs::rename(&linked, temp.path().join("old-linked")).unwrap();
    fs::create_dir(&linked).unwrap();
    test_support::git(&linked, &["init", "-b", "unrelated"]);
    assert!(matches!(clock.finish(service.select_worktree(&entry, &target)).await, MutationOutcome::Rejected { code: WorkspaceRejectionCode::RepositoryUnavailable, .. }));
    assert_eq!(service.snapshot().await.active_context_id.as_deref(), Some(entry.as_str()));
    assert_eq!(service.snapshot().await.entries.len(), 1);
    service.shutdown().await;
}

#[tokio::test]
async fn stale_worktree_registration_cannot_issue_authority_for_an_unrelated_repository() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    fs::rename(&linked, temp.path().join("old-linked")).unwrap();
    fs::create_dir(&linked).unwrap();
    test_support::git(&linked, &["init", "-b", "unrelated"]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    assert_eq!(worktrees.iter().map(|worktree| (worktree.label.as_str(), worktree.current)).collect::<Vec<_>>(), [("project", true)]);
    assert_eq!(service.snapshot().await.entries.len(), 1);
    service.shutdown().await;
}

#[tokio::test]
async fn stale_worktree_registration_cannot_issue_authority_for_an_ancestor_repository() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let unrelated = temp.path().join("unrelated");
    let linked = unrelated.join("linked");
    fs::create_dir(&unrelated).unwrap();
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    fs::rename(&linked, temp.path().join("old-linked")).unwrap();
    fs::create_dir(&linked).unwrap();
    test_support::git(&unrelated, &["init", "-b", "unrelated"]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    assert_eq!(worktrees.iter().map(|worktree| (worktree.label.as_str(), worktree.current)).collect::<Vec<_>>(), [("project", true)]);
    assert_eq!(service.snapshot().await.entries.len(), 1);
    service.shutdown().await;
}

#[tokio::test]
async fn bare_and_linked_contexts_discover_only_their_shared_repository_worktrees() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let bare = temp.path().join("bare.git");
    test_support::git(temp.path(), &["clone", "--bare", root.to_str().unwrap(), bare.to_str().unwrap()]);
    let linked = temp.path().join("linked");
    test_support::git(&bare, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let head_before = fs::read(bare.join("HEAD")).unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &bare, &clock).await;
    let ContextOptionsResult::Options { branches, worktrees } = clock.finish(service.list_contexts(&entry)).await else { panic!("bare options") };
    assert_eq!(branches.iter().map(|branch| branch.name.as_str()).collect::<Vec<_>>(), ["main", "topic"]);
    assert_eq!(worktrees.iter().map(|worktree| (worktree.label.as_str(), worktree.branch.as_deref(), worktree.current)).collect::<Vec<_>>(),
        [("linked", Some("topic"), false)]);
    let linked_id = worktrees.iter().find(|worktree| !worktree.current).unwrap().id.clone();
    let MutationOutcome::Updated { snapshot } = clock.finish(service.select_worktree(&entry, &linked_id)).await else { panic!("linked selection") };
    let linked_entry = snapshot.active_context_id.unwrap();
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&linked_entry)).await else { panic!("linked options") };
    assert_eq!(worktrees.iter().map(|worktree| (worktree.label.as_str(), worktree.current)).collect::<Vec<_>>(), [("linked", true)]);
    assert_eq!(fs::read(bare.join("HEAD")).unwrap(), head_before);
    service.shutdown().await;
}

#[tokio::test]
async fn sha256_committed_files_preserve_full_parent_identity() {
    let clock = ManualClock::new();
    let temp = tempfile::tempdir().unwrap(); let root = temp.path().join("sha256");
    fs::create_dir(&root).unwrap();
    test_support::git(&root, &["init", "-b", "main", "--object-format=sha256"]);
    fs::write(root.join("first"), "initial").unwrap(); test_support::commit(&root);
    let initial = head(&root);
    fs::write(root.join("second"), "next").unwrap(); test_support::commit(&root);
    let next = head(&root);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    assert_eq!(files(clock.finish(service.commit_files(&entry, &initial, None)).await), (None, vec![], vec![("first".into(), CommittedFileKind::Added)]));
    assert_eq!(files(clock.finish(service.commit_files(&entry, &next, None)).await), (Some(initial.clone()), vec![initial], vec![("second".into(), CommittedFileKind::Added)]));
    service.shutdown().await;
}

#[tokio::test]
async fn commit_files_rejects_malformed_or_oversized_git_output_and_honors_shared_deadline() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let oid = head(&root);
    let malformed = test_support::executable(temp.path(), r#"
case "$*" in *" diff-tree "*) printf 'M\000unterminated'; exit 0 ;; esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&malformed);
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.commit_files(&entry, &oid, None)).await, CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
    let oversized = test_support::executable(temp.path(), r#"
case "$*" in *" diff-tree "*) dd if=/dev/zero bs=1048577 count=1 2>/dev/null; exit 0 ;; esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&oversized);
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.commit_files(&entry, &oid, None)).await, CommitFilesResult::Error { code: HistoryErrorCode::ResourceLimit, .. }));
    service.shutdown().await;
    let entered = temp.path().join("deadline-entered");
    let slow = test_support::executable(temp.path(), &format!(r#"
case "$*" in *" diff-tree "*) : > {}; exec sleep 30 ;; esac
exec git "$@"
"#, test_support::quote(&entered)));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&slow));
    let entry = select(&service, &root, &clock).await;
    let child = Arc::clone(&service);
    let request = tokio::spawn(async move { child.commit_files(&entry, &oid, None).await });
    clock.wait_for_file(&entered).await;
    clock.advance(Duration::from_secs(31)).await;
    assert!(matches!(clock.finish(request).await.unwrap(), CommitFilesResult::Unavailable { code: HistoryErrorCode::Timeout, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn context_discovery_rejects_truncated_and_over_limit_worktree_records() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let truncated = test_support::executable(temp.path(), r#"
case "$*" in *" worktree list --porcelain -z"*) printf 'worktree /invalid\000HEAD abc'; exit 0 ;; esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&truncated);
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.list_contexts(&entry)).await, ContextOptionsResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
    let oversized = test_support::executable(temp.path(), r#"
case "$*" in *" worktree list --porcelain -z"*)
  n=0
  while [ "$n" -lt 65 ]; do printf 'worktree /fixture-%s\000bare\000\000' "$n"; n=$((n + 1)); done
  exit 0 ;;
esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&oversized);
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.list_contexts(&entry)).await, ContextOptionsResult::Error { code: HistoryErrorCode::ResourceLimit, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn worktree_membership_process_failure_is_not_silently_omitted() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let unavailable = test_support::executable(temp.path(), &format!(r#"
case "$*" in *" --git-common-dir"*)
  if [ "$PWD" = {} ]; then exit 42; fi ;;
esac
exec git "$@"
"#, test_support::quote(&fs::canonicalize(&linked).unwrap())));
    let service = RepositoryService::new().with_inspection_executable(&unavailable);
    let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.list_contexts(&entry)).await,
        ContextOptionsResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn removing_an_admitted_target_during_worktree_verification_reports_superseded_selection() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let gate = test_support::executable(temp.path(), &format!(r#"
case "$*" in
  *" rev-parse --absolute-git-dir"*)
    if [ -f {block} ]; then
      git "$@" || exit $?
      : > {entered}
      while [ ! -f {release} ]; do sleep 0.01; done
      exit 0
    fi ;;
esac
exec git "$@"
"#, block = test_support::quote(&temp.path().join("block")), entered = test_support::quote(&temp.path().join("entered")), release = test_support::quote(&temp.path().join("release"))));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&gate));
    let OpenOutcome::Opened { entry_id: target_entry, .. } = clock.finish(service.open_chosen(&linked)).await else { panic!("target must open") };
    let entry = select(&service, &root, &clock).await;
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    let target = worktrees.iter().find(|worktree| !worktree.current).unwrap().id.clone();
    fs::write(temp.path().join("block"), "").unwrap();
    let child = Arc::clone(&service); let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.select_worktree(&child_entry, &target).await });
    clock.wait_for_file(&temp.path().join("entered")).await;
    assert!(matches!(clock.finish(service.remove(&target_entry)).await, MutationOutcome::Updated { .. }));
    fs::write(temp.path().join("release"), "").unwrap();
    let outcome = clock.finish(request).await.unwrap();
    assert!(matches!(outcome, MutationOutcome::Rejected { code: WorkspaceRejectionCode::SupersededSelection, .. }));
    let json = serde_json::to_value(outcome).unwrap();
    assert_eq!(json["code"], "superseded_selection");
    assert!(json.get("message").is_none());
    assert_eq!(service.snapshot().await.active_context_id.as_deref(), Some(entry.as_str()));
    assert_eq!(service.snapshot().await.entries.len(), 1);
    service.shutdown().await;
}

#[tokio::test]
async fn worktree_admission_preserves_the_coded_rejection_of_a_replaced_stored_identity() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("linked");
    test_support::git(&root, &["worktree", "add", "-b", "topic", linked.to_str().unwrap()]);
    let service = RepositoryService::new();
    assert!(matches!(clock.finish(service.open_chosen(&linked)).await, OpenOutcome::Opened { .. }));
    let entry = select(&service, &root, &clock).await;
    fs::rename(&linked, temp.path().join("old-linked")).unwrap();
    test_support::git(&root, &["worktree", "prune"]);
    test_support::git(&root, &["worktree", "add", "-b", "replacement", linked.to_str().unwrap()]);
    let ContextOptionsResult::Options { worktrees, .. } = clock.finish(service.list_contexts(&entry)).await else { panic!("options") };
    let target = worktrees.iter().find(|worktree| !worktree.current).unwrap().id.clone();
    let outcome = clock.finish(service.select_worktree(&entry, &target)).await;
    assert!(matches!(outcome, MutationOutcome::Rejected { code: WorkspaceRejectionCode::RepositoryChanged, .. }));
    assert_eq!(service.snapshot().await.active_context_id.as_deref(), Some(entry.as_str()));
    assert_eq!(service.snapshot().await.entries.len(), 2);
    service.shutdown().await;
}
