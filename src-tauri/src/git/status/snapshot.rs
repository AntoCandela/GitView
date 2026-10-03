//! Isolates status from mutable filter configuration and recursive submodule processes.
//!
//! Only bounded index/attribute metadata is copied; objects remain read-only alternates.
//! The private repository never loads source includes, local configuration or global filters.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::{is_unsafe, process_error, status_path, StatusError, StatusPath, UnsupportedKind, STATUS_PATH_LIMIT};
use crate::git::process::{GitProcess, ProbeDeadline, ProcessOutput};

const METADATA_LIMIT: u64 = 1024 * 1024;
const INDEX_LIMIT: u64 = 64 * 1024 * 1024;
const INDEX_ENUMERATION_LIMIT: usize = 16 * 1024 * 1024;

pub(super) struct StatusSnapshot {
    directory: tempfile::TempDir,
    source_configuration: Vec<u8>,
    pub(super) submodules: Vec<StatusPath>,
}

impl StatusSnapshot {
    pub(super) async fn prepare(process: &GitProcess, root: &Path, deadline: ProbeDeadline) -> Result<Self, StatusError> {
        let source_configuration = required(process, root, &["config", "--null", "--list"], deadline).await?.stdout;
        let configuration = isolated_configuration(&source_configuration)?;
        let index = git_path(process, root, "index", deadline).await?;
        let objects = git_path(process, root, "objects", deadline).await?;
        let attributes = git_path(process, root, "info/attributes", deadline).await?;
        let excludes = git_path(process, root, "info/exclude", deadline).await?;
        let shared = required(process, root, &["rev-parse", "--shared-index-path"], deadline).await?;
        if !shared.stdout.strip_suffix(b"\n").unwrap_or(&shared.stdout).is_empty() {
            return Err(StatusError::UnsupportedConfiguration);
        }
        let head = process.run(Some(root), &["rev-parse", "--verify", "--quiet", "HEAD"], deadline).await.map_err(process_error)?;
        let head = if head.status.success() {
            let oid = head.stdout.strip_suffix(b"\n").unwrap_or(&head.stdout);
            if !matches!(oid.len(), 40 | 64) || !oid.iter().all(u8::is_ascii_hexdigit) {
                return Err(StatusError::InvalidStatus);
            }
            head.stdout
        } else {
            if is_unsafe(&head.stderr) { return Err(StatusError::UnsafeRepository); }
            if head.status.code() != Some(1) { return Err(StatusError::Inaccessible); }
            let symbolic = required(process, root, &["symbolic-ref", "--quiet", "HEAD"], deadline).await?;
            let reference = std::str::from_utf8(symbolic.stdout.strip_suffix(b"\n").unwrap_or(&symbolic.stdout))
                .map_err(|_| StatusError::UnsupportedPathEncoding)?;
            if !reference.starts_with("refs/heads/") { return Err(StatusError::InvalidStatus); }
            let absent = process.run(Some(root), &["show-ref", "--verify", "--quiet", reference], deadline).await.map_err(process_error)?;
            if absent.status.code() != Some(1) || !absent.stderr.is_empty() { return Err(StatusError::Inaccessible); }
            b"ref: refs/heads/gitview-unborn\n".to_vec()
        };
        let task = tokio::task::spawn_blocking(move || {
            let directory = tempfile::tempdir().map_err(|_| StatusError::Inaccessible)?;
            let git_dir = directory.path();
            std::fs::create_dir_all(git_dir.join("objects/info")).map_err(|_| StatusError::Inaccessible)?;
            std::fs::create_dir(git_dir.join("refs")).map_err(|_| StatusError::Inaccessible)?;
            std::fs::create_dir(git_dir.join("info")).map_err(|_| StatusError::Inaccessible)?;
            std::fs::write(git_dir.join("config"), configuration).map_err(|_| StatusError::Inaccessible)?;
            std::fs::write(git_dir.join("HEAD"), head).map_err(|_| StatusError::Inaccessible)?;
            let objects = objects.to_str().ok_or(StatusError::UnsupportedPathEncoding)?;
            let mut alternates = String::with_capacity(objects.len() + 3);
            alternates.push('"');
            append_escaped(&mut alternates, objects);
            alternates.push_str("\"\n");
            std::fs::write(git_dir.join("objects/info/alternates"), alternates).map_err(|_| StatusError::Inaccessible)?;
            copy_metadata(&index, &git_dir.join("index"), INDEX_LIMIT, true, deadline)?;
            copy_metadata(&attributes, &git_dir.join("info/attributes"), METADATA_LIMIT, false, deadline)?;
            copy_metadata(&excludes, &git_dir.join("info/exclude"), METADATA_LIMIT, false, deadline)?;
            deadline.check().map_err(process_error)?;
            Ok::<_, StatusError>(Self { directory, source_configuration, submodules: Vec::new() })
        });
        let mut snapshot = tokio::time::timeout_at(deadline.instant(), task).await
            .map_err(|_| StatusError::Timeout)?.map_err(|_| StatusError::Inaccessible)??;
        // Inspect the copied index, not a racy preflight of the original index.
        let flags = snapshot.run_with_limit(process, root, &["ls-files", "-v", "-z"], deadline, INDEX_ENUMERATION_LIMIT).await?;
        check_success(&flags)?;
        if flags.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty())
            .any(|record| record.first() != Some(&b'H') && record.first() != Some(&b'M')) {
            return Err(StatusError::UnsupportedConfiguration);
        }
        drop(flags);
        let entries = snapshot.run_with_limit(process, root, &["ls-files", "--stage", "-z"], deadline, INDEX_ENUMERATION_LIMIT).await?;
        check_success(&entries)?;
        let mut seen = std::collections::HashSet::new();
        for record in entries.stdout.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
            deadline.check().map_err(process_error)?;
            if record.starts_with(b"040000 ") { return Err(StatusError::UnsupportedConfiguration); }
            if !record.starts_with(b"160000 ") { continue; }
            let separator = record.iter().position(|byte| *byte == b'\t').ok_or(StatusError::InvalidStatus)?;
            let native_path = &record[separator + 1..];
            if !seen.insert(native_path) { continue; }
            if snapshot.submodules.len() >= STATUS_PATH_LIMIT { return Err(StatusError::ResourceLimit); }
            let mut path = status_path(native_path)?;
            // We deliberately make no claim about nested dirtiness or endpoint modes.
            path.unsupported_kind = Some(UnsupportedKind::Submodule);
            snapshot.submodules.push(path);
        }
        Ok(snapshot)
    }

    pub(super) async fn run(&self, process: &GitProcess, root: &Path, arguments: &[&str], deadline: ProbeDeadline) -> Result<ProcessOutput, StatusError> {
        self.run_with_limit(process, root, arguments, deadline, super::STATUS_OUTPUT_LIMIT).await
    }

    async fn run_with_limit(&self, process: &GitProcess, root: &Path, arguments: &[&str], deadline: ProbeDeadline, stdout_limit: usize) -> Result<ProcessOutput, StatusError> {
        let git_dir = self.directory.path().to_str().ok_or(StatusError::UnsupportedPathEncoding)?;
        let worktree = root.to_str().ok_or(StatusError::UnsupportedPathEncoding)?;
        let mut isolated = Vec::with_capacity(arguments.len() + 8);
        isolated.extend_from_slice(&["--git-dir", git_dir, "--work-tree", worktree, "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false"]);
        isolated.extend_from_slice(arguments);
        process.run_isolated_with_output_limit(Some(root), &isolated, deadline, stdout_limit).await.map_err(process_error)
    }

    pub(super) async fn verify_configuration(&self, process: &GitProcess, root: &Path, deadline: ProbeDeadline) -> Result<(), StatusError> {
        let current = required(process, root, &["config", "--null", "--list"], deadline).await?;
        if current.stdout != self.source_configuration {
            return Err(StatusError::UnsupportedConfiguration);
        }
        Ok(())
    }
}

