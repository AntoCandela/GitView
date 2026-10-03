//! Produces bounded read-only file comparisons from revision-authorized native paths.
//!
//! Git compares private snapshots, never live worktree paths: untracked and working
//! bytes are opened through rooted no-follow descriptors before any diff process.

use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component, OperationContext};
use crate::git::{GitError, GitProbe, Head};
use crate::git::process::{GitProcess, ProbeDeadline, ProcessError, ProcessFailure};
use crate::git::status::{GitStatusReader, StatusError, StatusPath, UnsupportedKind};
use crate::workspace::{NativeIdentity, SelectedContext};

mod parser;
pub(crate) mod rooted_read;
pub mod committed;

pub(crate) const CONTENT_LIMIT: usize = 1024 * 1024;

/// A comparison category, not a renderer-provided Git revision or path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCategory { Staged, Unstaged, Untracked }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ReviewFrom { #[serde(rename = "HEAD")] Head, #[serde(rename = "index")] Index, #[serde(rename = "absent")] Absent }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTo { Index, WorkingFiles }

/// Endpoint labels and absence are independent: an empty existing file is not absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewIdentity {
    pub entry_id: String,
    pub path_id: String,
    pub category: ReviewCategory,
    pub display_path: String,
    pub context_label: String,
    pub from: ReviewFrom,
    pub to: ReviewTo,
    pub from_absent: bool,
    pub to_absent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedReason { Conflict, RenameOrCopy, TypeChange, Submodule, Binary, LargeOrTruncated, UnbornHead, UnsupportedEncoding, Other }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewErrorCode { Inaccessible, GitUnavailable, UnsafeRepository, Timeout, ChangedDuringRead, InvalidOutput }

fn is_false(value: &bool) -> bool { !value }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind { Context, Addition, Removal }
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLine {
    pub kind: LineKind,
    pub text: String,
    #[serde(skip_serializing_if = "is_false")]
    pub no_final_newline: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<TextLine>,
}

/// Complete endpoint text and hunks come from the same bounded private snapshots.
pub(crate) struct TextContent {
    pub hunks: Vec<TextHunk>,
    pub from_content: String,
    pub to_content: String,
}

impl TextContent {
    pub(crate) fn from_snapshots(hunks: Vec<TextHunk>, (from, to): EndpointBytes) -> Result<Self, ReviewFailure> {
        let decode = |bytes| String::from_utf8(bytes).map_err(|_| unsupported(UnsupportedReason::UnsupportedEncoding));
        Ok(Self { hunks, from_content: decode(from)?, to_content: decode(to)? })
    }
}

/// Stale outcomes deliberately contain no obsolete path or context identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewResult {
    Text {
        #[serde(flatten)] identity: ReviewIdentity,
        hunks: Vec<TextHunk>,
        #[serde(rename = "fromContent")] from_content: String,
        #[serde(rename = "toContent")] to_content: String,
    },
    Unsupported { reason: UnsupportedReason, identity: ReviewIdentity },
    Unavailable { code: ReviewErrorCode, identity: ReviewIdentity },
    StaleSelection,
    StaleObservation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReviewFailure { Unsupported(UnsupportedReason), Unavailable(ReviewErrorCode) }
impl ReviewFailure {
    pub(crate) fn result(self, identity: ReviewIdentity) -> ReviewResult {
        match self {
            Self::Unsupported(reason) => ReviewResult::Unsupported { reason, identity },
            Self::Unavailable(code) => ReviewResult::Unavailable { code, identity },
        }
    }
}

type EndpointBytes = (Vec<u8>, Vec<u8>);

impl ReviewCategory {
    pub(crate) fn authorizes(self, path: &StatusPath) -> bool {
        // Unsupported rows have no ordinary category but remain explicitly reviewable.
        match self {
            Self::Staged => path.staged.is_some() || path.conflict || path.unsupported_kind.is_some(),
            Self::Unstaged => path.unstaged.is_some() || path.conflict || path.unsupported_kind.is_some(),
            Self::Untracked => path.untracked,
        }
    }
}

