//! Exercises production historical previews with immutable Git objects and generation-bound authority.

use super::*;
use crate::{application::RepositoryService, diff::LineKind, inspection::CommittedFile, test_support::{self, ManualClock}, workspace::OpenOutcome};
use std::{fs, path::Path, sync::Arc};

fn head(root: &Path) -> String { String::from_utf8(test_support::git_output(root, &["rev-parse", "HEAD"]).stdout).unwrap().trim_end().to_owned() }
async fn select(service: &RepositoryService, root: &Path, clock: &ManualClock) -> String {
    let OpenOutcome::Opened { entry_id, .. } = clock.finish(service.open_chosen(root)).await else { panic!("fixture must open") };
    clock.finish(service.select(&entry_id)).await;
    entry_id
}
async fn files(service: &RepositoryService, entry: &str, commit: &str, parent: Option<&str>, clock: &ManualClock) -> (Option<String>, Vec<CommittedFile>) {
    match clock.finish(service.commit_files(entry, commit, parent)).await {
        CommitFilesResult::Files { parent_oid, files, .. } => (parent_oid, files),
        other => panic!("expected committed files: {other:?}"),
    }
}
fn text(result: CommitReviewResult) -> (CommitReviewIdentity, Vec<(LineKind, String)>, String, String) {
    match result {
        CommitReviewResult::Text { identity, hunks, from_content, to_content } => (identity, hunks.into_iter().flat_map(|hunk| hunk.lines).map(|line| (line.kind, line.text)).collect(), from_content, to_content),
        other => panic!("expected committed text: {other:?}"),
    }
}

#[tokio::test]
async fn root_add_modify_delete_and_empty_files_use_committed_bytes_without_git_mutation() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::unborn_working_tree();
    let literal = ":(glob)* [bracket]\tquote\"\nfile";
    fs::write(root.join(literal), "before\n").unwrap();
    fs::write(root.join("deleted"), "gone\n").unwrap();
    test_support::commit(&root);
    let initial = head(&root);
    fs::write(root.join(literal), "committed\n").unwrap();
    fs::remove_file(root.join("deleted")).unwrap();
    fs::write(root.join("empty"), "").unwrap();
    fs::write(root.join("added"), "added\n").unwrap();
    test_support::commit(&root);
    let next = head(&root);
    fs::write(root.join(literal), "index bytes\n").unwrap();
    test_support::git(&root, &["--literal-pathspecs", "add", "--", literal]);
    fs::write(root.join(literal), "working bytes\n").unwrap();
    fs::write(root.join("deleted"), "resurrected working bytes\n").unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    let git_head = fs::read(root.join(".git/HEAD")).unwrap();
    let branch = fs::read(root.join(".git/refs/heads/main")).unwrap();
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (parent, root_files) = files(&service, &entry, &initial, None, &clock).await;
    assert_eq!(parent, None);
    let initial_file = root_files.iter().find(|file| file.display_path == literal).unwrap();
    let (identity, lines, from_content, to_content) = text(clock.finish(service.review_commit_file(&entry, &initial, None, &initial_file.id)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("", "before\n"));
    assert!(identity.from_absent && !identity.to_absent);
    assert_eq!(lines, [(LineKind::Addition, "before".into())]);
    let (parent, next_files) = files(&service, &entry, &next, None, &clock).await;
    assert_eq!(parent.as_deref(), Some(initial.as_str()));
    for file in next_files {
        let (identity, lines, from_content, to_content) = text(clock.finish(service.review_commit_file(&entry, &next, parent.as_deref(), &file.id)).await);
        assert_eq!(identity.file_id, file.id);
        assert_eq!(identity.parent_oid.as_deref(), Some(initial.as_str()));
        match file.display_path.as_str() {
            "added" => { assert!(identity.from_absent && !identity.to_absent); assert_eq!((from_content.as_str(), to_content.as_str()), ("", "added\n")); assert_eq!(lines, [(LineKind::Addition, "added".into())]); }
            "deleted" => { assert!(!identity.from_absent && identity.to_absent); assert_eq!((from_content.as_str(), to_content.as_str()), ("gone\n", "")); assert_eq!(lines, [(LineKind::Removal, "gone".into())]); }
            "empty" => { assert!(identity.from_absent && !identity.to_absent); assert_eq!((from_content.as_str(), to_content.as_str()), ("", "")); assert!(lines.is_empty()); }
            path if path == literal => { assert!(!identity.from_absent && !identity.to_absent); assert_eq!((from_content.as_str(), to_content.as_str()), ("before\n", "committed\n")); assert_eq!(lines, [(LineKind::Removal, "before".into()), (LineKind::Addition, "committed".into())]); }
            other => panic!("unexpected fixture path {other}"),
        }
    }
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), git_head);
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), branch);
    assert_eq!(fs::read_to_string(root.join(literal)).unwrap(), "working bytes\n");
    service.shutdown().await;
}

