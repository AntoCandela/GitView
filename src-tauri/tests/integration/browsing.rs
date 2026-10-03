//! Exercises real filesystem browsing, retained native authority and safe working-text reads.

use super::*;
use crate::{application::RepositoryService, diagnostics::{Component, DiagnosticStore, Event, OperationContext, OperationKind, Query, ReadOnlyDiagnostics},
    test_support::{self, ManualClock}, workspace::{NativeIdentity, OpenOutcome}};
use std::{fs, os::unix::fs::symlink, path::Path, sync::Arc, time::Duration};

async fn select(service: &RepositoryService, root: &Path, clock: &ManualClock) -> String {
    let OpenOutcome::Opened { entry_id, .. } = clock.finish(service.open_chosen(root)).await else { panic!("fixture must open") };
    clock.finish(service.select(&entry_id)).await;
    entry_id
}
async fn listing(service: &RepositoryService, entry: &str, clock: &ManualClock) -> (String, Vec<RepositoryFile>) {
    let mut request = RepositoryFilesRequest::default();
    let mut listing = None;
    let mut all_files = Vec::new();
    let mut pending = VecDeque::new();
    loop {
        match clock.finish(service.list_repository_files(entry, request)).await {
            RepositoryFilesResult::Files { entry_id, listing_id, directory_id, files, directories, cursor } => {
                assert_eq!(entry_id, entry);
                if let Some(previous) = &listing { assert_eq!(previous, &listing_id); }
                listing = Some(listing_id.clone());
                all_files.extend(files);
                pending.extend(directories.into_iter().map(|directory| directory.id));
                request = if let Some(cursor) = cursor {
                    RepositoryFilesRequest { listing_id: Some(listing_id), directory_id, cursor: Some(cursor) }
                } else if let Some(directory) = pending.pop_front() {
                    RepositoryFilesRequest { listing_id: Some(listing_id), directory_id: Some(directory), cursor: None }
                } else { break; };
            }
            other => panic!("expected repository files: {other:?}"),
        }
    }
    all_files.sort_unstable_by(|left, right| left.display_path.cmp(&right.display_path));
    (listing.unwrap(), all_files)
}

struct TestPage {
    listing_id: String,
    directory_id: Option<String>,
    files: Vec<RepositoryFile>,
    directories: Vec<RepositoryDirectory>,
    cursor: Option<String>,
}
impl TestPage {
    fn continuation(&self) -> RepositoryFilesRequest {
        RepositoryFilesRequest { listing_id: Some(self.listing_id.clone()), directory_id: self.directory_id.clone(), cursor: self.cursor.clone() }
    }
    fn directory(&self, name: &str) -> RepositoryFilesRequest {
        let directory = self.directories.iter().find(|directory| directory.display_path == name).expect("issued directory");
        RepositoryFilesRequest { listing_id: Some(self.listing_id.clone()), directory_id: Some(directory.id.clone()), cursor: None }
    }
}
async fn page(service: &RepositoryService, entry: &str, request: RepositoryFilesRequest, clock: &ManualClock) -> TestPage {
    match clock.finish(service.list_repository_files(entry, request)).await {
        RepositoryFilesResult::Files { entry_id, listing_id, directory_id, files, directories, cursor } => {
            assert_eq!(entry_id, entry);
            assert!(files.len() + directories.len() <= PAGE_ENTRIES);
            TestPage { listing_id, directory_id, files, directories, cursor }
        }
        other => panic!("expected directory page: {other:?}"),
    }
}
fn populate_directory(path: &Path, count: usize, long_names: bool) {
    fs::create_dir(path).unwrap();
    let suffix = if long_names { "x".repeat(110) } else { String::new() };
    for index in 0..count { fs::write(path.join(format!("file-{index:05}{suffix}")), "working bytes\n").unwrap(); }
}
fn named<'a>(files: &'a [RepositoryFile], path: &str) -> &'a RepositoryFile {
    files.iter().find(|file| file.display_path == path).expect("fixture file must be listed")
}
fn text(result: RepositoryFileResult) -> String {
    match result { RepositoryFileResult::Text { content, .. } => content, other => panic!("expected working text: {other:?}") }
}
fn native_context(root: &Path) -> SelectedContext {
    let root = fs::canonicalize(root).unwrap();
    let git_dir = root.join(".git");
    let identity = NativeIdentity::capture(&root, &git_dir).unwrap();
    SelectedContext { entry_id: "fixture".into(), root, git_dir, identity, kind: RepositoryKind::WorkingTree }
}

