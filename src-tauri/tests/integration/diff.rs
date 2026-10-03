//! Exercises production review with isolated Git roots, descriptor reads and causal SQLite capture.

use super::*;
use crate::application::RepositoryService;
use crate::observation::{ChangedPath, ObservationSnapshot};
use crate::workspace::OpenOutcome;
use crate::test_support::{self, ManualClock};
use std::{fs, path::{Path, PathBuf}, sync::Arc, time::Duration};

async fn select(service: &RepositoryService, root: &Path, clock: &ManualClock) -> (String, u64, Vec<ChangedPath>) {
    let OpenOutcome::Opened { entry_id, .. } = clock.finish(service.open_chosen(root)).await else { panic!("fixture not admitted") };
    clock.finish(service.select(&entry_id)).await;
    let (revision, paths) = ready(service, &entry_id, clock).await;
    (entry_id, revision, paths)
}
async fn ready(service: &RepositoryService, entry: &str, clock: &ManualClock) -> (u64, Vec<ChangedPath>) {
    clock.finish(async {
        loop {
            match service.observe_selected_context(entry).await {
                ObservationSnapshot::Ready { observation_revision, files, .. } => return (observation_revision, files),
                ObservationSnapshot::Unavailable { error_code, .. } => panic!("fixture status unavailable: {error_code:?}"),
                _ => tokio::task::yield_now().await,
            }
        }
    }).await
}
fn path<'a>(paths: &'a [ChangedPath], name: &str) -> &'a str { &paths.iter().find(|path| path.display_path == name).unwrap().path_id }
fn text(result: ReviewResult) -> (ReviewIdentity, Vec<TextHunk>, String, String) {
    match result { ReviewResult::Text { identity, hunks, from_content, to_content } => (identity, hunks, from_content, to_content), other => panic!("expected text: {other:?}") }
}

#[tokio::test]
async fn head_index_worktree_are_distinct_and_read_only_for_literal_special_paths() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let name = ":(glob)* [bracket]\tquote\"\nfile.txt";
    fs::write(root.join(name), b"HEAD\n").unwrap();
    test_support::commit(&root);
    fs::write(root.join(name), b"index\n").unwrap();
    test_support::git(&root, &["--literal-pathspecs", "add", "--", name]);
    fs::write(root.join(name), b"worktree").unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let head = fs::read(root.join(".git/HEAD")).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let token = path(&paths, name);
    let (staged, staged_hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, token, ReviewCategory::Staged)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("HEAD\n", "index\n"));
    assert_eq!((staged.from, staged.to, staged.from_absent, staged.to_absent), (ReviewFrom::Head, ReviewTo::Index, false, false));
    assert_eq!(staged_hunks[0].lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["HEAD", "index"]);
    let (unstaged, unstaged_hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, token, ReviewCategory::Unstaged)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("index\n", "worktree"));
    assert_eq!((unstaged.from, unstaged.to), (ReviewFrom::Index, ReviewTo::WorkingFiles));
    assert_eq!(unstaged_hunks[0].lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["index", "worktree"]);
    assert!(unstaged_hunks[0].lines[1].no_final_newline);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::read(root.join(name)).unwrap(), b"worktree");
    service.shutdown().await;
}

#[tokio::test]
async fn absent_endpoints_do_not_confuse_empty_files_with_deletion() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("deleted"), b"gone\n").unwrap();
    test_support::commit(&root);
    fs::remove_file(root.join("deleted")).unwrap();
    test_support::git(&root, &["add", "--", "deleted"]);
    fs::write(root.join("added"), b"").unwrap();
    test_support::git(&root, &["add", "--", "added"]);
    fs::write(root.join("untracked"), b"").unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let (deleted, hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, path(&paths, "deleted"), ReviewCategory::Staged)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("gone\n", ""));
    assert_eq!((deleted.from_absent, deleted.to_absent), (false, true));
    assert_eq!(hunks[0].lines[0].kind, LineKind::Removal);
    let (added, hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, path(&paths, "added"), ReviewCategory::Staged)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("", ""));
    assert_eq!((added.from_absent, added.to_absent, hunks), (true, false, Vec::new()));
    let (untracked, hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, path(&paths, "untracked"), ReviewCategory::Untracked)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("", ""));
    assert_eq!((untracked.from, untracked.from_absent, untracked.to_absent, hunks), (ReviewFrom::Absent, true, false, Vec::new()));
    service.shutdown().await;
}

