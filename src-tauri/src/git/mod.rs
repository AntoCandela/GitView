//! Interprets fixed, read-only Git operations into repository identity and HEAD facts.
//!
//! Native paths stay authoritative in Rust; display labels require lossless text.
//! Process limits and child lifecycle belong to the process adapter, not workspace policy.

use std::path::{Path, PathBuf};

use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component, DiagnosticDetails, Event, OperationContext};
use crate::git::process::{GitProcess, ProbeDeadline, ProcessError, ProcessFailure, ProcessOutput};

pub(crate) mod process;
pub(crate) mod status;

// Keep the executable surface closed: renderer input never becomes Git arguments.
#[derive(Clone, Copy)]
enum GitOperation {
    Version,
    GitDirectory,
    IsBare,
    WorktreeRoot,
    SymbolicHead,
    VerifyHead,
    SymbolicHeadTarget,
    References,
}

impl GitOperation {
    fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Version => &["--version"],
            Self::GitDirectory => &["rev-parse", "--absolute-git-dir"],
            Self::IsBare => &["rev-parse", "--is-bare-repository"],
            Self::WorktreeRoot => &["rev-parse", "--show-toplevel"],
            Self::SymbolicHead => &["symbolic-ref", "--quiet", "--short", "HEAD"],
            Self::VerifyHead => &["rev-parse", "--verify", "HEAD"],
            Self::SymbolicHeadTarget => &["symbolic-ref", "--quiet", "HEAD"],
            Self::References => &["show-ref"],
        }
    }
}

/// Closed probe failures; serialized outcomes contain codes, never native paths or Git stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitError {
    GitUnavailable,
    NotRepository,
    Inaccessible,
    UnsafeRepository,
    ProbeTimeout,
    RepositoryChanged,
    UnsupportedPathEncoding,
    #[serde(rename = "repository_unavailable")]
    Unavailable,
}

impl GitError {
    pub fn code(self) -> &'static str {
        match self {
            Self::GitUnavailable => "git_unavailable",
            Self::NotRepository => "not_repository",
            Self::Inaccessible => "inaccessible",
            Self::UnsafeRepository => "unsafe_repository",
            Self::ProbeTimeout => "probe_timeout",
            Self::RepositoryChanged => "repository_changed",
            Self::UnsupportedPathEncoding => "unsupported_path_encoding",
            Self::Unavailable => "repository_unavailable",
        }
    }
}

/// Repository layout, not worktree cleanliness or workspace eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepositoryKind {
    WorkingTree,
    Bare,
}

/// Resolved branch, eight-character detached object ID, or verified absent branch target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    Branch(String),
    Detached(String),
    Unborn(String),
}

/// Read-only facts from one probe; canonical paths are host-only identity and I/O inputs.
#[derive(Clone, Debug)]
pub struct RepositoryFacts {
    /// Per-worktree Git directory; linked worktrees must not deduplicate by common directory.
    pub git_dir: PathBuf,
    /// Canonical worktree root, or the Git directory itself for a bare repository.
    pub root: PathBuf,
    pub kind: RepositoryKind,
    pub head: Head,
    /// Lossless display text only; neither label is an authoritative native path.
    pub repository_label: String,
    pub location_label: String,
}

/// Probes repository facts through the bounded process adapter without changing Git state.
#[derive(Clone, Default)]
pub struct GitProbe {
    process: GitProcess,
}

impl GitProbe {
    /// Inspects a native directory with one 30-second deadline across all Git operations.
    ///
    /// Rejects unsafe ownership, unsupported path/reference encoding and unstable
    /// canonical paths. HEAD verification failure alone is never proof of an unborn branch.
    pub async fn probe(&self, selected: &Path) -> Result<RepositoryFacts, GitError> {
        self.probe_with_deadline(selected, ProbeDeadline::new()).await
    }

    /// Reuses the caller's absolute deadline, including an admission verification pass.
    pub(crate) async fn probe_with_deadline(
        &self, selected: &Path, deadline: ProbeDeadline,
    ) -> Result<RepositoryFacts, GitError> {
        let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
        let result = self.probe_inner(selected, deadline).await;
        if let Some(trace) = &mut trace {
            trace.finish(if result.is_ok() { Event::Completed } else { Event::Failed },
                result.as_ref().err().map(|error| (*error).into()), DiagnosticDetails::default());
        }
        result
    }