#[tokio::test]
async fn unchanged_ignored_untracked_and_dotfiles_are_complete_and_read_exact_working_text() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::unborn_working_tree();
    fs::write(root.join("unchanged.txt"), "unchanged\r\nlast line").unwrap();
    fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    test_support::commit(&root);
    fs::create_dir(root.join("ignored")).unwrap();
    fs::write(root.join("ignored/private.txt"), "ignored text\n").unwrap();
    fs::write(root.join(".hidden"), "dotfile\n").unwrap();
    fs::write(root.join("untracked\tfile\n.txt"), "untracked\n").unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    assert_eq!(files.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(),
        [".gitignore", ".hidden", "ignored/private.txt", "unchanged.txt", "untracked\tfile\n.txt"]);
    assert_eq!(named(&files, "ignored/private.txt").segments, ["ignored", "private.txt"]);
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "unchanged.txt").id)).await), "unchanged\r\nlast line");
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "ignored/private.txt").id)).await), "ignored text\n");
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &token, &named(&files, ".hidden").id)).await), "dotfile\n");
    service.shutdown().await;
}

#[tokio::test]
async fn nested_repositories_and_real_submodules_are_opaque_without_metadata_or_child_files() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let nested = root.join("nested-repository"); fs::create_dir(&nested).unwrap();
    test_support::git(&nested, &["init", "-b", "main"]);
    fs::write(nested.join("secret.txt"), "nested bytes").unwrap();
    let (_source_temp, source) = test_support::working_tree();
    fs::write(source.join("submodule-file"), "submodule bytes").unwrap(); test_support::commit(&source);
    test_support::git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", source.to_str().unwrap(), "module"]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    assert_eq!(files.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), [".gitmodules", "module", "nested-repository"]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "module").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "nested-repository").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    service.shutdown().await;
}

#[tokio::test]
async fn nested_bare_repositories_are_opaque_without_head_config_objects_or_refs_descendants() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let bare = root.join("mirror.git"); fs::create_dir(&bare).unwrap();
    test_support::git(&bare, &["init", "--bare"]);
    test_support::git(&root, &["push", bare.to_str().unwrap(), "HEAD:refs/heads/main"]);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    assert_eq!(files.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), ["mirror.git"]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &files[0].id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    assert_eq!(service.review_repository_file(&entry, &token, "mirror.git/config").await,
        RepositoryFileResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn ordinary_config_is_readable_but_an_ancestor_converted_to_bare_revokes_old_read_authority() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let cache = root.join("cache"); fs::create_dir(&cache).unwrap();
    fs::write(cache.join("config"), "ordinary cache config\n").unwrap();
    fs::create_dir(cache.join("objects")).unwrap(); fs::create_dir(cache.join("refs")).unwrap();
    fs::write(cache.join("HEAD"), "ordinary cache header\n").unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    let config = named(&files, "cache/config");
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &token, &config.id)).await), "ordinary cache config\n");
    fs::remove_file(cache.join("config")).unwrap(); fs::remove_file(cache.join("HEAD")).unwrap();
    test_support::git(&cache, &["init", "--bare"]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &config.id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    let (_, refreshed) = listing(&service, &entry, &clock).await;
    assert_eq!(refreshed.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), ["cache"]);
    service.shutdown().await;
}

#[tokio::test]
async fn separate_git_directory_inside_the_root_is_excluded_entirely() {
    let clock = ManualClock::new();
    let temp = tempfile::tempdir().unwrap(); let root = temp.path().join("project"); fs::create_dir(&root).unwrap();
    let metadata = root.join("native-metadata");
    test_support::git(&root, &["init", "-b", "main", &format!("--separate-git-dir={}", metadata.to_str().unwrap())]);
    fs::write(root.join("file.txt"), "working\n").unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (_, files) = listing(&service, &entry, &clock).await;
    assert_eq!(files.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), ["file.txt"]);
    service.shutdown().await;
}

