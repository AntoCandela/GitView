//! Exercises read-only status snapshots through isolated real repositories.

use std::fs;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

use super::*;
use crate::test_support::{commit, git, git_output, working_tree};
#[cfg(unix)]
use crate::test_support::{executable, quote};

fn path<'a>(paths: &'a [StatusPath], name: &str) -> &'a StatusPath {
    paths.iter().find(|path| path.display_path == name).unwrap()
}

#[tokio::test]
async fn private_index_preserves_racy_same_size_worktree_edits() {
    let (_temp, root) = working_tree();
    let file = root.join("file");
    fs::write(&file, b"initial\n").unwrap();
    commit(&root);
    git(&root, &["config", "core.checkstat", "minimal"]);
    git(&root, &["config", "core.trustctime", "false"]);
    let timestamp = std::time::SystemTime::now() - std::time::Duration::from_secs(5);
    let times = fs::FileTimes::new().set_modified(timestamp);
    fs::File::options().write(true).open(&file).unwrap().set_times(times).unwrap();
    git(&root, &["update-index", "--refresh"]);
    fs::File::options().write(true).open(root.join(".git/index")).unwrap().set_times(times).unwrap();
    let index = fs::read(root.join(".git/index")).unwrap();
    fs::write(&file, b"changed\n").unwrap();
    fs::File::options().write(true).open(&file).unwrap().set_times(times).unwrap();

    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].display_path, "file");
    assert_eq!(paths[0].unstaged, Some(ChangeKind::Modified));
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}

#[tokio::test]
async fn large_tracked_index_with_few_changes_is_read_without_mutating_source_metadata() {
    let (_temp, root) = working_tree();
    git(&root, &["config", "index.version", "2"]);
    let prefix = "tracked".repeat(16);
    for index in 0..7000 {
        fs::write(root.join(format!("{prefix}-{index:04}")), b"baseline\n").unwrap();
    }
    commit(&root);
    let index = fs::read(root.join(".git/index")).unwrap();
    assert!(index.len() > 1024 * 1024);
    assert!(git_output(&root, &["ls-files", "--stage", "-z"]).stdout.len() > 1024 * 1024);
    let reader = GitStatusReader::default();
    assert_eq!(reader.read(&root).await.unwrap(), []);
    let changed = format!("{prefix}-3000");
    fs::write(root.join(&changed), b"externally edited\n").unwrap();
    let paths = reader.read(&root).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].display_path, changed);
    assert_eq!(paths[0].unstaged, Some(ChangeKind::Modified));
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
}

#[tokio::test]
async fn larger_index_budget_does_not_allow_oversized_attribute_metadata() {
    let (_temp, root) = working_tree();
    fs::File::create(root.join(".git/info/attributes")).unwrap().set_len(1024 * 1024 + 1).unwrap();
    assert_eq!(GitStatusReader::default().read(&root).await, Err(StatusError::ResourceLimit));
}

#[tokio::test]
async fn isolated_status_preserves_tracked_unicode_filename_identity() {
    let (_temp, root) = working_tree();
    git(&root, &["config", "core.precomposeunicode", "true"]);
    fs::write(root.join("café.txt"), b"initial\n").unwrap();
    commit(&root);
    let reader = GitStatusReader::default();
    assert_eq!(reader.read(&root).await.unwrap(), []);
    fs::write(root.join("café.txt"), b"changed\n").unwrap();
    let paths = reader.read(&root).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].native_path, std::path::PathBuf::from("café.txt"));
    assert_eq!(paths[0].staged, None);
    assert_eq!(paths[0].unstaged, Some(ChangeKind::Modified));
}

