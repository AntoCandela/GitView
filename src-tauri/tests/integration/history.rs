//! Exercises real Git graph topology, pinned paging, repository kinds and local-only failures.

use super::*;
use crate::{application::RepositoryService, test_support::{self, ManualClock}, workspace::OpenOutcome};
use std::{fs, path::{Path, PathBuf}};

fn head(root: &Path) -> String { String::from_utf8(test_support::git_output(root, &["rev-parse", "HEAD"]).stdout).unwrap().trim_end().to_owned() }
fn create_commit(root: &Path, parents: &[&str], subject: &str) -> String {
    let mut arguments = vec!["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "commit-tree", "HEAD^{tree}"];
    for parent in parents { arguments.extend_from_slice(&["-p", parent]); }
    arguments.extend_from_slice(&["-m", subject]);
    let output = test_support::git_output(root, &arguments);
    assert!(output.status.success(), "isolated commit object fixture failed");
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}
fn linear_history(root: &Path, count: usize) -> Vec<String> {
    let mut commits = vec![head(root)];
    for index in 0..count {
        let next = create_commit(root, &[commits.last().unwrap()], &format!("row {index}"));
        commits.push(next);
    }
    test_support::git(root, &["update-ref", "refs/heads/main", commits.last().unwrap()]);
    commits
}
async fn selected(service: &RepositoryService, root: &Path, clock: &ManualClock) -> String {
    let OpenOutcome::Opened { entry_id, .. } = clock.finish(service.open_chosen(root)).await else { panic!("fixture must be admitted") };
    clock.finish(service.select(&entry_id)).await;
    entry_id
}
fn page(result: HistoryPageResult) -> HistoryPage {
    match result { HistoryPageResult::Page { page } => page, other => panic!("expected graph page: {other:?}") }
}

#[tokio::test]
async fn raw_merge_ancestry_and_peeled_local_refs_are_real_and_read_only() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let initial = head(&root);
    let left = create_commit(&root, &[&initial], "left");
    let right = create_commit(&root, &[&initial], "right");
    let merge = create_commit(&root, &[&left, &right], "merge");
    test_support::git(&root, &["update-ref", "refs/heads/main", &merge]);
    test_support::git(&root, &["update-ref", "refs/heads/topic", &right]);
    test_support::git(&root, &["update-ref", "refs/remotes/origin/topic", &right]);
    test_support::git(&root, &["tag", "light", &right]);
    test_support::git(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "tag.gpgSign=false", "tag", "-a", "annotated", "-m", "tag", &left]);
    let head_bytes = fs::read(root.join(".git/HEAD")).unwrap();
    let index_bytes = fs::read(root.join(".git/index")).unwrap();
    let ref_bytes = fs::read(root.join(".git/refs/heads/main")).unwrap();
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(graph.commits[0].oid, merge);
    assert_eq!(graph.commits[0].parents, vec![HistoryParent { oid: left.clone(), state: ParentState::Loaded }, HistoryParent { oid: right.clone(), state: ParentState::Loaded }]);
    assert!(graph.commits.iter().find(|row| row.oid == initial).unwrap().root);
    assert_eq!((graph.head.scope, graph.head.state, graph.head.branch.as_deref(), graph.head.oid.as_deref()), (HeadScope::Worktree, HeadState::Attached, Some("main"), Some(merge.as_str())));
    assert!(graph.refs.contains(&HistoryRef { kind: RefKind::Tag, name: "annotated".into(), commit_oid: left }));
    assert!(graph.refs.contains(&HistoryRef { kind: RefKind::Tag, name: "light".into(), commit_oid: right.clone() }));
    assert!(graph.refs.contains(&HistoryRef { kind: RefKind::RemoteTracking, name: "origin/topic".into(), commit_oid: right }));
    assert_eq!((graph.has_more, graph.completeness), (false, Completeness::Complete));
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head_bytes);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index_bytes);
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), ref_bytes);
    service.shutdown().await;
}