#[tokio::test]
async fn symlinks_are_listed_but_never_followed_and_post_listing_swaps_cannot_expose_outside_bytes() {
    let clock = ManualClock::new();
    let (temp, root) = test_support::working_tree();
    let outside = temp.path().join("outside"); fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), "OUTSIDE BYTES").unwrap();
    symlink(outside.join("secret"), root.join("file-link")).unwrap();
    symlink(&outside, root.join("directory-link")).unwrap();
    fs::write(root.join("regular"), "inside").unwrap();
    fs::create_dir(root.join("nested")).unwrap(); fs::write(root.join("nested/secret"), "inside nested").unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    assert_eq!(files.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), ["directory-link", "file-link", "nested/secret", "regular"]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "file-link").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::TypeChange });
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "directory-link").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::TypeChange });
    fs::remove_file(root.join("regular")).unwrap(); symlink(outside.join("secret"), root.join("regular")).unwrap();
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "regular").id)).await,
        RepositoryFileResult::Unavailable { code: ReviewErrorCode::ChangedDuringRead });
    fs::rename(root.join("nested"), root.join("original-nested")).unwrap(); symlink(&outside, root.join("nested")).unwrap();
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "nested/secret").id)).await,
        RepositoryFileResult::Unavailable { code: ReviewErrorCode::ChangedDuringRead });
    service.shutdown().await;
}

#[tokio::test]
async fn directories_that_become_submodules_after_listing_are_not_read_through_old_tokens() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    fs::create_dir(root.join("nested")).unwrap(); fs::write(root.join("nested/file"), "formerly ordinary").unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    fs::write(root.join("nested/.git"), "gitdir: /unreachable\n").unwrap();
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "nested/file").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    fs::remove_file(root.join("nested/.git")).unwrap();
    let oid = String::from_utf8(test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout).unwrap();
    test_support::git(&root, &["update-index", "--add", "--cacheinfo", &format!("160000,{},nested", oid.trim_end())]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "nested/file").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    service.shutdown().await;
}

#[tokio::test]
async fn refresh_retains_authority_and_reads_current_text_until_bounded_eviction() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree(); fs::write(root.join("file"), "before").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (first, files) = listing(&service, &entry, &clock).await; let id = &named(&files, "file").id;
    let (second, second_files) = listing(&service, &entry, &clock).await;
    fs::write(root.join("file"), "after\n").unwrap();
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &first, id)).await), "after\n");
    assert_eq!(service.review_repository_file(&entry, &second, id).await, RepositoryFileResult::StaleSelection);
    assert_eq!(service.review_repository_file(&entry, "../../secret", id).await, RepositoryFileResult::StaleSelection);
    assert_eq!(service.review_repository_file(&entry, &first, "../file").await, RepositoryFileResult::StaleSelection);
    for _ in 0..MAX_LISTINGS - 1 { listing(&service, &entry, &clock).await; }
    assert_eq!(service.review_repository_file(&entry, &first, id).await, RepositoryFileResult::StaleSelection);
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &second, &named(&second_files, "file").id)).await), "after\n");
    service.shutdown().await;
}

#[tokio::test]
async fn reselection_and_entry_changes_reject_old_tokens_without_obsolete_identity() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree(); fs::write(root.join("file"), "one").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    clock.finish(service.select(&entry)).await;
    assert_eq!(service.review_repository_file(&entry, &token, &files[0].id).await, RepositoryFileResult::StaleSelection);
    let (renewed, renewed_files) = listing(&service, &entry, &clock).await;
    let (_other_temp, other_root) = test_support::working_tree();
    let other = select(&service, &other_root, &clock).await;
    assert!(matches!(service.list_repository_files(&entry, RepositoryFilesRequest::default()).await, RepositoryFilesResult::StaleSelection));
    assert_eq!(service.review_repository_file(&other, &renewed, &renewed_files[0].id).await, RepositoryFileResult::StaleSelection);
    assert_eq!(service.review_repository_file(&entry, &renewed, &renewed_files[0].id).await, RepositoryFileResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn replaced_repository_root_cannot_reuse_listing_or_read_authority() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree();
    fs::write(root.join("file"), "original").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    fs::rename(&root, temp.path().join("original-root")).unwrap(); fs::create_dir(&root).unwrap();
    test_support::git(&root, &["init", "-b", "main"]); fs::write(root.join("file"), "replacement bytes").unwrap();
    assert!(matches!(clock.finish(service.list_repository_files(&entry, RepositoryFilesRequest::default())).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &files[0].id)).await,
        RepositoryFileResult::Unavailable { code: ReviewErrorCode::ChangedDuringRead });
    service.shutdown().await;
}