#[tokio::test]
async fn raw_merge_parents_authorize_distinct_previews_and_reject_forged_scope() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let base = head(&root);
    fs::write(root.join("file"), "left\n").unwrap(); test_support::commit(&root); let left = head(&root);
    fs::write(root.join("file"), "right\n").unwrap(); test_support::commit(&root); let right = head(&root);
    fs::write(root.join("file"), "merge\n").unwrap(); test_support::commit(&root);
    let output = test_support::git_output(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "commit-tree", "HEAD^{tree}", "-p", &left, "-p", &right, "-m", "merge"]);
    assert!(output.status.success()); let merge = String::from_utf8(output.stdout).unwrap().trim_end().to_owned();
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (parent, first) = files(&service, &entry, &merge, None, &clock).await;
    assert_eq!(parent.as_deref(), Some(left.as_str()));
    let (_, second) = files(&service, &entry, &merge, Some(&right), &clock).await;
    for (parent, file, removed) in [(&left, &first[0], "left"), (&right, &second[0], "right")] {
        let (_, lines, from_content, to_content) = text(clock.finish(service.review_commit_file(&entry, &merge, Some(parent), &file.id)).await);
        assert_eq!((from_content.as_str(), to_content.as_str()), (format!("{removed}\n").as_str(), "merge\n"));
        assert_eq!(lines, [(LineKind::Removal, removed.into()), (LineKind::Addition, "merge".into())]);
    }
    for (entry_id, commit, parent, id) in [
        (entry.as_str(), merge.as_str(), Some(right.as_str()), first[0].id.as_str()),
        (entry.as_str(), merge.as_str(), None, first[0].id.as_str()),
        (entry.as_str(), base.as_str(), Some(left.as_str()), first[0].id.as_str()),
        ("forged-entry", merge.as_str(), Some(left.as_str()), first[0].id.as_str()),
        (entry.as_str(), merge.as_str(), Some(left.as_str()), "file"),
        (entry.as_str(), merge.as_str(), Some(left.as_str()), "../../secret"),
    ] { assert_eq!(service.review_commit_file(entry_id, commit, parent, id).await, CommitReviewResult::StaleSelection); }
    clock.finish(service.select(&entry)).await;
    assert_eq!(service.review_commit_file(&entry, &merge, Some(&left), &first[0].id).await, CommitReviewResult::StaleSelection);
    let (_, renewed) = files(&service, &entry, &merge, Some(&left), &clock).await;
    assert_ne!(renewed[0].id, first[0].id);
    let (_other_temp, other_root) = test_support::working_tree();
    let other_entry = select(&service, &other_root, &clock).await;
    assert_eq!(service.review_commit_file(&other_entry, &merge, Some(&left), &renewed[0].id).await, CommitReviewResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn binary_encoding_oversize_symlink_type_change_and_submodule_are_explicit() {
    use std::os::unix::fs::symlink;
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree(); let base = head(&root);
    fs::write(root.join("type"), "regular\n").unwrap(); test_support::commit(&root);
    fs::remove_file(root.join("type")).unwrap(); symlink("outside", root.join("type")).unwrap();
    symlink("outside", root.join("symlink")).unwrap();
    fs::write(root.join("binary"), b"a\0b").unwrap();
    fs::write(root.join("encoding"), b"\xff").unwrap();
    fs::write(root.join("oversize"), vec![b'a'; diff::CONTENT_LIMIT + 1]).unwrap();
    test_support::git(&root, &["add", "--all"]);
    test_support::git(&root, &["update-index", "--add", "--cacheinfo", &format!("160000,{base},submodule")]);
    test_support::git(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "commit", "-m", "unsupported"]);
    let commit = head(&root); let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (parent, listed) = files(&service, &entry, &commit, None, &clock).await;
    for file in listed {
        let expected = match file.display_path.as_str() {
            "binary" => UnsupportedReason::Binary, "encoding" => UnsupportedReason::UnsupportedEncoding,
            "oversize" => UnsupportedReason::LargeOrTruncated, "symlink" | "type" => UnsupportedReason::TypeChange,
            "submodule" => UnsupportedReason::Submodule, other => panic!("unexpected {other}"),
        };
        let result = clock.finish(service.review_commit_file(&entry, &commit, parent.as_deref(), &file.id)).await;
        assert!(matches!(result, CommitReviewResult::Unsupported { reason, .. } if reason == expected));
    }
    service.shutdown().await;
}