#[tokio::test]
async fn untracked_content_encoding_and_resource_failures_remain_explicit() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("binary"), b"a\0b").unwrap();
    fs::write(root.join("encoding"), b"\xff").unwrap();
    fs::write(root.join("large"), vec![b'x'; CONTENT_LIMIT + 1]).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    for (name, expected) in [("binary", UnsupportedReason::Binary), ("encoding", UnsupportedReason::UnsupportedEncoding), ("large", UnsupportedReason::LargeOrTruncated)] {
        let result = clock.finish(service.review_file(&entry, revision, path(&paths, name), ReviewCategory::Untracked)).await;
        assert!(matches!(result, ReviewResult::Unsupported { reason, .. } if reason == expected), "{result:?}");
    }
    service.shutdown().await;
}

#[tokio::test]
async fn unborn_staged_review_is_not_an_implicit_empty_head_policy() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::unborn_working_tree();
    fs::write(root.join("added"), b"initial\n").unwrap();
    test_support::git(&root, &["add", "--", "added"]);
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "added"), ReviewCategory::Staged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::UnbornHead, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn stale_revision_token_category_and_selection_cannot_authorize_native_reads() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("untracked"), b"source").unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let token = path(&paths, "untracked");
    assert_eq!(service.review_file(&entry, revision + 1, token, ReviewCategory::Untracked).await, ReviewResult::StaleObservation);
    assert_eq!(service.review_file(&entry, revision, "forged", ReviewCategory::Untracked).await, ReviewResult::StaleObservation);
    assert_eq!(service.review_file(&entry, revision, token, ReviewCategory::Staged).await, ReviewResult::StaleObservation);
    assert_eq!(service.review_file("other", revision, token, ReviewCategory::Untracked).await, ReviewResult::StaleSelection);
    clock.finish(service.select(&entry)).await;
    assert_eq!(service.review_file(&entry, revision, token, ReviewCategory::Untracked).await, ReviewResult::StaleObservation);
    service.shutdown().await;
}

#[tokio::test]
async fn rooted_reader_rejects_symlink_components_special_files_and_replaced_roots() {
    use std::os::unix::fs::symlink;
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let outside = temp.path().join("private");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), b"private payload").unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    symlink(outside.join("secret"), root.join("link")).unwrap();
    use std::os::unix::ffi::OsStrExt;
    let fifo = std::ffi::CString::new(root.join("fifo").as_os_str().as_bytes()).unwrap();
    // SAFETY: the isolated native fixture pathname is NUL terminated.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let facts = clock.finish(GitProbe::default().probe(&root)).await.unwrap();
    let context = SelectedContext { entry_id: "fixture".into(), identity: NativeIdentity::capture(&facts.root, &facts.git_dir).unwrap(), root: facts.root, git_dir: facts.git_dir, kind: facts.kind };
    assert_eq!(clock.finish(rooted_read::read(&context, Path::new("escape/secret"), ProbeDeadline::new())).await, Err(unavailable(ReviewErrorCode::ChangedDuringRead)));
    assert_eq!(clock.finish(rooted_read::read(&context, Path::new("link"), ProbeDeadline::new())).await, Err(unavailable(ReviewErrorCode::ChangedDuringRead)));
    assert_eq!(clock.finish(rooted_read::read(&context, Path::new("../private/secret"), ProbeDeadline::new())).await, Err(unavailable(ReviewErrorCode::InvalidOutput)));
    assert_eq!(clock.finish(rooted_read::read(&context, Path::new("fifo"), ProbeDeadline::new())).await, Err(unsupported(UnsupportedReason::TypeChange)));
    fs::rename(&root, temp.path().join("original")).unwrap();
    fs::create_dir(&root).unwrap();
    fs::write(root.join("secret"), b"replacement").unwrap();
    assert_eq!(clock.finish(rooted_read::read(&context, Path::new("secret"), ProbeDeadline::new())).await, Err(unavailable(ReviewErrorCode::ChangedDuringRead)));
}