#[tokio::test]
async fn text_outcomes_distinguish_binary_encoding_empty_files_and_content_bounds() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::write(root.join("binary"), b"binary\0bytes").unwrap(); fs::write(root.join("encoding"), b"\xff").unwrap();
    fs::write(root.join("large"), vec![b'x'; diff::CONTENT_LIMIT + 1]).unwrap(); fs::write(root.join("empty"), "").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (token, files) = listing(&service, &entry, &clock).await;
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "empty").id)).await), "");
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "binary").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Binary });
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "encoding").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::UnsupportedEncoding });
    assert_eq!(clock.finish(service.review_repository_file(&entry, &token, &named(&files, "large").id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::LargeOrTruncated });
    service.shutdown().await;
}

#[test]
fn page_count_and_output_bounds_continue_without_losing_entries() {
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("one"), "1").unwrap(); fs::write(root.join("two"), "2").unwrap();
    let context = native_context(&root);
    let mut cursor = walk::DirectoryCursor::open(&context, PathBuf::new(), None, ProbeDeadline::new()).unwrap();
    let mut found = Vec::new();
    loop {
        let (entries, complete) = cursor.page(&context, PageLimits { entries: 1, ..Default::default() }, ProbeDeadline::new()).unwrap();
        assert!(entries.len() <= 1);
        found.extend(entries.into_iter().map(|entry| entry.file.display_path));
        if complete { break; }
    }
    found.sort_unstable();
    assert_eq!(found, ["one", "two"]);
    let mut cursor = walk::DirectoryCursor::open(&context, PathBuf::new(), None, ProbeDeadline::new()).unwrap();
    assert_eq!(cursor.page(&context, PageLimits { output_bytes: 520, ..Default::default() }, ProbeDeadline::new()).err(), Some(HistoryErrorCode::ResourceLimit));
    let mut cursor = walk::DirectoryCursor::open(&context, PathBuf::new(), None, ProbeDeadline::new()).unwrap();
    assert_eq!(cursor.page(&context, PageLimits { depth: 0, ..Default::default() }, ProbeDeadline::new()).err(), Some(HistoryErrorCode::ResourceLimit));
}

#[test]
fn serialized_page_boundary_preserves_the_overflow_entry_for_the_next_page() {
    let (_temp, root) = test_support::working_tree();
    fs::write(root.join("one"), "1").unwrap(); fs::write(root.join("two"), "2").unwrap();
    let context = native_context(&root);
    let mut cursor = walk::DirectoryCursor::open(&context, PathBuf::new(), None, ProbeDeadline::new()).unwrap();
    let limits = PageLimits { output_bytes: 640, ..Default::default() };
    let (first, complete) = cursor.page(&context, limits, ProbeDeadline::new()).unwrap();
    assert_eq!(first.len(), 1); assert!(!complete);
    let (second, complete) = cursor.page(&context, limits, ProbeDeadline::new()).unwrap();
    assert_eq!(second.len(), 1); assert!(complete);
    let mut paths = [first[0].file.display_path.as_str(), second[0].file.display_path.as_str()];
    paths.sort_unstable();
    assert_eq!(paths, ["one", "two"]);
}