pub(crate) fn identity(context: &SelectedContext, path_id: &str, path: &StatusPath, category: ReviewCategory) -> ReviewIdentity {
    let (from, to, old_mode, new_mode) = match category {
        ReviewCategory::Staged => (ReviewFrom::Head, ReviewTo::Index, path.modes.as_ref().and_then(|m| m.head), path.modes.as_ref().and_then(|m| m.index)),
        ReviewCategory::Unstaged => (ReviewFrom::Index, ReviewTo::WorkingFiles, path.modes.as_ref().and_then(|m| m.index), path.modes.as_ref().map(|m| m.worktree)),
        ReviewCategory::Untracked => (ReviewFrom::Absent, ReviewTo::WorkingFiles, Some(0), None),
    };
    ReviewIdentity {
        entry_id: context.entry_id.clone(), path_id: path_id.to_owned(), category,
        display_path: path.display_path.clone(),
        context_label: context.root.file_name().and_then(|name| name.to_str()).unwrap_or("Working tree").to_owned(),
        from, to, from_absent: old_mode == Some(0), to_absent: new_mode == Some(0),
    }
}

pub(crate) async fn read_review(process: &GitProcess, context: &SelectedContext, path: &StatusPath, identity: ReviewIdentity) -> ReviewResult {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
    let result = match read_inner(process, context, path, &identity).await {
        Ok(TextContent { hunks, from_content, to_content }) => ReviewResult::Text { identity, hunks, from_content, to_content },
        Err(failure) => failure.result(identity),
    };
    if let Some(trace) = &mut trace { trace.outcome(&result); }
    result
}

async fn verify_context(context: &SelectedContext, deadline: ProbeDeadline) -> Result<Head, ReviewFailure> {
    verify_identity(context, deadline).await?;
    let facts = GitProbe::default().probe_with_deadline(&context.root, deadline).await.map_err(git_error)?;
    if facts.root != context.root || facts.git_dir != context.git_dir || facts.kind != context.kind {
        return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
    }
    verify_identity(context, deadline).await?;
    Ok(facts.head)
}

async fn verify_identity(context: &SelectedContext, deadline: ProbeDeadline) -> Result<(), ReviewFailure> {
    let root = context.root.clone();
    let git_dir = context.git_dir.clone();
    let task = tokio::task::spawn_blocking(move || NativeIdentity::capture(&root, &git_dir));
    let captured = tokio::time::timeout_at(deadline.instant(), task).await
        .map_err(|_| unavailable(ReviewErrorCode::Timeout))?
        .map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?.map_err(git_error)?;
    if captured != context.identity { return Err(unavailable(ReviewErrorCode::ChangedDuringRead)); }
    Ok(())
}

async fn check_status(context: &SelectedContext, path: &StatusPath, deadline: ProbeDeadline) -> Result<(), ReviewFailure> {
    let paths = GitStatusReader::default().read_with_deadline(&context.root, deadline).await.map_err(status_error)?;
    if paths.iter().find(|candidate| candidate.native_path == path.native_path) != Some(path) {
        return Err(unavailable(ReviewErrorCode::ChangedDuringRead));
    }
    Ok(())
}