fn gated_review(at: &Path) -> PathBuf {
    test_support::executable(at, &format!(r#"
case "$*" in
  *" cat-file "*)
    git "$@" || exit $?
    printf '%s\n' "$$" > {pid}
    : > {entered}
    while [ ! -f {release} ]; do sleep 0.01; done
    exit 0 ;;
esac
exec git "$@"
"#, pid = test_support::quote(&at.join(".gitview-child-pid")), entered = test_support::quote(&at.join(".gitview-entered")), release = test_support::quote(&at.join(".gitview-release"))))
}

async fn gated_fixture(clock: &ManualClock) -> (tempfile::TempDir, PathBuf, Arc<RepositoryService>, String, u64, String) {
    let (temp, root) = test_support::working_tree();
    fs::write(root.join("file"), b"before\n").unwrap();
    test_support::commit(&root);
    fs::write(root.join("file"), b"after\n").unwrap();
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&gated_review(temp.path())));
    let (entry, revision, paths) = select(&service, &root, clock).await;
    let token = path(&paths, "file").to_owned();
    (temp, root, service, entry, revision, token)
}
fn start_review(service: &Arc<RepositoryService>, entry: &str, revision: u64, token: &str) -> tokio::task::JoinHandle<ReviewResult> {
    let service = Arc::clone(service);
    let entry = entry.to_owned();
    let token = token.to_owned();
    tokio::spawn(async move { service.review_file(&entry, revision, &token, ReviewCategory::Unstaged).await })
}

#[tokio::test]
async fn same_entry_reselection_during_io_discards_completed_payload() {
    let clock = ManualClock::new();
    let (temp, _root, service, entry, revision, token) = gated_fixture(&clock).await;
    let review = start_review(&service, &entry, revision, &token);
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    clock.finish(service.select(&entry)).await;
    test_support::release(temp.path());
    assert_eq!(clock.finish(review).await.unwrap(), ReviewResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn changing_bytes_without_changing_status_category_is_unavailable_not_old_success() {
    let clock = ManualClock::new();
    let (temp, root, service, entry, revision, token) = gated_fixture(&clock).await;
    let review = start_review(&service, &entry, revision, &token);
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    fs::write(root.join("file"), b"second edit\n").unwrap();
    // The first index read was captured; the working read has not happened yet.
    // Change the index while retaining the same ordinary unstaged category.
    fs::write(root.join("new-index"), b"new index\n").unwrap();
    test_support::git(&root, &["hash-object", "-w", "new-index"]);
    fs::write(root.join("file"), b"new index\n").unwrap();
    test_support::git(&root, &["add", "--", "file"]);
    fs::write(root.join("file"), b"second edit\n").unwrap();
    test_support::release(temp.path());
    assert!(matches!(clock.finish(review).await.unwrap(), ReviewResult::Unavailable { code: ReviewErrorCode::ChangedDuringRead, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn cancelled_review_reaps_git_and_retains_safe_causal_failure_evidence() {
    use crate::diagnostics::{DiagnosticStore, OperationContext, OperationKind, ReadOnlyDiagnostics, Query, Event, Component};
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    fs::write(root.join("private-name"), b"private source\n").unwrap();
    test_support::commit(&root);
    fs::write(root.join("private-name"), b"private newer source\n").unwrap();
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = Arc::new(RepositoryService::with_diagnostics(store.sink()).with_inspection_executable(&gated_review(temp.path())));
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let token = path(&paths, "private-name").to_owned();
    let context = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::ReviewFile);
    let id = context.id();
    let child_service = Arc::clone(&service);
    let review = tokio::spawn(async move { context.scope(child_service.review_file(&entry, revision, &token, ReviewCategory::Unstaged)).await });
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    review.abort();
    assert!(clock.finish(review).await.unwrap_err().is_cancelled());
    // Reaping is an OS event; its polling sleep must use real time, not the paused clock.
    tokio::time::resume();
    test_support::wait_for_reaped_child(temp.path()).await;
    service.shutdown().await;
    // OS disappearance can precede the detached reaper's diagnostic submission.
    let rows = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            store.flush(Duration::from_secs(5)).unwrap();
            let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query {
                operation_id: Some(id), limit: 200, ..Default::default()
            }).unwrap();
            assert!(!rows.has_more);
            if rows.events.iter().any(|row| row.component == Component::Process && row.event == Event::CleanupCompleted) {
                break rows;
            }
            tokio::task::yield_now().await;
        }
    }).await.expect("reaped review must submit its cleanup evidence");
    assert!(rows.events.iter().any(|row| row.component == Component::Application && row.event == Event::Cancelled));
    assert!(rows.events.iter().any(|row| row.component == Component::Process && row.event == Event::CleanupCompleted));
    assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::ReviewFile));
    store.shutdown(Duration::from_secs(5)).unwrap();
    let bytes = fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-name".len()).any(|bytes| bytes == b"private-name"));
    assert!(!bytes.windows(b"private source".len()).any(|bytes| bytes == b"private source"));
}