#[tokio::test]
async fn asynchronous_listing_and_preview_completions_are_rejected_after_reselection() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree(); fs::write(root.join("file"), "inside").unwrap();
    let entered = temp.path().join("entered"); let release = temp.path().join("release"); let blocked = temp.path().join("blocked");
    let executable = test_support::executable(temp.path(), &format!(r#"
case "$*" in *"rev-parse --show-toplevel"*)
    if [ -f {blocked} ]; then
        : > {entered}
        while [ ! -f {release} ]; do sleep 0.01; done
    fi ;;
esac
exec git "$@"
"#, blocked = test_support::quote(&blocked), entered = test_support::quote(&entered), release = test_support::quote(&release)));
    let service = Arc::new(RepositoryService::new().with_inspection_executable(&executable));
    let entry = select(&service, &root, &clock).await;
    fs::write(&blocked, "").unwrap();
    let child = Arc::clone(&service); let child_entry = entry.clone();
    let request = tokio::spawn(async move { child.list_repository_files(&child_entry, RepositoryFilesRequest::default()).await });
    clock.wait_for_file(&entered).await; clock.finish(service.select(&entry)).await; fs::write(&release, "").unwrap();
    assert!(matches!(clock.finish(request).await.unwrap(), RepositoryFilesResult::StaleSelection));
    fs::remove_file(&blocked).unwrap(); fs::remove_file(&entered).unwrap(); fs::remove_file(&release).unwrap();
    let (token, files) = listing(&service, &entry, &clock).await;
    fs::write(&blocked, "").unwrap();
    let child = Arc::clone(&service); let child_entry = entry.clone(); let file = files[0].id.clone();
    let request = tokio::spawn(async move { child.review_repository_file(&child_entry, &token, &file).await });
    clock.wait_for_file(&entered).await; clock.finish(service.select(&entry)).await; fs::write(&release, "").unwrap();
    assert_eq!(clock.finish(request).await.unwrap(), RepositoryFileResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn bare_repositories_and_expired_deadlines_report_explicit_unavailability() {
    let clock = ManualClock::new(); let temp = tempfile::tempdir().unwrap(); let root = temp.path().join("bare.git");
    fs::create_dir(&root).unwrap(); test_support::git(&root, &["init", "--bare"]);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.list_repository_files(&entry, RepositoryFilesRequest::default())).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    service.shutdown().await;
    let (_working_temp, working) = test_support::working_tree();
    let context = native_context(&working); let deadline = ProbeDeadline::new();
    clock.advance(Duration::from_secs(31)).await;
    assert_eq!(walk::DirectoryCursor::open(&context, PathBuf::new(), None, deadline).err(), Some(HistoryErrorCode::Timeout));
}

#[tokio::test]
async fn browsing_diagnostics_keep_closed_outcomes_and_never_persist_paths_or_contents() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree();
    fs::write(root.join("private-browser-name"), "private-browser-content\n").unwrap();
    let database = temp.path().canonicalize().unwrap().join("capture/diagnostics.sqlite"); let store = DiagnosticStore::open(database.clone());
    let service = RepositoryService::with_diagnostics(store.sink()); let entry = select(&service, &root, &clock).await;
    let operation = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::ListRepositoryFiles);
    let (token, files) = operation.scope(listing(&service, &entry, &clock)).await;
    let review = operation.child().with_kind(OperationKind::ReviewRepositoryFile);
    let content = review.scope(clock.finish(service.review_repository_file(&entry, &token, &files[0].id))).await;
    assert_eq!(text(content), "private-browser-content\n");
    let forged = operation.child().with_kind(OperationKind::ReviewRepositoryFile);
    assert_eq!(forged.scope(service.review_repository_file(&entry, &token, "../../private-browser-name")).await, RepositoryFileResult::StaleSelection);
    store.flush(Duration::from_secs(5)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(forged.id()), ..Default::default() }).unwrap();
    assert!(rows.events.iter().any(|row| row.component == Component::Application && row.event == Event::Superseded && row.code.is_none()));
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(review.id()), ..Default::default() }).unwrap();
    assert!(rows.events.iter().any(|row| row.component == Component::Application && row.event == Event::Completed));
    assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::ReviewRepositoryFile));
    service.shutdown().await; store.shutdown(Duration::from_secs(5)).unwrap();
    let bytes = fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-browser-name".len()).any(|bytes| bytes == b"private-browser-name"));
    assert!(!bytes.windows(b"private-browser-content".len()).any(|bytes| bytes == b"private-browser-content"));
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn unsupported_native_filename_encoding_aborts_the_listing_instead_of_hiding_a_file() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::write(root.join(OsString::from_vec(b"bad-\xff".to_vec())), "unrepresentable path").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    assert!(matches!(clock.finish(service.list_repository_files(&entry, RepositoryFilesRequest::default())).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn root_page_issues_empty_and_ignored_directories_without_prewalking_descendants() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir(root.join("empty")).unwrap();
    populate_directory(&root.join("ignored"), PAGE_ENTRIES + 1, false);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let root_page = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    assert!(root_page.cursor.is_none());
    assert_eq!(root_page.files.len(), 1);
    assert_eq!(root_page.directories.len(), 2);
    let empty = page(&service, &entry, root_page.directory("empty"), &clock).await;
    assert!(empty.files.is_empty()); assert!(empty.directories.is_empty()); assert!(empty.cursor.is_none());
    let ignored = page(&service, &entry, root_page.directory("ignored"), &clock).await;
    assert!(!ignored.files.is_empty()); assert!(ignored.cursor.is_some());
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &ignored.listing_id, &ignored.files[0].id)).await), "working bytes\n");
    service.shutdown().await;
}