#[tokio::test]
async fn duplicate_expansion_retains_ids_and_bounded_eviction_revokes_old_authority() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::working_tree();
    let mut commits = Vec::new();
    for index in 0..=MAX_COMPARISONS { fs::write(root.join("file"), format!("version {index}\n")).unwrap(); test_support::commit(&root); commits.push(head(&root)); }
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (parent, first) = files(&service, &entry, &commits[0], None, &clock).await;
    for _ in 0..12 {
        let (_, repeated) = files(&service, &entry, &commits[0], None, &clock).await;
        assert_eq!(repeated[0].id, first[0].id);
    }
    for commit in &commits[1..] { files(&service, &entry, commit, None, &clock).await; }
    assert_eq!(service.review_commit_file(&entry, &commits[0], parent.as_deref(), &first[0].id).await, CommitReviewResult::StaleSelection);
    let (parent, latest) = files(&service, &entry, commits.last().unwrap(), None, &clock).await;
    let (_, lines, ..) = text(clock.finish(service.review_commit_file(&entry, commits.last().unwrap(), parent.as_deref(), &latest[0].id)).await);
    assert_eq!(lines, [(LineKind::Removal, format!("version {}", MAX_COMPARISONS - 1)), (LineKind::Addition, format!("version {MAX_COMPARISONS}"))]);
    service.shutdown().await;
}

#[tokio::test]
async fn completed_object_payload_is_discarded_after_same_entry_reselection() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree();
    fs::write(root.join("file"), "committed\n").unwrap(); test_support::commit(&root); let commit = head(&root);
    let gate = test_support::executable(temp.path(), &format!(r#"
case "$*" in
  *" cat-file blob "*)
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
    let (parent, listed) = files(&service, &entry, &commit, None, &clock).await;
    fs::write(temp.path().join("block"), "").unwrap();
    let child = Arc::clone(&service); let child_entry = entry.clone(); let id = listed[0].id.clone();
    let request = tokio::spawn(async move { child.review_commit_file(&child_entry, &commit, parent.as_deref(), &id).await });
    clock.wait_for_file(&temp.path().join("entered")).await;
    clock.finish(service.select(&entry)).await;
    fs::write(temp.path().join("release"), "").unwrap();
    assert_eq!(clock.finish(request).await.unwrap(), CommitReviewResult::StaleSelection);
    service.shutdown().await;
}

#[tokio::test]
async fn status_unavailable_and_bare_contexts_do_not_block_historical_reads() {
    let clock = ManualClock::new(); let (temp, root) = test_support::working_tree();
    fs::write(root.join("file"), "historical\n").unwrap(); test_support::commit(&root); let commit = head(&root);
    test_support::git(&root, &["config", "core.sparseCheckout", "true"]);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    clock.finish(async {
        loop { if matches!(service.observe_selected_context(&entry).await, crate::observation::ObservationSnapshot::Unavailable { .. }) { break; } tokio::task::yield_now().await; }
    }).await;
    let (parent, listed) = files(&service, &entry, &commit, None, &clock).await;
    let (_, lines, ..) = text(clock.finish(service.review_commit_file(&entry, &commit, parent.as_deref(), &listed[0].id)).await);
    assert_eq!(lines, [(LineKind::Addition, "historical".into())]);
    let bare = temp.path().join("bare.git"); test_support::git(temp.path(), &["clone", "--bare", root.to_str().unwrap(), bare.to_str().unwrap()]);
    let bare_entry = select(&service, &bare, &clock).await;
    let (parent, listed) = files(&service, &bare_entry, &commit, None, &clock).await;
    let (_, lines, ..) = text(clock.finish(service.review_commit_file(&bare_entry, &commit, parent.as_deref(), &listed[0].id)).await);
    assert_eq!(lines, [(LineKind::Addition, "historical".into())]);
    service.shutdown().await;
}