    async fn probe_inner(
        &self,
        selected: &Path,
        deadline: ProbeDeadline,
    ) -> Result<RepositoryFacts, GitError> {
        check_deadline(deadline)?;
        // Never send lossy path text to the renderer or use it as a repository identity.
        selected.to_str().ok_or(GitError::UnsupportedPathEncoding)?;
        ensure_directory(selected)?;
        let selected_directory = std::fs::canonicalize(selected).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GitError::NotRepository
            } else if error.kind() == std::io::ErrorKind::PermissionDenied {
                GitError::Inaccessible
            } else {
                GitError::Unavailable
            }
        })?;
        let version = self.run(&selected_directory, GitOperation::Version, deadline).await?;
        if !version.status.success() || !valid_git_version(&version.stdout) {
            return Err(GitError::GitUnavailable);
        }
        let git_dir = self
            .required(&selected_directory, GitOperation::GitDirectory, deadline)
            .await?;
        let git_dir = path_from_git_output(git_dir)?;
        let git_dir = std::fs::canonicalize(git_dir).map_err(|_| GitError::RepositoryChanged)?;
        let bare = self.required(&selected_directory, GitOperation::IsBare, deadline).await?;
        let kind = match bare.as_slice() {
            b"true" => RepositoryKind::Bare,
            b"false" => RepositoryKind::WorkingTree,
            _ => return Err(GitError::Unavailable),
        };
        let root = if kind == RepositoryKind::Bare {
            git_dir.clone()
        } else {
            let output = self
                .required(&selected_directory, GitOperation::WorktreeRoot, deadline)
                .await?;
            let root = path_from_git_output(output)?;
            std::fs::canonicalize(root).map_err(|_| GitError::RepositoryChanged)?
        };
        let location_label = root
            .to_str()
            .ok_or(GitError::UnsupportedPathEncoding)?
            .to_owned();
        let repository_label = root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&location_label)
            .to_owned();
        let head = self.head(&selected_directory, deadline).await?;
        // A picker alias or resolved root may move while subprocesses are reading it.
        if std::fs::canonicalize(selected).ok().as_deref() != Some(selected_directory.as_path())
            || std::fs::canonicalize(&git_dir).ok().as_deref() != Some(git_dir.as_path())
            || std::fs::canonicalize(&root).ok().as_deref() != Some(root.as_path())
        {
            return Err(GitError::RepositoryChanged);
        }
        check_deadline(deadline)?;
        Ok(RepositoryFacts {
            git_dir,
            root,
            kind,
            head,
            repository_label,
            location_label,
        })
    }

    async fn head(&self, at: &Path, deadline: ProbeDeadline) -> Result<Head, GitError> {
        let symbolic = self.run(at, GitOperation::SymbolicHead, deadline).await?;
        let verified = self.run(at, GitOperation::VerifyHead, deadline).await?;
        if is_unsafe(&symbolic.stderr) || is_unsafe(&verified.stderr) {
            return Err(GitError::UnsafeRepository);
        }
        if symbolic.status.success() {
            let branch = git_text(symbolic.stdout)?;
            if verified.status.success() {
                if !valid_oid(strip_line_ending(&verified.stdout)) {
                    return Err(GitError::Unavailable);
                }
                return Ok(Head::Branch(branch));
            }
            // Failed verification alone cannot distinguish an unborn branch from a read failure.
            // Require an absent symbolic target in successfully inspected reference storage.
            let target = self.required(at, GitOperation::SymbolicHeadTarget, deadline).await?;
            if !target.starts_with(b"refs/heads/") {
                return Err(GitError::Unavailable);
            }
            let references = self.run(at, GitOperation::References, deadline).await?;
            if is_unsafe(&references.stderr) {
                return Err(GitError::UnsafeRepository);
            }
            if !references.stderr.is_empty() {
                return Err(GitError::Unavailable);
            }
            if !references.status.success() {
                // show-ref uses exit 1 with no output for an empty reference set.
                if references.status.code() == Some(1) && references.stdout.is_empty() {
                    return Ok(Head::Unborn(branch));
                }
                return Err(GitError::Unavailable);
            }
            for line in strip_line_ending(&references.stdout).split(|byte| *byte == b'\n') {
                let Some(separator) = line.iter().position(|byte| *byte == b' ') else {
                    return Err(GitError::Unavailable);
                };
                if !valid_oid(&line[..separator]) || line[separator + 1..].is_empty() {
                    return Err(GitError::Unavailable);
                }
                // An existing target rules out unborn even when HEAD verification failed.
                if line[separator + 1..] == target {
                    return Err(GitError::Unavailable);
                }
            }
            return Ok(Head::Unborn(branch));
        }
        // Only symbolic-ref's documented non-symbolic exit permits the detached fallback.
        if symbolic.status.code() == Some(1) && verified.status.success() {
            let oid = git_text(verified.stdout)?;
            if !valid_oid(oid.as_bytes()) {
                return Err(GitError::Unavailable);
            }
            return Ok(Head::Detached(oid[..8].to_owned()));
        }
        Err(GitError::Unavailable)
    }

    async fn required(
        &self,
        at: &Path,
        operation: GitOperation,
        deadline: ProbeDeadline,
    ) -> Result<Vec<u8>, GitError> {
        let mut output = self.run(at, operation, deadline).await?;
        if !output.status.success() {
            if is_unsafe(&output.stderr) {
                return Err(GitError::UnsafeRepository);
            }
            if output
                .stderr
                .windows(b"Permission denied".len())
                .any(|part| part == b"Permission denied")
            {
                return Err(GitError::Inaccessible);
            }
            return Err(if matches!(operation, GitOperation::GitDirectory) {
                GitError::NotRepository
            } else {
                GitError::Unavailable
            });
        }
        trim_line_ending(&mut output.stdout);
        Ok(output.stdout)
    }

    async fn run(
        &self,
        at: &Path,
        operation: GitOperation,
        deadline: ProbeDeadline,
    ) -> Result<ProcessOutput, GitError> {
        let directory = if matches!(operation, GitOperation::Version) {
            None
        } else {
            Some(at)
        };
        self.process
            .run(directory, operation.arguments(), deadline)
            .await
            .map_err(|error| process_error(error, operation))
    }

    #[cfg(test)]
    pub(crate) fn with_executable(executable: &Path) -> Self {
        Self {
            process: GitProcess::with_executable(executable),
        }
    }
}