async fn read_inner(process: &GitProcess, context: &SelectedContext, path: &StatusPath, identity: &ReviewIdentity) -> Result<TextContent, ReviewFailure> {
    let deadline = ProbeDeadline::new();
    let head = verify_context(context, deadline).await?;
    check_status(context, path, deadline).await?;
    if path.conflict { return Err(unsupported(UnsupportedReason::Conflict)); }
    if let Some(kind) = path.unsupported_kind {
        return Err(unsupported(match kind {
            UnsupportedKind::RenameOrCopy => UnsupportedReason::RenameOrCopy,
            UnsupportedKind::Submodule => UnsupportedReason::Submodule,
            UnsupportedKind::TypeChange => UnsupportedReason::TypeChange,
        }));
    }
    if identity.category == ReviewCategory::Staged && matches!(head, Head::Unborn(_)) {
        return Err(unsupported(UnsupportedReason::UnbornHead));
    }
    if let Some(modes) = &path.modes {
        if [modes.head.unwrap_or(0), modes.index.unwrap_or(0), modes.worktree].iter()
            .any(|mode| !matches!(*mode, 0 | 0o100644 | 0o100755)) {
            return Err(unsupported(UnsupportedReason::TypeChange));
        }
        let (old_mode, new_mode) = match identity.category {
            ReviewCategory::Staged => (modes.head.unwrap_or(0), modes.index.unwrap_or(0)),
            ReviewCategory::Unstaged => (modes.index.unwrap_or(0), modes.worktree),
            ReviewCategory::Untracked => (0, 0),
        };
        if old_mode != 0 && new_mode != 0 && old_mode != new_mode {
            return Err(unsupported(UnsupportedReason::Other));
        }
    }
    let before = endpoints(process, context, path, identity, deadline).await?;
    let (hunks, before) = compare(process, before, deadline).await?;
    // Status alone cannot detect a second edit which leaves the category unchanged.
    let after = endpoints(process, context, path, identity, deadline).await?;
    if before != after { return Err(unavailable(ReviewErrorCode::ChangedDuringRead)); }
    check_status(context, path, deadline).await?;
    verify_context(context, deadline).await?;
    TextContent::from_snapshots(hunks, before)
}

async fn endpoints(process: &GitProcess, context: &SelectedContext, path: &StatusPath, identity: &ReviewIdentity, deadline: ProbeDeadline) -> Result<EndpointBytes, ReviewFailure> {
    let from = if identity.from_absent { Vec::new() } else {
        let revision = if identity.category == ReviewCategory::Staged { "HEAD" } else { "" };
        blob(process, &context.root, revision, &path.native_path, deadline).await?
    };
    let to = if identity.to_absent { Vec::new() } else if identity.category == ReviewCategory::Staged {
        blob(process, &context.root, "", &path.native_path, deadline).await?
    } else {
        rooted_read::read(context, &path.native_path, deadline).await?
    };
    validate_content(&from)?;
    validate_content(&to)?;
    Ok((from, to))
}

async fn blob(process: &GitProcess, root: &Path, revision: &str, path: &Path, deadline: ProbeDeadline) -> Result<Vec<u8>, ReviewFailure> {
    let path = path.to_str().ok_or(unsupported(UnsupportedReason::UnsupportedEncoding))?;
    let object = format!("{revision}:{path}");
    let output = process.run(Some(root), &["--no-optional-locks", "-c", "core.fsmonitor=false", "cat-file", "blob", &object], deadline).await.map_err(process_error)?;
    if !output.status.success() { return Err(git_exit(&output.stderr)); }
    Ok(output.stdout)
}

pub(crate) async fn compare(process: &GitProcess, endpoints: EndpointBytes, deadline: ProbeDeadline) -> Result<(Vec<TextHunk>, EndpointBytes), ReviewFailure> {
    // Private snapshots make Git incapable of racing a live path into a symlink escape.
    let task = tokio::task::spawn_blocking(move || {
        let scratch = tempfile::tempdir().map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
        write_snapshot(&scratch.path().join("from"), &endpoints.0)?;
        write_snapshot(&scratch.path().join("to"), &endpoints.1)?;
        Ok::<_, ReviewFailure>((scratch, endpoints))
    });
    let (scratch, endpoints) = tokio::time::timeout_at(deadline.instant(), task).await
        .map_err(|_| unavailable(ReviewErrorCode::Timeout))?
        .map_err(|_| unavailable(ReviewErrorCode::Inaccessible))??;
    let output = process.run(Some(scratch.path()), &[
        "--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "diff.algorithm=myers", "-c", "diff.indentHeuristic=false", "-c", "diff.suppressBlankEmpty=false",
        "diff", "--no-index", "--text", "--no-ext-diff", "--no-textconv", "--no-renames", "--no-color", "--no-prefix", "--unified=3", "--output-indicator-new=+", "--output-indicator-old=-", "--output-indicator-context= ", "--", "from", "to",
    ], deadline).await.map_err(process_error)?;
    if !matches!(output.status.code(), Some(0 | 1)) { return Err(git_exit(&output.stderr)); }
    Ok((parser::parse(&output.stdout, deadline)?, endpoints))
}