#[tokio::test]
async fn replacement_refs_cannot_change_pinned_bytes_and_missing_objects_are_unavailable() {
    let clock = ManualClock::new(); let (_temp, root) = test_support::unborn_working_tree();
    fs::write(root.join("file"), "original\n").unwrap(); test_support::commit(&root); let original = head(&root);
    let blob = String::from_utf8(test_support::git_output(&root, &["rev-parse", &format!("{original}:file")]).stdout).unwrap().trim_end().to_owned();
    fs::write(root.join("file"), "replacement\n").unwrap(); test_support::commit(&root); let replacement = head(&root);
    test_support::git(&root, &["replace", &original, &replacement]);
    let service = RepositoryService::new(); let entry = select(&service, &root, &clock).await;
    let (parent, listed) = files(&service, &entry, &original, None, &clock).await;
    assert_eq!(parent, None);
    let (identity, lines, from_content, to_content) = text(clock.finish(service.review_commit_file(&entry, &original, None, &listed[0].id)).await);
    assert_eq!((from_content.as_str(), to_content.as_str()), ("", "original\n"));
    assert!(identity.from_absent);
    assert_eq!(lines, [(LineKind::Addition, "original".into())]);
    fs::remove_file(root.join(".git/objects").join(&blob[..2]).join(&blob[2..])).unwrap();
    assert!(matches!(clock.finish(service.review_commit_file(&entry, &original, None, &listed[0].id)).await,
        CommitReviewResult::Unavailable { code: ReviewErrorCode::Inaccessible, .. }));
    service.shutdown().await;
}

#[tokio::test]
async fn full_context_and_hunks_stay_pinned_after_head_index_and_worktree_advance() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let original = format!("é — pinned prefix\r\n{}tail without newline", (2..=100).map(|line| format!("original line {line}\n")).collect::<String>());
    fs::write(root.join("file"), &original).unwrap();
    test_support::commit(&root);
    let before = head(&root);
    let committed = original.replace("original line 15\n", "committed edit\n");
    fs::write(root.join("file"), &committed).unwrap();
    test_support::commit(&root);
    let commit = head(&root);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (parent, listed) = files(&service, &entry, &commit, None, &clock).await;
    assert_eq!(parent.as_deref(), Some(before.as_str()));
    fs::write(root.join("file"), "later HEAD\n").unwrap();
    test_support::commit(&root);
    fs::write(root.join("file"), "later index\n").unwrap();
    test_support::git(&root, &["add", "--", "file"]);
    fs::write(root.join("file"), "later working bytes\n").unwrap();
    let result = clock.finish(service.review_commit_file(&entry, &commit, parent.as_deref(), &listed[0].id)).await;
    let serialized = serde_json::to_value(&result).unwrap();
    assert_eq!(serialized["fromContent"], original);
    assert_eq!(serialized["toContent"], committed);
    let CommitReviewResult::Text { identity, hunks, from_content, to_content } = result else { panic!("expected pinned text") };
    assert_eq!(identity.commit_oid, commit);
    assert_eq!(identity.parent_oid, parent);
    assert_eq!(from_content, original);
    assert_eq!(to_content, committed);
    assert_eq!((hunks[0].old_start, hunks[0].new_start), (12, 12));
    assert!(hunks[0].lines.iter().any(|line| line.kind == LineKind::Removal && line.text == "original line 15"));
    assert!(hunks[0].lines.iter().any(|line| line.kind == LineKind::Addition && line.text == "committed edit"));
    assert!(!hunks[0].lines.iter().any(|line| line.text.contains("pinned prefix") || line.text == "tail without newline"));
    service.shutdown().await;
}

#[tokio::test]
async fn oversized_full_context_is_rejected_for_either_pinned_endpoint() {
    let clock = ManualClock::new();
    let (_temp, root) = test_support::working_tree();
    let at_limit = "\n".repeat(32_768);
    let over_limit = format!("{at_limit}tail");
    fs::write(root.join("over-from"), &over_limit).unwrap();
    fs::write(root.join("over-to"), &at_limit).unwrap();
    test_support::commit(&root);
    fs::write(root.join("over-from"), &at_limit).unwrap();
    fs::write(root.join("over-to"), &over_limit).unwrap();
    test_support::commit(&root);
    let commit = head(&root);
    let service = RepositoryService::new();
    let entry = select(&service, &root, &clock).await;
    let (parent, listed) = files(&service, &entry, &commit, None, &clock).await;
    for file in listed {
        assert!(matches!(clock.finish(service.review_commit_file(&entry, &commit, parent.as_deref(), &file.id)).await,
            CommitReviewResult::Unsupported { reason: UnsupportedReason::LargeOrTruncated, .. }));
    }
    service.shutdown().await;
}