fn process_error(error: ProcessError, operation: GitOperation) -> GitError {
    // Cleanup failure prevents reporting a timeout as though the child lifecycle completed.
    if error.cleanup_failed() {
        return GitError::Unavailable;
    }
    match error.failure {
        ProcessFailure::Deadline => GitError::ProbeTimeout,
        ProcessFailure::Start(_) if matches!(operation, GitOperation::Version) => {
            GitError::GitUnavailable
        }
        _ => GitError::Unavailable,
    }
}

fn check_deadline(deadline: ProbeDeadline) -> Result<(), GitError> {
    deadline.check().map_err(|error| process_error(error, GitOperation::GitDirectory))
}

fn ensure_directory(path: &Path) -> Result<(), GitError> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(GitError::NotRepository),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(GitError::NotRepository),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(GitError::Inaccessible)
        }
        Err(_) => Err(GitError::Unavailable),
    }
}

fn valid_git_version(output: &[u8]) -> bool {
    let Ok(output) = std::str::from_utf8(strip_line_ending(output)) else {
        return false;
    };
    let Some(version) = output
        .strip_prefix("git version ")
        .and_then(|rest| rest.split_ascii_whitespace().next())
    else {
        return false;
    };
    let Some((major, remainder)) = version.split_once('.') else {
        return false;
    };
    let minor = remainder.split('.').next().unwrap_or_default();
    !major.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && !minor.is_empty()
        && minor.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_oid(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 40 | 64) && bytes.iter().all(u8::is_ascii_hexdigit)
}

fn is_unsafe(stderr: &[u8]) -> bool {
    [
        b"detected dubious ownership".as_slice(),
        b"unsafe repository",
        b"is owned by someone else",
    ]
    .iter()
    .any(|needle| stderr.windows(needle.len()).any(|window| window == *needle))
}

// Strip one protocol terminator, not whitespace that could belong to a native path.
fn strip_line_ending(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

fn trim_line_ending(bytes: &mut Vec<u8>) {
    bytes.truncate(strip_line_ending(bytes).len());
}

fn git_text(mut bytes: Vec<u8>) -> Result<String, GitError> {
    trim_line_ending(&mut bytes);
    if bytes.is_empty() {
        return Err(GitError::Unavailable);
    }
    String::from_utf8(bytes).map_err(|_| GitError::UnsupportedPathEncoding)
}

/// Decodes Git's native path bytes before canonicalization, without replacement characters.
fn path_from_git_output(bytes: Vec<u8>) -> Result<PathBuf, GitError> {
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(GitError::Unavailable);
    }
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        // Unix filesystem identity is byte-based; text validation happens at the label boundary.
        PathBuf::from(std::ffi::OsString::from_vec(bytes))
    };
    #[cfg(not(unix))]
    let path = {
        let text = String::from_utf8(bytes).map_err(|_| GitError::UnsupportedPathEncoding)?;
        PathBuf::from(text)
    };
    if !path.is_absolute() {
        return Err(GitError::Unavailable);
    }
    Ok(path)
}

#[cfg(test)]
#[path = "../../tests/integration/git.rs"]
mod integration_tests;