#[tokio::test]
async fn mode_only_changes_are_unsupported_instead_of_apparently_no_remaining_changes() {
    use std::os::unix::fs::PermissionsExt;
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("mode"), b"unchanged\n").unwrap();
    test_support::commit(&root);
    fs::set_permissions(root.join("mode"), fs::Permissions::from_mode(0o755)).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "mode"), ReviewCategory::Unstaged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::Other, .. }));
    test_support::git(&root, &["add", "--", "mode"]);
    clock.finish(service.select(&entry)).await;
    let (revision, paths) = ready(&service, &entry, &clock).await;
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "mode"), ReviewCategory::Staged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::Other, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn rename_submodule_and_type_changes_are_explicit_unsupported_reviews() {
    use std::os::unix::fs::symlink;
    let clock = ManualClock::new();
    let (_source_temp, source) = test_support::working_tree();
    fs::write(source.join("file"), b"initial\n").unwrap();
    test_support::commit(&source);
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("regular"), b"regular\n").unwrap();
    fs::write(root.join("old"), b"rename\n").unwrap();
    test_support::git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", source.to_str().unwrap(), "module"]);
    test_support::commit(&root);
    fs::write(root.join("module/file"), b"edited\n").unwrap();
    fs::remove_file(root.join("regular")).unwrap();
    symlink("target", root.join("regular")).unwrap();
    test_support::git(&root, &["mv", "old", "new"]);
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    for (name, expected) in [("module", UnsupportedReason::Submodule), ("regular", UnsupportedReason::TypeChange), ("new", UnsupportedReason::RenameOrCopy)] {
        let result = clock.finish(service.review_file(&entry, revision, path(&paths, name), ReviewCategory::Unstaged)).await;
        assert!(matches!(result, ReviewResult::Unsupported { reason, .. } if reason == expected), "{result:?}");
    }
    service.shutdown().await;
}

#[tokio::test]
async fn configured_filters_added_after_observation_never_execute_during_review() {
    let clock = ManualClock::new();
    for operation in ["clean", "process"] {
        let (temp, root) = test_support::working_tree();
        fs::write(root.join(".gitattributes"), b"tracked filter=side_effect\n").unwrap();
        fs::write(root.join("tracked"), b"initial\n").unwrap();
        test_support::commit(&root);
        fs::write(root.join("tracked"), b"changed\n").unwrap();
        let service = RepositoryService::new();
        let (entry, revision, paths) = select(&service, &root, &clock).await;
        let marker = temp.path().join("review-filter-ran");
        let filter = format!("touch {}; cat", test_support::quote(&marker));
        test_support::git(&root, &["config", &format!("filter.side_effect.{operation}"), &filter]);

        let result = clock.finish(service.review_file(&entry, revision, path(&paths, "tracked"), ReviewCategory::Unstaged)).await;

        assert!(!marker.exists());
        assert!(matches!(result, ReviewResult::Unsupported { reason: UnsupportedReason::Other, .. }), "{result:?}");
        service.shutdown().await;
    }
}