#[tokio::test]
async fn real_git_clean_partial_added_deleted_and_nested_untracked_are_read_exactly() {
    let (_temp, root) = working_tree();
    let reader = GitStatusReader::default();
    assert_eq!(reader.read(&root).await.unwrap(), []);
    fs::write(root.join("partial"), b"initial\n").unwrap();
    fs::write(root.join("deleted"), b"initial\n").unwrap();
    commit(&root);
    fs::write(root.join("partial"), b"staged\n").unwrap();
    fs::write(root.join("added"), b"new\n").unwrap();
    git(&root, &["add", "partial", "added"]);
    fs::write(root.join("partial"), b"unstaged\n").unwrap();
    fs::remove_file(root.join("deleted")).unwrap();
    fs::create_dir(root.join("nested")).unwrap();
    #[cfg(unix)]
    let untracked = "space name\né.txt";
    #[cfg(not(unix))]
    let untracked = "space name é.txt";
    fs::write(root.join("nested").join(untracked), b"untracked\n").unwrap();
    let paths = reader.read(&root).await.unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(path(&paths, "partial").staged, Some(ChangeKind::Modified));
    assert_eq!(path(&paths, "partial").unstaged, Some(ChangeKind::Modified));
    assert_eq!(path(&paths, "added").staged, Some(ChangeKind::Added));
    assert_eq!(path(&paths, "deleted").unstaged, Some(ChangeKind::Deleted));
    assert_eq!(path(&paths, &format!("nested/{untracked}")).segments, ["nested", untracked]);
}

#[cfg(unix)]
#[tokio::test]
async fn real_git_rename_preserves_the_original_native_path() {
    let (_temp, root) = working_tree();
    fs::write(root.join("old name"), b"content\n").unwrap();
    commit(&root);
    git(&root, &["mv", "old name", "new\nname"]);
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].native_path, PathBuf::from("new\nname"));
    assert_eq!(paths[0].native_origin, Some(PathBuf::from("old name")));
    assert_eq!(paths[0].unsupported_kind, Some(UnsupportedKind::RenameOrCopy));
}

#[tokio::test]
async fn real_git_merge_conflict_remains_a_conflict_only() {
    let (_temp, root) = working_tree();
    fs::write(root.join("conflict"), b"initial\n").unwrap();
    commit(&root);
    git(&root, &["checkout", "-b", "other"]);
    fs::write(root.join("conflict"), b"other\n").unwrap();
    commit(&root);
    git(&root, &["checkout", "main"]);
    fs::write(root.join("conflict"), b"main\n").unwrap();
    commit(&root);
    let merge = git_output(&root, &["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org", "merge", "other"]);
    assert!(!merge.status.success());
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].conflict);
    assert_eq!(paths[0].staged, None);
    assert_eq!(paths[0].unstaged, None);
}

#[cfg(unix)]
#[tokio::test]
async fn real_git_submodules_remain_explicit_without_recursive_worktree_scanning() {
    let (_source_temp, source) = working_tree();
    fs::write(source.join("file"), b"initial\n").unwrap();
    fs::write(source.join(".gitattributes"), b"file filter=side_effect\n").unwrap();
    commit(&source);
    let (temp, root) = working_tree();
    git(&root, &["-c", "protocol.file.allow=always", "submodule", "add", source.to_str().unwrap(), "module"]);
    fs::write(root.join("ordinary"), b"initial\n").unwrap();
    commit(&root);
    let clean = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(path(&clean, "module").unsupported_kind, Some(UnsupportedKind::Submodule));
    fs::write(root.join("ordinary"), b"changed\n").unwrap();
    let marker = temp.path().join("submodule-filter-ran");
    let filter = format!("touch {}; cat", quote(&marker));
    git(&root.join("module"), &["config", "filter.side_effect.process", &filter]);
    fs::write(root.join("module/file"), b"changed\n").unwrap();
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(path(&paths, "module").unsupported_kind, Some(UnsupportedKind::Submodule));
    assert_eq!(path(&paths, "ordinary").unstaged, Some(ChangeKind::Modified));
    assert!(!marker.exists());
}

#[cfg(unix)]
#[tokio::test]
async fn real_git_type_changes_remain_visible_without_submodules() {
    use std::os::unix::fs::symlink;
    let (_temp, root) = working_tree();
    fs::write(root.join("link"), b"regular\n").unwrap();
    commit(&root);
    fs::remove_file(root.join("link")).unwrap();
    symlink("target", root.join("link")).unwrap();
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(path(&paths, "link").unsupported_kind, Some(UnsupportedKind::TypeChange));
}