fn write_snapshot(path: &Path, bytes: &[u8]) -> Result<(), ReviewFailure> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut file = options.open(path).map_err(|_| unavailable(ReviewErrorCode::Inaccessible))?;
    file.write_all(bytes).map_err(|_| unavailable(ReviewErrorCode::Inaccessible))
}

pub(crate) fn validate_content(bytes: &[u8]) -> Result<(), ReviewFailure> {
    if bytes.len() > CONTENT_LIMIT { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    if bytes.contains(&0) { return Err(unsupported(UnsupportedReason::Binary)); }
    std::str::from_utf8(bytes).map_err(|_| unsupported(UnsupportedReason::UnsupportedEncoding))?;
    let lines = bytes.iter().filter(|byte| **byte == b'\n').take(parser::MAX_LINES + 1).count()
        + usize::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'));
    if lines > parser::MAX_LINES { return Err(unsupported(UnsupportedReason::LargeOrTruncated)); }
    Ok(())
}

pub(crate) fn unsupported(reason: UnsupportedReason) -> ReviewFailure { ReviewFailure::Unsupported(reason) }
pub(crate) fn unavailable(code: ReviewErrorCode) -> ReviewFailure { ReviewFailure::Unavailable(code) }
pub(crate) fn process_error(error: ProcessError) -> ReviewFailure {
    if error.cleanup_failed() { return unavailable(ReviewErrorCode::Inaccessible); }
    match error.failure {
        ProcessFailure::OutputLimit => unsupported(UnsupportedReason::LargeOrTruncated),
        ProcessFailure::Deadline => unavailable(ReviewErrorCode::Timeout),
        ProcessFailure::Start(_) => unavailable(ReviewErrorCode::GitUnavailable),
        ProcessFailure::Io(_) => unavailable(ReviewErrorCode::Inaccessible),
    }
}
fn git_exit(stderr: &[u8]) -> ReviewFailure {
    let unsafe_repository = [b"detected dubious ownership".as_slice(), b"unsafe repository", b"is owned by someone else"]
        .iter().any(|needle| stderr.windows(needle.len()).any(|window| window == *needle));
    unavailable(if unsafe_repository { ReviewErrorCode::UnsafeRepository } else { ReviewErrorCode::Inaccessible })
}
fn git_error(error: GitError) -> ReviewFailure {
    unavailable(match error {
        GitError::GitUnavailable => ReviewErrorCode::GitUnavailable,
        GitError::UnsafeRepository => ReviewErrorCode::UnsafeRepository,
        GitError::ProbeTimeout => ReviewErrorCode::Timeout,
        GitError::RepositoryChanged => ReviewErrorCode::ChangedDuringRead,
        _ => ReviewErrorCode::Inaccessible,
    })
}
fn status_error(error: StatusError) -> ReviewFailure {
    match error {
        StatusError::ResourceLimit => unsupported(UnsupportedReason::LargeOrTruncated),
        StatusError::UnsupportedPathEncoding => unsupported(UnsupportedReason::UnsupportedEncoding),
        StatusError::InvalidStatus => unavailable(ReviewErrorCode::InvalidOutput),
        StatusError::GitUnavailable => unavailable(ReviewErrorCode::GitUnavailable),
        StatusError::UnsafeRepository => unavailable(ReviewErrorCode::UnsafeRepository),
        StatusError::Timeout => unavailable(ReviewErrorCode::Timeout),
        StatusError::Inaccessible => unavailable(ReviewErrorCode::Inaccessible),
        StatusError::UnsupportedConfiguration => unsupported(UnsupportedReason::Other),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/diff.rs"]
mod unit_tests;
#[cfg(all(test, unix))]
#[path = "../../tests/integration/diff.rs"]
mod integration_tests;