#[tokio::test]
async fn conflicts_never_render_as_ordinary_text_comparisons() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("conflict"), b"initial\n").unwrap();
    test_support::commit(&root);
    test_support::git(&root, &["checkout", "-b", "other"]);
    fs::write(root.join("conflict"), b"other\n").unwrap();
    test_support::commit(&root);
    test_support::git(&root, &["checkout", "main"]);
    fs::write(root.join("conflict"), b"main\n").unwrap();
    test_support::commit(&root);
    assert!(!test_support::git_output(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "merge", "other"]).status.success());
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "conflict"), ReviewCategory::Unstaged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::Conflict, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn diff_process_malformed_overflow_and_unsafe_failures_are_not_empty_success() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    fs::write(root.join("untracked"), b"ordinary source\n").unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let facts = clock.finish(GitProbe::default().probe(&root)).await.unwrap();
    let context = SelectedContext { entry_id: entry, identity: NativeIdentity::capture(&facts.root, &facts.git_dir).unwrap(), root: facts.root, git_dir: facts.git_dir, kind: facts.kind };
    let native_path = clock.finish(GitStatusReader::default().read(&root)).await.unwrap().remove(0);
    let identity = identity(&context, path(&paths, "untracked"), &native_path, ReviewCategory::Untracked);
    let malformed = test_support::executable(temp.path(), "printf '%s\\n' 'not a patch'\nexit 1");
    assert!(matches!(clock.finish(read_review(&GitProcess::with_executable(&malformed), &context, &native_path, identity.clone())).await,
        ReviewResult::Unavailable { code: ReviewErrorCode::InvalidOutput, .. }));
    let overflow = test_support::executable(temp.path(), "dd if=/dev/zero bs=1048577 count=1 2>/dev/null\nexit 1");
    assert!(matches!(clock.finish(read_review(&GitProcess::with_executable(&overflow), &context, &native_path, identity.clone())).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::LargeOrTruncated, .. }));
    let unsafe_git = test_support::executable(temp.path(), "printf '%s' 'fatal: detected dubious ownership' >&2\nexit 128");
    assert!(matches!(clock.finish(read_review(&GitProcess::with_executable(&unsafe_git), &context, &native_path, identity)).await,
        ReviewResult::Unavailable { code: ReviewErrorCode::UnsafeRepository, .. }));
    assert_eq!(service.review_file(&context.entry_id, revision + 1, path(&paths, "untracked"), ReviewCategory::Untracked).await, ReviewResult::StaleObservation);
    service.shutdown().await;
}

#[tokio::test]
async fn advancing_observation_during_io_discards_an_older_revision_payload() {
    let clock = ManualClock::new();
    let (temp, root, service, entry, revision, token) = gated_fixture(&clock).await;
    let review = start_review(&service, &entry, revision, &token);
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    fs::write(root.join("new-path"), b"changed observation facts\n").unwrap();
    // Advance past the timer wheel's boundary, not merely to its exact sleep deadline.
    clock.advance(Duration::from_secs(2)).await;
    clock.finish(async {
        loop {
            if let ObservationSnapshot::Ready { observation_revision, .. } = service.observe_selected_context(&entry).await {
                if observation_revision > revision { break; }
            }
            tokio::task::yield_now().await;
        }
    }).await;
    test_support::release(temp.path());
    assert_eq!(clock.finish(review).await.unwrap(), ReviewResult::StaleObservation);
    service.shutdown().await;
}

#[tokio::test]
async fn symlink_swap_after_authorization_cannot_expose_outside_worktree_content() {
    use std::os::unix::fs::symlink;
    let clock = ManualClock::new();
    let (temp, root, service, entry, revision, token) = gated_fixture(&clock).await;
    let outside = temp.path().join("outside-private");
    fs::write(&outside, b"never expose private bytes\n").unwrap();
    let review = start_review(&service, &entry, revision, &token);
    clock.wait_for_file(&temp.path().join(".gitview-entered")).await;
    fs::remove_file(root.join("file")).unwrap();
    symlink(&outside, root.join("file")).unwrap();
    test_support::release(temp.path());
    assert!(matches!(clock.finish(review).await.unwrap(), ReviewResult::Unavailable { code: ReviewErrorCode::ChangedDuringRead, .. }));
    assert_eq!(fs::read(outside).unwrap(), b"never expose private bytes\n");
    service.shutdown().await;
}

#[tokio::test]
async fn review_process_deadline_terminates_instead_of_publishing_a_truncated_patch() {
    let clock = ManualClock::new();
    let temp = tempfile::tempdir().unwrap();
    let entered = temp.path().join("entered");
    let executable = test_support::executable(temp.path(), &format!(": > {}\nexec sleep 30", test_support::quote(&entered)));
    let review = tokio::spawn(async move {
        compare(&GitProcess::with_executable(&executable), (b"old\n".to_vec(), b"new\n".to_vec()), ProbeDeadline::after(Duration::from_secs(1))).await
    });
    clock.wait_for_file(&entered).await;
    clock.advance(Duration::from_secs(2)).await;
    assert_eq!(clock.finish(review).await.unwrap(), Err(unavailable(ReviewErrorCode::Timeout)));
}