#[tokio::test]
async fn paging_is_pinned_under_ref_movement_and_irrelevant_file_observation_revisions() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let expected = linear_history(&root, 110);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let first = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(first.commits.len(), 100);
    assert_eq!(first.completeness, Completeness::Paged);
    assert_eq!(first.commits[99].parents[0].state, ParentState::OutsidePage);
    let cursor = first.cursor.as_deref().unwrap();
    let new_tip = create_commit(&root, &[expected.last().unwrap()], "new external tip");
    test_support::git(&root, &["update-ref", "refs/heads/main", &new_tip]);
    fs::write(root.join("unrelated-untracked"), b"observation changes only").unwrap();
    clock.advance(Duration::from_secs(2)).await;
    let second = page(clock.finish(service.history_page(&entry, Some(cursor), None)).await);
    assert_eq!(second.refs, first.refs);
    assert_eq!(second.head, first.head);
    assert_eq!(second.commits.len(), 11);
    assert!(!second.has_more);
    assert_eq!(second.cursor, None);
    let actual: Vec<_> = first.commits.iter().chain(&second.commits).map(|row| row.oid.clone()).collect();
    assert_eq!(actual, expected.into_iter().rev().collect::<Vec<_>>());
    assert!(matches!(service.history_page(&entry, Some(cursor), None).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    let refreshed = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(refreshed.commits[0].oid, new_tip);
    assert_ne!(refreshed.head, first.head);
    service.shutdown().await;
}

#[tokio::test]
async fn shallow_boundary_retains_unavailable_real_parent_instead_of_inventing_a_root() {
    let clock = ManualClock::new();
    let (_source_temp, source) = test_support::working_tree();
    let chain = linear_history(&source, 2);
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shallow");
    let url = format!("file://{}", source.to_str().unwrap());
    test_support::git(temp.path(), &["clone", "--depth=1", "--branch=main", &url, root.to_str().unwrap()]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(graph.commits.len(), 1);
    assert_eq!(graph.commits[0].oid, chain[2]);
    assert_eq!(graph.commits[0].parents, vec![HistoryParent { oid: chain[1].clone(), state: ParentState::Unavailable }]);
    assert!(!graph.commits[0].root);
    assert_eq!((graph.has_more, graph.completeness), (false, Completeness::ShallowOrMissing));
    service.shutdown().await;
}

#[tokio::test]
async fn bare_detached_unborn_and_unresolved_head_keep_truthful_scope_and_other_valid_refs() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let initial = head(&root);
    test_support::git(&root, &["checkout", "--detach", &initial]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let detached = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!((detached.head.scope, detached.head.state, detached.head.branch, detached.head.oid), (HeadScope::Worktree, HeadState::Detached, None, Some(initial.clone())));
    test_support::git(&root, &["symbolic-ref", "HEAD", "refs/heads/newborn"]);
    let unborn = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!((unborn.head.state, unborn.head.branch.as_deref(), unborn.head.oid), (HeadState::Unborn, Some("newborn"), None));
    assert_eq!(unborn.commits[0].oid, initial);
    fs::write(root.join(".git/HEAD"), format!("{}\n", "f".repeat(40))).unwrap();
    let unresolved = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!((unresolved.head.state, unresolved.head.oid), (HeadState::Unresolved, None));
    assert_eq!(unresolved.commits[0].oid, initial);
    service.shutdown().await;
    let bare = temp.path().join("bare.git");
    test_support::git(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    test_support::git(temp.path(), &["clone", "--bare", root.to_str().unwrap(), bare.to_str().unwrap()]);
    test_support::git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let service = RepositoryService::new();
    let entry = selected(&service, &bare, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!((graph.head.scope, graph.head.state), (HeadScope::Repository, HeadState::Attached));
    assert_eq!(graph.commits[0].oid, initial);
    service.shutdown().await;
}

#[tokio::test]
async fn a_new_unborn_repository_has_no_fake_commits_or_ancestry() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::unborn_working_tree();
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!((graph.head.state, graph.head.branch.as_deref(), graph.head.oid), (HeadState::Unborn, Some("main"), None));
    assert_eq!(graph.upstream.state, UpstreamState::Unborn);
    assert_eq!(graph.commits, []);
    assert_eq!(graph.refs, []);
    assert_eq!((graph.has_more, graph.cursor, graph.completeness), (false, None, Completeness::Complete));
    service.shutdown().await;
}

#[tokio::test]
async fn vanished_pinned_tip_and_wrong_entry_or_superseded_cursors_fail_explicitly() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let chain = linear_history(&root, 101);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let first = page(clock.finish(service.history_page(&entry, None, None)).await);
    let cursor = first.cursor.unwrap();
    assert!(matches!(service.history_page(&entry, Some("--all"), None).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    let tip = chain.last().unwrap();
    fs::remove_file(root.join(".git/objects").join(&tip[..2]).join(&tip[2..])).unwrap();
    assert!(matches!(clock.finish(service.history_page(&entry, Some(&cursor), None)).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::MissingObjects, .. }));
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    test_support::git(&other, &["init", "-b", "other"]);
    let other_entry = selected(&service, &other, &clock).await;
    assert!(matches!(service.history_page(&other_entry, Some(&cursor), None).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    assert!(matches!(service.history_page(&entry, None, None).await, HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleSelection, .. }));
    service.shutdown().await;
}

fn gated_history(at: &Path) -> PathBuf {
    test_support::executable(at, &format!(r#"
case "$*" in
  *" rev-list "*)
    git "$@" || exit $?
    printf '%s\n' "$$" > {pid}
    : > {entered}
    while [ ! -f {release} ]; do sleep 0.01; done
    exit 0 ;;
esac
exec git "$@"
"#, pid = test_support::quote(&at.join(".gitview-child-pid")), entered = test_support::quote(&at.join(".gitview-entered")), release = test_support::quote(&at.join(".gitview-release"))))
}

#[tokio::test]
async fn same_entry_reselection_during_history_io_discards_old_graph_and_cursor() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&gated_history(temp.path())));
    let entry = selected(&service, &root, &clock).await;
    let child = Arc::clone(&service);
    let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.history_page(&child_entry, None, None).await });
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    clock.finish(service.select(&entry)).await;
    test_support::release(temp.path());
    assert!(matches!(clock.finish(request).await.unwrap(), HistoryPageResult::Unavailable { code: HistoryErrorCode::StaleSelection, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn cancelled_history_reaps_git_and_correlates_sqlite_without_subjects_or_ref_names() {
    use crate::diagnostics::{DiagnosticStore, OperationContext, OperationKind, ReadOnlyDiagnostics, Query, Event, Component};
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let private = create_commit(&root, &[&head(&root)], "private-history-subject");
    test_support::git(&root, &["update-ref", "refs/heads/main", &private]);
    test_support::git(&root, &["update-ref", "refs/heads/private-history-branch", &private]);
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = Arc::new(RepositoryService::with_diagnostics(store.sink()).with_inspection_executable(&gated_history(temp.path())));
    let entry = selected(&service, &root, &clock).await;
    let context = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::HistoryPage);
    let id = context.id();
    let child = Arc::clone(&service);
    let request = tokio::spawn(async move { context.scope(child.history_page(&entry, None, None)).await });
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    request.abort();
    assert!(clock.finish(request).await.unwrap_err().is_cancelled());
    tokio::time::resume();
    test_support::wait_for_reaped_child(temp.path()).await;
    service.shutdown().await;
    store.flush(Duration::from_secs(5)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(id), ..Default::default() }).unwrap();
    assert!(rows.events.iter().any(|row| row.component == Component::Application && row.event == Event::Cancelled));
    assert!(rows.events.iter().any(|row| row.component == Component::Process && row.event == Event::CleanupCompleted));
    assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::HistoryPage));
    store.shutdown(Duration::from_secs(5)).unwrap();
    let bytes = fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-history-subject".len()).any(|bytes| bytes == b"private-history-subject"));
    assert!(!bytes.windows(b"private-history-branch".len()).any(|bytes| bytes == b"private-history-branch"));
}