async fn required(process: &GitProcess, root: &Path, arguments: &[&str], deadline: ProbeDeadline) -> Result<ProcessOutput, StatusError> {
    let mut safe_arguments = Vec::with_capacity(arguments.len() + 3);
    safe_arguments.extend_from_slice(&["--no-optional-locks", "-c", "core.fsmonitor=false"]);
    safe_arguments.extend_from_slice(arguments);
    let output = process.run(Some(root), &safe_arguments, deadline).await.map_err(process_error)?;
    check_success(&output)?;
    Ok(output)
}

fn check_success(output: &ProcessOutput) -> Result<(), StatusError> {
    if output.status.success() { return Ok(()); }
    Err(if is_unsafe(&output.stderr) { StatusError::UnsafeRepository } else { StatusError::Inaccessible })
}

async fn git_path(process: &GitProcess, root: &Path, name: &str, deadline: ProbeDeadline) -> Result<PathBuf, StatusError> {
    let output = required(process, root, &["rev-parse", "--path-format=absolute", "--git-path", name], deadline).await?;
    let value = std::str::from_utf8(output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout)).map_err(|_| StatusError::UnsupportedPathEncoding)?;
    let path = PathBuf::from(value);
    if !path.is_absolute() { return Err(StatusError::InvalidStatus); }
    Ok(path)
}