#[tokio::test]
async fn repository_external_diff_textconv_and_fsmonitor_cannot_run_during_review() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    fs::write(root.join("file"), b"initial\n").unwrap();
    fs::write(root.join(".gitattributes"), b"file diff=private-driver\n").unwrap();
    test_support::commit(&root);
    fs::write(root.join("file"), b"changed\n").unwrap();
    let marker = temp.path().join("unexpected-hook");
    let hook = test_support::executable(temp.path(), &format!(": > {}\nexit 1", test_support::quote(&marker)));
    test_support::git(&root, &["config", "diff.external", hook.to_str().unwrap()]);
    test_support::git(&root, &["config", "diff.private-driver.textconv", hook.to_str().unwrap()]);
    test_support::git(&root, &["config", "core.fsmonitor", hook.to_str().unwrap()]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let (_, hunks, ..) = text(clock.finish(service.review_file(&entry, revision, path(&paths, "file"), ReviewCategory::Unstaged)).await);
    assert_eq!(hunks[0].lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["initial", "changed"]);
    assert!(!marker.exists());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    service.shutdown().await;
}

#[tokio::test]
async fn a_middle_file_edit_returns_full_endpoints_matching_real_git_hunks() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let original = format!("é — full-file prefix\r\n{}tail without newline", (2..=100).map(|line| format!("original line {line}\n")).collect::<String>());
    fs::write(root.join("middle.txt"), &original).unwrap();
    test_support::commit(&root);
    let edited = original.replace("original line 15\n", "middle file edit\n");
    fs::write(root.join("middle.txt"), &edited).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let result = clock.finish(service.review_file(&entry, revision, path(&paths, "middle.txt"), ReviewCategory::Unstaged)).await;
    let serialized = serde_json::to_value(&result).unwrap();
    assert_eq!(serialized["fromContent"], original);
    assert_eq!(serialized["toContent"], edited);
    let (_, hunks, from_content, to_content) = text(result);
    assert_eq!(from_content, original);
    assert_eq!(to_content, edited);
    assert_eq!((hunks[0].old_start, hunks[0].new_start), (12, 12));
    assert!(hunks[0].lines.iter().any(|line| line.kind == LineKind::Removal && line.text == "original line 15"));
    assert!(hunks[0].lines.iter().any(|line| line.kind == LineKind::Addition && line.text == "middle file edit"));
    assert!(!hunks[0].lines.iter().any(|line| line.text.contains("full-file prefix") || line.text == "tail without newline"));
    service.shutdown().await;
}

#[tokio::test]
async fn unchanged_endpoint_lines_are_bounded_even_when_the_patch_is_small() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let original = "\n".repeat(parser::MAX_LINES);
    fs::write(root.join("within"), &original).unwrap();
    fs::write(root.join("over"), format!("{original}tail")).unwrap();
    fs::write(root.join("over-to"), &original).unwrap();
    test_support::commit(&root);
    fs::write(root.join("within"), format!("changed\n{}", &original[1..])).unwrap();
    fs::write(root.join("over"), format!("changed\n{}tail", &original[1..])).unwrap();
    fs::write(root.join("over-to"), format!("{original}tail")).unwrap();
    let service = RepositoryService::new();
    let (entry, revision, paths) = select(&service, &root, &clock).await;
    let (_, hunks, from_content, to_content) = text(clock.finish(service.review_file(&entry, revision, path(&paths, "within"), ReviewCategory::Unstaged)).await);
    assert_eq!(from_content, original);
    assert_eq!(to_content, format!("changed\n{}", &original[1..]));
    assert!(hunks[0].lines.iter().any(|line| line.kind == LineKind::Addition && line.text == "changed"));
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "over"), ReviewCategory::Unstaged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::LargeOrTruncated, .. }));
    assert!(matches!(clock.finish(service.review_file(&entry, revision, path(&paths, "over-to"), ReviewCategory::Unstaged)).await,
        ReviewResult::Unsupported { reason: UnsupportedReason::LargeOrTruncated, .. }));
    service.shutdown().await;
}