#[tokio::test]
async fn directory_and_cursor_authority_rejects_forgery_replay_and_cross_listing_or_directory_scope() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    populate_directory(&root.join("one"), PAGE_ENTRIES + 10, false);
    populate_directory(&root.join("two"), PAGE_ENTRIES + 10, false);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let root_page = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    let first = page(&service, &entry, root_page.directory("one"), &clock).await;
    let mut wrong_directory = root_page.directory("two"); wrong_directory.cursor = first.cursor.clone();
    assert!(matches!(clock.finish(service.list_repository_files(&entry, wrong_directory)).await, RepositoryFilesResult::StaleSelection));
    let refreshed = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    let mut wrong_listing = first.continuation(); wrong_listing.listing_id = Some(refreshed.listing_id);
    assert!(matches!(clock.finish(service.list_repository_files(&entry, wrong_listing)).await, RepositoryFilesResult::StaleSelection));
    let forged = RepositoryFilesRequest { listing_id: Some(first.listing_id.clone()), directory_id: Some("../one".into()), cursor: None };
    assert!(matches!(clock.finish(service.list_repository_files(&entry, forged)).await, RepositoryFilesResult::StaleSelection));
    let file_as_directory = RepositoryFilesRequest { listing_id: Some(first.listing_id.clone()), directory_id: Some(first.files[0].id.clone()), cursor: None };
    assert!(matches!(clock.finish(service.list_repository_files(&entry, file_as_directory)).await, RepositoryFilesResult::StaleSelection));
    let replay = first.continuation();
    let final_page = page(&service, &entry, first.continuation(), &clock).await;
    assert!(final_page.cursor.is_none());
    assert!(matches!(clock.finish(service.list_repository_files(&entry, replay)).await, RepositoryFilesResult::StaleSelection));
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &first.listing_id, &first.files[0].id)).await), "working bytes\n");
    clock.finish(service.select(&entry)).await;
    assert!(matches!(clock.finish(service.list_repository_files(&entry, root_page.directory("two"))).await, RepositoryFilesResult::StaleSelection));
    service.shutdown().await;
}

#[tokio::test]
async fn changed_directory_cursor_fails_explicitly_instead_of_marking_partial_results_complete() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    populate_directory(&root.join("wide"), PAGE_ENTRIES + 10, false);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let root_page = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    let first = page(&service, &entry, root_page.directory("wide"), &clock).await;
    fs::write(root.join("wide/new"), "arrived after first page").unwrap();
    assert!(matches!(clock.finish(service.list_repository_files(&entry, first.continuation())).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &first.listing_id, &first.files[0].id)).await), "working bytes\n");
    service.shutdown().await;
}