// macOS filesystems reject invalid UTF-8 filenames before Git can enumerate them.
#[cfg(all(unix, not(target_os = "macos")))]
#[tokio::test]
async fn real_git_non_unicode_path_never_becomes_a_display_alias() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let (_temp, root) = working_tree();
    fs::write(root.join(OsStr::from_bytes(b"invalid-\xff")), b"file\n").unwrap();
    assert_eq!(GitStatusReader::default().read(&root).await, Err(StatusError::UnsupportedPathEncoding));
}

#[cfg(unix)]
#[tokio::test]
async fn status_suppresses_fsmonitor_and_preserves_index_head_refs_and_worktree() {
    let (temp, root) = working_tree();
    fs::write(root.join("tracked"), b"initial\n").unwrap();
    commit(&root);
    fs::write(root.join("tracked"), b"changed\n").unwrap();
    let marker = temp.path().join("fsmonitor-ran");
    let hook = executable(temp.path(), &format!("touch {}\nexit 1", quote(&marker)));
    git(&root, &["config", "core.fsmonitor", hook.to_str().unwrap()]);
    let index = fs::read(root.join(".git/index")).unwrap();
    let head = fs::read(root.join(".git/HEAD")).unwrap();
    let reference = fs::read(root.join(".git/refs/heads/main")).unwrap();
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(path(&paths, "tracked").unstaged, Some(ChangeKind::Modified));
    assert!(!marker.exists());
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(root.join(".git/HEAD")).unwrap(), head);
    assert_eq!(fs::read(root.join(".git/refs/heads/main")).unwrap(), reference);
    assert_eq!(fs::read(root.join("tracked")).unwrap(), b"changed\n");
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_status_never_executes_clean_or_process_filters() {
    for operation in ["clean", "process"] {
        let (temp, root) = working_tree();
        fs::write(root.join(".gitattributes"), b"tracked filter=side_effect\n").unwrap();
        fs::write(root.join("tracked"), b"initial\n").unwrap();
        commit(&root);
        let marker = temp.path().join("filter-ran");
        let filter = format!("touch {}; cat", quote(&marker));
        git(&root, &["config", &format!("filter.side_effect.{operation}"), &filter]);
        // Equal-size edits force Git to hash content rather than decide from size alone.
        fs::write(root.join("tracked"), b"changed\n").unwrap();

        let result = GitStatusReader::default().read(&root).await;

        assert!(!marker.exists(), "automatic status executed the {operation} filter");
        assert_eq!(result, Err(StatusError::UnsupportedConfiguration));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn new_filter_configuration_after_preflight_cannot_execute_during_status() {
    let (temp, root) = working_tree();
    fs::write(root.join(".gitattributes"), b"tracked filter=raced\n").unwrap();
    fs::write(root.join("tracked"), b"initial\n").unwrap();
    commit(&root);
    fs::write(root.join("tracked"), b"changed\n").unwrap();
    let marker = temp.path().join("raced-filter-ran");
    let configured = temp.path().join("configured");
    let filter = format!("touch {}; cat", quote(&marker));
    let wrapper = executable(temp.path(), &format!(r#"
if [ "$*" = '--no-optional-locks -c core.fsmonitor=false config --null --list' ] && [ ! -f {configured} ]; then
    git "$@" || exit $?
    git config filter.raced.clean "{filter}"
    touch {configured}
    exit 0
fi
exec git "$@"
"#, configured = quote(&configured)));
    let result = GitStatusReader::with_executable(&wrapper).read(&root).await;
    assert!(configured.exists());
    assert!(!marker.exists());
    assert_eq!(result, Err(StatusError::UnsupportedConfiguration));
}

#[tokio::test]
async fn unsupported_index_layouts_and_flags_never_produce_clean_snapshots() {
    for layout in ["split", "sparse", "assume_unchanged", "skip_worktree"] {
        let (_temp, root) = working_tree();
        fs::write(root.join("tracked"), b"initial\n").unwrap();
        commit(&root);
        match layout {
            "split" => git(&root, &["update-index", "--split-index"]),
            "sparse" => git(&root, &["config", "core.sparseCheckout", "true"]),
            "assume_unchanged" => git(&root, &["update-index", "--assume-unchanged", "tracked"]),
            "skip_worktree" => git(&root, &["update-index", "--skip-worktree", "tracked"]),
            _ => unreachable!(),
        }
        fs::write(root.join("tracked"), b"changed\n").unwrap();
        assert_eq!(GitStatusReader::default().read(&root).await, Err(StatusError::UnsupportedConfiguration), "{layout}");
    }
}

#[tokio::test]
async fn ordinary_attribute_and_line_ending_normalization_remains_accurate() {
    let (_temp, root) = working_tree();
    git(&root, &["config", "core.autocrlf", "true"]);
    fs::write(root.join("tracked"), b"initial\r\n").unwrap();
    fs::write(root.join(".git/info/attributes"), b"tracked text eol=crlf\n").unwrap();
    commit(&root);
    fs::write(root.join(".git/info/exclude"), b"ignored\n").unwrap();
    fs::write(root.join("ignored"), b"ignored content\n").unwrap();
    assert_eq!(GitStatusReader::default().read(&root).await.unwrap(), []);
    fs::write(root.join("tracked"), b"changed\r\n").unwrap();
    let paths = GitStatusReader::default().read(&root).await.unwrap();
    assert_eq!(path(&paths, "tracked").unstaged, Some(ChangeKind::Modified));
    assert!(!paths.iter().any(|path| path.display_path == "ignored"));
}

#[cfg(unix)]
#[tokio::test]
async fn filters_from_included_configuration_are_unavailable_without_execution() {
    let (temp, root) = working_tree();
    fs::write(root.join(".gitattributes"), b"tracked filter=side_effect\n").unwrap();
    fs::write(root.join("tracked"), b"initial\n").unwrap();
    commit(&root);
    let marker = temp.path().join("included-filter-ran");
    let included = temp.path().join("included-config");
    let filter = format!("touch {}; cat", quote(&marker));
    git(&root, &["config", "--file", included.to_str().unwrap(), "filter.side_effect.clean", &filter]);
    git(&root, &["config", "include.path", included.to_str().unwrap()]);
    fs::write(root.join("tracked"), b"changed\n").unwrap();
    let result = GitStatusReader::default().read(&root).await;
    assert!(!marker.exists());
    assert_eq!(result, Err(StatusError::UnsupportedConfiguration));
}

#[tokio::test]
async fn missing_root_and_missing_git_are_distinct_failures() {
    let (temp, root) = working_tree();
    assert_eq!(GitStatusReader::default().read(&root.join("missing")).await, Err(StatusError::Inaccessible));
    let reader = GitStatusReader { process: GitProcess::with_executable(&temp.path().join("missing-git")) };
    assert_eq!(reader.read(&root).await, Err(StatusError::GitUnavailable));
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_repository_and_git_failure_never_return_clean() {
    let (temp, root) = working_tree();
    let unsafe_git = executable(temp.path(), "printf '%s' 'fatal: detected dubious ownership' >&2\nexit 128");
    let reader = GitStatusReader { process: GitProcess::with_executable(&unsafe_git) };
    assert_eq!(reader.read(&root).await, Err(StatusError::UnsafeRepository));
    let failed_git = executable(temp.path(), "printf '%s' 'fatal: permission denied' >&2\nexit 128");
    let reader = GitStatusReader::with_executable(&failed_git);
    assert_eq!(reader.read(&root).await, Err(StatusError::Inaccessible));
}

#[cfg(unix)]
#[tokio::test]
async fn process_timeout_and_output_limit_are_unavailable_not_clean() {
    let (temp, root) = working_tree();
    let slow_git = executable(temp.path(), "exec sleep 30");
    let reader = GitStatusReader { process: GitProcess::with_executable(&slow_git) };
    assert_eq!(reader.read_with_deadline(&root, ProbeDeadline::after(Duration::from_millis(20))).await, Err(StatusError::Timeout));
    let overflow_git = executable(temp.path(), "exec dd if=/dev/zero bs=1048577 count=1");
    let reader = GitStatusReader::with_executable(&overflow_git);
    assert_eq!(reader.read(&root).await, Err(StatusError::ResourceLimit));
}