#[tokio::test]
async fn malformed_batch_overflow_and_deadline_never_publish_an_empty_clean_graph() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    // Consume the batch request before closing stdin; otherwise a BrokenPipe can
    // win the process race and hide the output error this scenario exercises.
    let malformed = test_support::executable(temp.path(), r#"
case "$*" in *" cat-file --batch"*) cat >/dev/null; printf '%s\n' 'not a batch'; exit 0 ;; esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&malformed);
    let entry = selected(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.history_page(&entry, None, None)).await, HistoryPageResult::Error { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
    let overflow = test_support::executable(temp.path(), r#"
case "$*" in *" cat-file --batch"*) cat >/dev/null; dd if=/dev/zero bs=1048577 count=1 2>/dev/null; exit 0 ;; esac
exec git "$@"
"#);
    let service = RepositoryService::new().with_inspection_executable(&overflow);
    let entry = selected(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.history_page(&entry, None, None)).await, HistoryPageResult::Error { code: HistoryErrorCode::ResourceLimit, .. }));
    service.shutdown().await;
    let entered = temp.path().join("timeout-entered");
    let slow = test_support::executable(temp.path(), &format!(": > {}\nexec sleep 30", test_support::quote(&entered)));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&slow));
    let entry = selected(&service, &root, &clock).await;
    let child = Arc::clone(&service);
    let request = tokio::spawn(async move { child.history_page(&entry, None, None).await });
    clock.wait_for_file(&entered).await;
    clock.advance(Duration::from_secs(31)).await;
    assert!(matches!(clock.finish(request).await.unwrap(), HistoryPageResult::Unavailable { code: HistoryErrorCode::Timeout, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn sha256_repositories_keep_full_commit_tree_and_parent_object_identity() {
    let clock = ManualClock::new();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("sha256");
    fs::create_dir(&root).unwrap();
    test_support::git(&root, &["init", "-b", "main", "--object-format=sha256"]);
    test_support::commit(&root);
    let initial = head(&root);
    let next = create_commit(&root, &[&initial], "sha256 child");
    test_support::git(&root, &["update-ref", "refs/heads/main", &next]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(graph.commits[0].oid, next);
    assert_eq!(graph.commits[0].oid.len(), 64);
    assert_eq!(graph.commits[0].parents, vec![HistoryParent { oid: initial, state: ParentState::Loaded }]);
    assert_eq!(graph.head.oid.as_deref(), Some(next.as_str()));
    service.shutdown().await;
}

#[tokio::test]
async fn upstream_fetches_non_origin_divergence_and_keeps_immutable_side_ranges() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "team", "main:refs/heads/shared"]);
    test_support::git(&root, &["config", "branch.main.remote", "team"]);
    test_support::git(&root, &["config", "branch.main.merge", "refs/heads/shared"]);
    let base = head(&root);
    let local = create_commit(&root, &[&base], "local");
    let incoming = create_commit(&remote, &[&base], "incoming");
    test_support::git(&root, &["update-ref", "refs/heads/main", &local]);
    test_support::git(&remote, &["update-ref", "refs/heads/shared", &incoming]);
    fs::write(root.join("uncommitted"), b"excluded").unwrap();
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let graph = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(graph.upstream.state, UpstreamState::Ready);
    assert_eq!(graph.upstream.freshness, UpstreamFreshness::Fresh);
    assert_eq!((graph.upstream.ahead, graph.upstream.behind), (1, 1));
    assert_eq!(graph.upstream.upstream.as_deref(), Some("team/shared"));
    let range = graph.upstream.incoming.unwrap();
    assert_eq!((&range.base_oid, &range.tip_oid), (&base, &incoming));
    assert_eq!(graph.upstream.outgoing.unwrap().tip_oid, local);
    assert!(matches!(clock.finish(service.upstream_files(&entry, &range.token)).await, crate::inspection::CommitFilesResult::Files { .. }));
    test_support::git(&root, &["remote", "set-url", "team", "/nonexistent-gitview-fixture-remote"]);
    let stale = page(clock.finish(service.history_page(&entry, None, None)).await);
    assert_eq!(stale.upstream.freshness, UpstreamFreshness::Stale);
    assert_eq!((stale.upstream.ahead, stale.upstream.behind), (1, 1));
    assert!(matches!(service.upstream_files(&entry, &range.token).await, crate::inspection::CommitFilesResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    assert_eq!(head(&root), local);
    assert_eq!(fs::read(root.join("uncommitted")).unwrap(), b"excluded");
    service.shutdown().await;
}

#[tokio::test]
async fn upstream_tracks_sync_one_sided_merge_and_detached_states_without_worktree_writes() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    assert_eq!(page(clock.finish(service.history_page(&entry, None, None)).await).upstream.state, UpstreamState::NoUpstream);
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "-u", "team", "main:refs/heads/shared"]);
    let initial = head(&root);
    let synchronized = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((synchronized.state, synchronized.ahead, synchronized.behind), (UpstreamState::Ready, 0, 0));
    assert!(synchronized.incoming.is_none() && synchronized.outgoing.is_none());
    let first = create_commit(&root, &[&initial], "first");
    let second = create_commit(&root, &[&initial], "second");
    let merge = create_commit(&root, &[&first, &second], "merge");
    test_support::git(&root, &["update-ref", "refs/heads/main", &merge]);
    let outgoing = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((outgoing.ahead, outgoing.behind), (3, 0));
    assert_eq!(outgoing.outgoing.unwrap().base_oid, initial);
    test_support::git(&root, &["push", "team", "main:refs/heads/shared"]);
    test_support::git(&root, &["update-ref", "refs/heads/main", &initial]);
    let incoming = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((incoming.ahead, incoming.behind), (0, 3));
    assert_eq!(incoming.incoming.unwrap().tip_oid, merge);
    test_support::git(&root, &["branch", "new-local"]);
    let newly_created = page(clock.finish(service.history_page(&entry, None, Some("new-local"))).await).upstream;
    assert_eq!(newly_created.state, UpstreamState::NoUpstream);
    test_support::git(&root, &["checkout", "--detach"]);
    assert_eq!(page(clock.finish(service.history_page(&entry, None, None)).await).upstream.state, UpstreamState::Detached);
    let viewed = page(clock.finish(service.history_page(&entry, None, Some("main"))).await).upstream;
    assert_eq!((viewed.state, viewed.behind), (UpstreamState::Ready, 3));
    // A deleted remote branch cannot refresh the still-known tracking state.
    test_support::git(&remote, &["update-ref", "-d", "refs/heads/shared"]);
    assert_eq!(page(clock.finish(service.history_page(&entry, None, Some("main"))).await).upstream.freshness, UpstreamFreshness::Stale);
    test_support::git(&root, &["update-ref", "-d", "refs/remotes/team/shared"]);
    assert_eq!(page(clock.finish(service.history_page(&entry, None, Some("main"))).await).upstream.state, UpstreamState::Unavailable);
    service.shutdown().await;
}