#[tokio::test]
async fn issued_directory_cannot_be_replaced_by_a_symlink_or_nested_repository() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree();
    fs::create_dir(root.join("linked")).unwrap(); fs::create_dir(root.join("nested")).unwrap();
    let outside = temp.path().join("outside"); fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret"), "outside").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let root_page = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    fs::remove_dir(root.join("linked")).unwrap(); symlink(&outside, root.join("linked")).unwrap();
    test_support::git(&root.join("nested"), &["init", "-b", "main"]);
    assert!(matches!(clock.finish(service.list_repository_files(&entry, root_page.directory("linked"))).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    assert!(matches!(clock.finish(service.list_repository_files(&entry, root_page.directory("nested"))).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::Inaccessible, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn single_large_directory_pages_beyond_old_file_traversal_output_and_git_index_limits() {
    const FILES: usize = 40_050;
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    populate_directory(&root.join("bulk"), FILES, true);
    test_support::commit(&root);
    assert!(fs::metadata(root.join(".git/index")).unwrap().len() > 1024 * 1024);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let root_page = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    assert!(root_page.files.is_empty()); assert_eq!(root_page.directories.len(), 1);
    let mut current = page(&service, &entry, root_page.directory("bulk"), &clock).await;
    assert!(current.cursor.is_some());
    let selected = current.files[0].clone();
    let listing_id = current.listing_id.clone();
    let mut paths = HashSet::new();
    let mut output_bytes = 0;
    loop {
        output_bytes += serde_json::to_vec(&current.files).unwrap().len();
        for file in &current.files { assert!(paths.insert(file.display_path.clone()), "a page duplicated a leaf"); }
        if current.cursor.is_none() { break; }
        current = page(&service, &entry, current.continuation(), &clock).await;
    }
    assert_eq!(paths.len(), FILES);
    assert!(output_bytes > 8 * 1024 * 1024);
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &listing_id, &selected.id)).await), "working bytes\n");
    page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    assert_eq!(text(clock.finish(service.review_repository_file(&entry, &listing_id, &selected.id)).await), "working bytes\n");
    service.shutdown().await;
}

#[tokio::test]
async fn deeply_nested_gitlinks_with_literal_glob_characters_are_not_hidden_by_ancestor_queries() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::create_dir_all(root.join("parent/[child]*?/leaf")).unwrap();
    fs::write(root.join("parent/[child]*?/leaf/file"), "formerly ordinary").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (listing_id, files) = listing(&service, &entry, &clock).await;
    let selected = named(&files, "parent/[child]*?/leaf/file");
    let oid = String::from_utf8(test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout).unwrap();
    test_support::git(&root, &["update-index", "--add", "--cacheinfo", &format!("160000,{},parent/[child]*?/leaf", oid.trim_end())]);
    assert_eq!(clock.finish(service.review_repository_file(&entry, &listing_id, &selected.id)).await,
        RepositoryFileResult::Unsupported { reason: UnsupportedReason::Submodule });
    let (_, fresh) = listing(&service, &entry, &clock).await;
    assert_eq!(fresh.iter().map(|file| file.display_path.as_str()).collect::<Vec<_>>(), ["parent/[child]*?/leaf"]);
    service.shutdown().await;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn invalid_encoding_in_an_unopened_descendant_does_not_block_the_first_root_page() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::create_dir(root.join("later")).unwrap();
    fs::write(root.join("visible"), "first usable file").unwrap();
    fs::write(root.join("later").join(OsString::from_vec(b"bad-\xff".to_vec())), "not hidden").unwrap();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let first = page(&service, &entry, RepositoryFilesRequest::default(), &clock).await;
    assert_eq!(first.files[0].display_path, "visible");
    assert!(matches!(clock.finish(service.list_repository_files(&entry, first.directory("later"))).await,
        RepositoryFilesResult::Unavailable { code: HistoryErrorCode::InvalidOutput, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn targeted_gitlink_queries_override_process_literals_without_matching_descendants_or_glob_neighbors() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    fs::create_dir_all(root.join("ordinary/nested")).unwrap();
    fs::write(root.join("ordinary/nested/file"), "tracked descendant").unwrap();
    fs::write(root.join("literal-neighbor"), "tracked neighbor").unwrap();
    test_support::commit(&root);
    let oid = String::from_utf8(test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout).unwrap();
    for path in ["module", "λ", r"literal[*]?\folder", ":(glob)*"] {
        fs::create_dir(root.join(path)).unwrap();
        test_support::git(&root, &["update-index", "--add", "--cacheinfo", &format!("160000,{},{}", oid.trim_end(), path)]);
    }
    let targets = ["ordinary", "module", "λ", r"literal[*]?\folder", ":(glob)*"].map(PathBuf::from);
    let links = clock.finish(gitlinks(&GitProcess::default(), &native_context(&root), &targets, ProbeDeadline::new())).await.unwrap();
    assert_eq!(links, HashSet::from([
        PathBuf::from("module"), PathBuf::from("λ"), PathBuf::from(r"literal[*]?\folder"), PathBuf::from(":(glob)*"),
    ]));
}