fn isolated_configuration(bytes: &[u8]) -> Result<String, StatusError> {
    let mut configuration = String::from("[core]\n\tbare = false\n\trepositoryformatversion = 0\n");
    for record in bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
        let (key, value) = match record.iter().position(|byte| *byte == b'\n') {
            Some(separator) => (&record[..separator], &record[separator + 1..]),
            None => (record, b"true".as_slice()),
        };
        let key = std::str::from_utf8(key).map_err(|_| StatusError::UnsupportedConfiguration)?;
        if key.starts_with("filter.") && (key.ends_with(".clean") || key.ends_with(".process")) && !value.is_empty() {
            return Err(StatusError::UnsupportedConfiguration);
        }
        if key.starts_with("extensions.") && !matches!(key, "extensions.objectformat" | "extensions.worktreeconfig") {
            return Err(StatusError::UnsupportedConfiguration);
        }
        if matches!(key, "core.sparsecheckout" | "core.sparsecheckoutcone" | "index.sparse" | "core.splitindex")
            && value != b"false" && value != b"0" {
            return Err(StatusError::UnsupportedConfiguration);
        }
        if !matches!(key, "core.filemode" | "core.autocrlf" | "core.eol" | "core.ignorecase" | "core.precomposeunicode" | "core.symlinks" | "core.checkstat" | "core.trustctime" | "core.attributesfile" | "core.excludesfile" | "diff.renames" | "diff.renamelimit" | "status.renames" | "extensions.objectformat") {
            continue;
        }
        let value = std::str::from_utf8(value).map_err(|_| StatusError::UnsupportedConfiguration)?;
        let (section, name) = key.split_once('.').ok_or(StatusError::UnsupportedConfiguration)?;
        if key == "extensions.objectformat" {
            if !matches!(value, "sha1" | "sha256") { return Err(StatusError::UnsupportedConfiguration); }
            configuration.push_str("[core]\n\trepositoryformatversion = 1\n");
        }
        configuration.push('[');
        configuration.push_str(section);
        configuration.push_str("]\n\t");
        configuration.push_str(name);
        configuration.push_str(" = \"");
        append_escaped(&mut configuration, value);
        configuration.push_str("\"\n");
    }
    Ok(configuration)
}

fn append_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\\' => output.push_str("\\\\"),
            '"' => output.push_str("\\\""),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\u{8}' => output.push_str("\\b"),
            other => output.push(other),
        }
    }
}

fn copy_metadata(source: &Path, destination: &Path, limit: u64, preserve_timestamp: bool, deadline: ProbeDeadline) -> Result<(), StatusError> {
    deadline.check().map_err(process_error)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let file = match options.open(source) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(StatusError::Inaccessible),
    };
    let metadata = file.metadata().map_err(|_| StatusError::Inaccessible)?;
    if !metadata.is_file() { return Err(StatusError::Inaccessible); }
    if metadata.len() > limit { return Err(StatusError::ResourceLimit); }
    let mut output = std::fs::File::create(destination).map_err(|_| StatusError::Inaccessible)?;
    let copied = std::io::copy(&mut file.take(limit + 1), &mut output).map_err(|_| StatusError::Inaccessible)?;
    if copied > limit { return Err(StatusError::ResourceLimit); }
    deadline.check().map_err(process_error)?;
    if preserve_timestamp {
        // Git's racy-index check needs the original mtime; a fresh copy can hide same-size edits.
        let modified = metadata.modified().map_err(|_| StatusError::Inaccessible)?;
        output.set_times(std::fs::FileTimes::new().set_modified(modified)).map_err(|_| StatusError::Inaccessible)?;
    }
    Ok(())
}