#[tokio::test]
async fn upstream_aggregate_diff_is_merge_base_to_side_and_tokens_reject_reselection() {
    use crate::inspection::CommitFilesResult;
    use crate::diff::committed::CommitReviewResult;
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "-u", "team", "main:refs/heads/shared"]);
    test_support::git(&remote, &["checkout", "shared"]);
    fs::write(remote.join("incoming"), b"remote one\n").unwrap();
    test_support::git(&remote, &["add", "incoming"]);
    test_support::git(&remote, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "-m", "remote one"]);
    fs::write(remote.join("incoming"), b"remote final\n").unwrap();
    test_support::git(&remote, &["add", "incoming"]);
    test_support::git(&remote, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "-m", "remote two"]);
    fs::write(root.join("outgoing"), b"local final\n").unwrap();
    test_support::git(&root, &["add", "outgoing"]);
    test_support::git(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "-c", "commit.gpgsign=false", "commit", "-m", "local"]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let summary = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((summary.ahead, summary.behind), (1, 2));
    let incoming = summary.incoming.unwrap();
    let outgoing = summary.outgoing.unwrap();
    for (range, path, content) in [(&incoming, "incoming", "remote final\n"), (&outgoing, "outgoing", "local final\n")] {
        let CommitFilesResult::Files { commit_oid, parent_oid, files, .. } = clock.finish(service.upstream_files(&entry, &range.token)).await else { panic!("range must list files"); };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].display_path, path);
        let CommitReviewResult::Text { from_content, to_content, .. } = clock.finish(service.review_commit_file(&entry, &commit_oid, parent_oid.as_deref(), &files[0].id)).await else { panic!("range must reuse committed review"); };
        assert!(from_content.is_empty());
        assert_eq!(to_content, content);
    }
    assert!(matches!(service.upstream_files(&entry, &incoming.tip_oid).await, CommitFilesResult::Unavailable { code: HistoryErrorCode::StaleCursor, .. }));
    clock.finish(service.select(&entry)).await;
    assert!(matches!(service.upstream_files(&entry, &incoming.token).await, CommitFilesResult::Unavailable { .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn unsafe_fetch_mapping_cannot_update_local_branches() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "-u", "team", "main:refs/heads/shared"]);
    let before = head(&root);
    test_support::git(&root, &["config", "--replace-all", "remote.team.fetch", "+refs/heads/*:refs/heads/*"]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let summary = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!(summary.state, UpstreamState::Unavailable);
    assert_eq!(head(&root), before);
    service.shutdown().await;
}

#[tokio::test]
async fn upstream_pagination_is_pinned_and_fetch_timeout_preserves_local_history() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    linear_history(&root, 110);
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "-u", "team", "main:refs/heads/shared"]);
    let entered = temp.path().join("fetch-entered");
    let slow = test_support::executable(temp.path(), &format!(r#"
case "$*" in *" fetch "*) : > {}; exec sleep 30 ;; esac
exec git "$@"
"#, test_support::quote(&entered)));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&slow));
    let entry = selected(&service, &root, &clock).await;
    let child = Arc::clone(&service);
    let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.history_page(&child_entry, None, None).await });
    clock.wait_for_file(&entered).await;
    clock.advance(Duration::from_secs(31)).await;
    let first = page(clock.finish(request).await.unwrap());
    assert_eq!(first.upstream.freshness, UpstreamFreshness::Stale);
    assert_eq!(first.commits.len(), 100);
    fs::remove_file(&entered).unwrap();
    let next = page(clock.finish(service.history_page(&entry, first.cursor.as_deref(), None)).await);
    assert_eq!(next.upstream, first.upstream);
    assert!(!entered.exists());
    assert_eq!(next.commits.len(), 11);
    service.shutdown().await;
}

#[tokio::test]
async fn unrelated_and_ambiguous_ancestry_never_issue_misleading_side_diffs() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let base = head(&root);
    let unrelated = create_commit(&root, &[], "unrelated");
    test_support::git(&root, &["update-ref", "refs/heads/tracked", &unrelated]);
    test_support::git(&root, &["config", "branch.main.remote", "."]);
    test_support::git(&root, &["config", "branch.main.merge", "refs/heads/tracked"]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let summary = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((summary.state, summary.ahead, summary.behind), (UpstreamState::Unavailable, 1, 1));
    assert!(summary.incoming.is_none() && summary.outgoing.is_none());
    let left = create_commit(&root, &[&base], "left");
    let right = create_commit(&root, &[&base], "right");
    let local = create_commit(&root, &[&left, &right], "local merge");
    let remote = create_commit(&root, &[&right, &left], "remote merge");
    test_support::git(&root, &["update-ref", "refs/heads/main", &local]);
    test_support::git(&root, &["update-ref", "refs/heads/tracked", &remote]);
    let summary = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((summary.state, summary.ahead, summary.behind), (UpstreamState::Unavailable, 1, 1));
    assert!(summary.incoming.is_none() && summary.outgoing.is_none());
    service.shutdown().await;
}

#[tokio::test]
async fn configured_pruning_hooks_and_extra_refmaps_cannot_mutate_local_work() {
    use std::os::unix::fs::PermissionsExt;
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let (_remote_temp, remote) = test_support::working_tree();
    test_support::git(&root, &["remote", "add", "unused", "/nonexistent-gitview-fixture-remote"]);
    test_support::git(&root, &["remote", "add", "team", remote.to_str().unwrap()]);
    test_support::git(&root, &["push", "-u", "team", "main:refs/heads/shared"]);
    test_support::git(&root, &["tag", "local-only"]);
    test_support::git(&root, &["branch", "protected"]);
    test_support::git(&root, &["config", "--add", "remote.team.fetch", "+refs/heads/main:refs/heads/protected"]);
    test_support::git(&root, &["config", "fetch.prune", "true"]);
    test_support::git(&root, &["config", "fetch.pruneTags", "true"]);
    test_support::git(&root, &["config", "remote.team.prune", "true"]);
    let marker = root.join("hook-wrote-working-tree");
    let hook = root.join(".git/hooks/reference-transaction");
    fs::write(&hook, format!("#!/bin/sh\nprintf unsafe > {}\n", test_support::quote(&marker))).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let initial = head(&root);
    let index = fs::read(root.join(".git/index")).unwrap();
    let next = create_commit(&remote, &[&initial], "incoming");
    test_support::git(&remote, &["update-ref", "refs/heads/shared", &next]);
    let service = RepositoryService::new();
    let entry = selected(&service, &root, &clock).await;
    let summary = page(clock.finish(service.history_page(&entry, None, None)).await).upstream;
    assert_eq!((summary.state, summary.freshness, summary.behind), (UpstreamState::Ready, UpstreamFreshness::Fresh, 1));
    assert!(!marker.exists());
    assert_eq!(head(&root), initial);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    for reference in ["refs/heads/protected", "refs/tags/local-only"] {
        assert_eq!(String::from_utf8(test_support::git_output(&root, &["rev-parse", reference]).stdout).unwrap().trim(), initial);
    }
    service.shutdown().await;
}
