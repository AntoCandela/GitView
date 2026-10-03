//! Reads one bounded, read-only Git status snapshot without turning display text into identity.
//!
//! Porcelain-v2 records are parsed bytewise and validated in full. Unsupported
//! comparisons stay visible; malformed output never becomes a partial or clean snapshot.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component as DiagnosticComponent, DiagnosticDetails, Event, OperationContext};
use crate::git::process::{GitProcess, ProbeDeadline, ProcessError, ProcessFailure};

mod snapshot;

const STATUS_OUTPUT_LIMIT: usize = 1024 * 1024;
const STATUS_PATH_LIMIT: usize = 16_384;
const STATUS_ARGUMENTS: &[&str] = &[
    "--no-optional-locks",
    "status",
    "--porcelain=v2",
    "-z",
    "--untracked-files=all",
    "--ignore-submodules=all",
];

/// An ordinary comparison category, independently reported for index and worktree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Modified,
    Added,
    Deleted,
}

/// Visible changes for which no ordinary text comparison is currently supported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnsupportedKind {
    RenameOrCopy,
    Submodule,
    TypeChange,
}

/// Sanitized observation failures; none of these imply a clean worktree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusError {
    Inaccessible,
    GitUnavailable,
    UnsafeRepository,
    Timeout,
    InvalidStatus,
    UnsupportedPathEncoding,
    ResourceLimit,
    UnsupportedConfiguration,
}

/// Native Git modes retained for later review, never renderer-supplied authority.
///
/// Ordinary records retain HEAD/index/worktree modes. Unmerged records instead
/// retain stages 1/2/3; untracked paths have no mode record at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusModes {
    pub(crate) head: Option<u32>,
    pub(crate) index: Option<u32>,
    pub(crate) worktree: u32,
    pub(crate) conflict_stages: Option<[u32; 3]>,
}

/// Exact repository-relative native paths plus lossless, presentation-only labels.
///
/// Renames/copies retain their native origin. Conflicts and unsupported changes
/// intentionally have no ordinary staged/unstaged comparison categories.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StatusPath {
    pub(crate) native_path: PathBuf,
    pub(crate) native_origin: Option<PathBuf>,
    pub(crate) display_path: String,
    pub(crate) segments: Vec<String>,
    pub(crate) staged: Option<ChangeKind>,
    pub(crate) unstaged: Option<ChangeKind>,
    pub(crate) untracked: bool,
    pub(crate) conflict: bool,
    pub(crate) unsupported_kind: Option<UnsupportedKind>,
    pub(crate) modes: Option<StatusModes>,
}

/// Reads complete status snapshots through the existing bounded Git process adapter.
#[derive(Clone, Default)]
pub(crate) struct GitStatusReader {
    process: GitProcess,
}

impl GitStatusReader {
    /// Reads a worktree root with a single deadline and no optional index writes.
    ///
    /// Ownership checks remain enabled. Status hashes only with isolated comparison
    /// configuration; executable filters and unsupported index layouts fail closed.
    /// Dropping this future cancels and reaps Git; errors discard the entire snapshot.
    pub(crate) async fn read(&self, root: &Path) -> Result<Vec<StatusPath>, StatusError> {
        self.read_with_deadline(root, ProbeDeadline::new()).await
    }

    pub(crate) async fn read_with_deadline(
        &self, root: &Path, deadline: ProbeDeadline,
    ) -> Result<Vec<StatusPath>, StatusError> {
        let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, DiagnosticComponent::Git));
        let result = self.read_inner(root, deadline).await;
        if let Some(trace) = &mut trace {
            trace.finish(if result.is_ok() { Event::Completed } else { Event::Failed },
                result.as_ref().err().map(|error| (*error).into()), DiagnosticDetails::default());
        }
        result
    }

    async fn read_inner(
        &self,
        root: &Path,
        deadline: ProbeDeadline,
    ) -> Result<Vec<StatusPath>, StatusError> {
        deadline.check().map_err(process_error)?;
        root.to_str().ok_or(StatusError::UnsupportedPathEncoding)?;
        let metadata = std::fs::metadata(root).map_err(|_| StatusError::Inaccessible)?;
        if !metadata.is_dir() {
            return Err(StatusError::Inaccessible);
        }
        let snapshot = snapshot::StatusSnapshot::prepare(&self.process, root, deadline).await?;
        let output = snapshot.run(&self.process, root, STATUS_ARGUMENTS, deadline).await?;
        if !output.status.success() {
            return Err(if is_unsafe(&output.stderr) {
                StatusError::UnsafeRepository
            } else {
                StatusError::Inaccessible
            });
        }
        snapshot.verify_configuration(&self.process, root, deadline).await?;
        let mut paths = parse_status(&output.stdout, deadline)?;
        let seen: HashSet<&Path> = paths.iter().map(|path| path.native_path.as_path()).collect();
        let mut submodules = snapshot.submodules;
        submodules.retain(|path| !seen.contains(path.native_path.as_path()));
        drop(seen);
        if paths.len() + submodules.len() > STATUS_PATH_LIMIT { return Err(StatusError::ResourceLimit); }
        paths.extend(submodules);
        Ok(paths)
    }

    #[cfg(test)]
    pub(crate) fn with_executable(executable: &Path) -> Self {
        Self { process: GitProcess::with_executable(executable) }
    }
}

fn process_error(error: ProcessError) -> StatusError {
    // A failed reap must not claim an otherwise successfully completed timeout cleanup.
    if error.cleanup_failed() {
        return StatusError::Inaccessible;
    }
    match error.failure {
        ProcessFailure::Deadline => StatusError::Timeout,
        ProcessFailure::OutputLimit => StatusError::ResourceLimit,
        ProcessFailure::Start(_) => StatusError::GitUnavailable,
        ProcessFailure::Io(_) => StatusError::Inaccessible,
    }
}

fn is_unsafe(stderr: &[u8]) -> bool {
    [
        b"detected dubious ownership".as_slice(),
        b"unsafe repository",
        b"is owned by someone else",
    ].iter().any(|needle| stderr.windows(needle.len()).any(|window| window == *needle))
}

fn parse_status(bytes: &[u8], deadline: ProbeDeadline) -> Result<Vec<StatusPath>, StatusError> {
    deadline.check().map_err(process_error)?;
    if bytes.len() > STATUS_OUTPUT_LIMIT {
        return Err(StatusError::ResourceLimit);
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let body = bytes.strip_suffix(b"\0").ok_or(StatusError::InvalidStatus)?;
    let mut records = body.split(|byte| *byte == 0);
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    while let Some(record) = records.next() {
        deadline.check().map_err(process_error)?;
        if paths.len() >= STATUS_PATH_LIMIT {
            return Err(StatusError::ResourceLimit);
        }
        let (path, native_bytes) = parse_record(record, &mut records)?;
        if !seen.insert(native_bytes) {
            return Err(StatusError::InvalidStatus);
        }
        paths.push(path);
    }
    deadline.check().map_err(process_error)?;
    Ok(paths)
}

fn parse_record<'a>(
    record: &'a [u8],
    records: &mut impl Iterator<Item = &'a [u8]>,
) -> Result<(StatusPath, &'a [u8]), StatusError> {
    match record.first() {
        Some(b'?') => {
            let bytes = record.strip_prefix(b"? ").ok_or(StatusError::InvalidStatus)?;
            let mut path = status_path(bytes)?;
            path.untracked = true;
            Ok((path, bytes))
        }
        Some(b'1') => {
            let fields = fields::<9>(record);
            if fields[0] != b"1" {
                return Err(StatusError::InvalidStatus);
            }
            let path = ordinary_path(fields[1], fields[2], &fields[3..6], &fields[6..8], fields[8], false)?;
            Ok((path, fields[8]))
        }
        Some(b'2') => {
            let fields = fields::<10>(record);
            if fields[0] != b"2" {
                return Err(StatusError::InvalidStatus);
            }
            let mut path = ordinary_path(fields[1], fields[2], &fields[3..6], &fields[6..8], fields[9], true)?;
            validate_score(fields[8], fields[1])?;
            let origin = records.next().ok_or(StatusError::InvalidStatus)?;
            let origin = validate_path(origin)?;
            if origin == path.display_path {
                return Err(StatusError::InvalidStatus);
            }
            path.native_origin = Some(PathBuf::from(origin));
            // Record 2 cannot be reduced to a plain addition/modification, even for text modes.
            path.unsupported_kind = Some(UnsupportedKind::RenameOrCopy);
            path.staged = None;
            path.unstaged = None;
            Ok((path, fields[9]))
        }
        Some(b'u') => conflict_path(record),
        _ => Err(StatusError::InvalidStatus),
    }
}

fn fields<const N: usize>(record: &[u8]) -> [&[u8]; N] {
    // Only protocol metadata uses spaces as delimiters; the final field is the raw path.
    let mut fields = record.splitn(N, |byte| *byte == b' ');
    std::array::from_fn(|_| fields.next().unwrap_or_default())
}

fn ordinary_path(
    xy: &[u8],
    submodule: &[u8],
    modes: &[&[u8]],
    oids: &[&[u8]],
    bytes: &[u8],
    rename: bool,
) -> Result<StatusPath, StatusError> {
    validate_xy(xy, rename)?;
    validate_oids(oids)?;
    let modes = [parse_mode(modes[0])?, parse_mode(modes[1])?, parse_mode(modes[2])?];
    validate_mode_oids(&modes, oids)?;
    let submodule = validate_submodule(submodule, &modes)?;
    let mut path = status_path(bytes)?;
    path.modes = Some(StatusModes {
        head: Some(modes[0]),
        index: Some(modes[1]),
        worktree: modes[2],
        conflict_stages: None,
    });
    path.unsupported_kind = if submodule {
        Some(UnsupportedKind::Submodule)
    } else if xy.contains(&b'T') || different_types(&modes) {
        Some(UnsupportedKind::TypeChange)
    } else {
        None
    };
    if path.unsupported_kind.is_none() && !rename {
        path.staged = change_kind(xy[0]);
        path.unstaged = change_kind(xy[1]);
    }
    Ok(path)
}

fn conflict_path(record: &[u8]) -> Result<(StatusPath, &[u8]), StatusError> {
    let fields = fields::<11>(record);
    if fields[0] != b"u" || !matches!(fields[1], b"DD" | b"AU" | b"UD" | b"UA" | b"DU" | b"AA" | b"UU") {
        return Err(StatusError::InvalidStatus);
    }
    validate_oids(&fields[7..10])?;
    let stages = [parse_mode(fields[3])?, parse_mode(fields[4])?, parse_mode(fields[5])?];
    let worktree = parse_mode(fields[6])?;
    validate_mode_oids(&stages, &fields[7..10])?;
    let submodule = validate_submodule(fields[2], &[stages[0], stages[1], stages[2], worktree])?;
    let mut path = status_path(fields[10])?;
    path.conflict = true;
    path.modes = Some(StatusModes {
        head: None,
        index: None,
        worktree,
        conflict_stages: Some(stages),
    });
    if submodule {
        path.unsupported_kind = Some(UnsupportedKind::Submodule);
    }
    Ok((path, fields[10]))
}

fn validate_xy(xy: &[u8], rename: bool) -> Result<(), StatusError> {
    if xy.len() != 2 || xy == b".." || !xy.iter().all(|byte| matches!(byte, b'.' | b'M' | b'A' | b'D' | b'T' | b'R' | b'C')) {
        return Err(StatusError::InvalidStatus);
    }
    if rename != xy.iter().any(|byte| matches!(byte, b'R' | b'C')) {
        return Err(StatusError::InvalidStatus);
    }
    Ok(())
}

fn change_kind(byte: u8) -> Option<ChangeKind> {
    match byte {
        b'M' => Some(ChangeKind::Modified),
        b'A' => Some(ChangeKind::Added),
        b'D' => Some(ChangeKind::Deleted),
        _ => None,
    }
}

fn parse_mode(bytes: &[u8]) -> Result<u32, StatusError> {
    match bytes {
        b"000000" => Ok(0),
        b"100644" => Ok(0o100644),
        b"100755" => Ok(0o100755),
        b"120000" => Ok(0o120000),
        b"160000" => Ok(0o160000),
        _ => Err(StatusError::InvalidStatus),
    }
}

fn validate_oids(oids: &[&[u8]]) -> Result<(), StatusError> {
    let length = oids[0].len();
    if !matches!(length, 40 | 64) || !oids.iter().all(|oid| oid.len() == length && oid.iter().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))) {
        return Err(StatusError::InvalidStatus);
    }
    Ok(())
}

fn validate_mode_oids(modes: &[u32], oids: &[&[u8]]) -> Result<(), StatusError> {
    if modes.iter().all(|mode| *mode == 0)
        || modes.iter().zip(oids).any(|(mode, oid)| (*mode == 0) != oid.iter().all(|byte| *byte == b'0')) {
        return Err(StatusError::InvalidStatus);
    }
    Ok(())
}

fn validate_submodule(bytes: &[u8], modes: &[u32]) -> Result<bool, StatusError> {
    let gitlink = modes.contains(&0o160000);
    if bytes == b"N..." {
        return Ok(gitlink);
    }
    if bytes.len() == 4 && bytes[0] == b'S' && matches!(bytes[1], b'.' | b'C')
        && matches!(bytes[2], b'.' | b'M') && matches!(bytes[3], b'.' | b'U') && gitlink {
        return Ok(true);
    }
    Err(StatusError::InvalidStatus)
}

fn different_types(modes: &[u32]) -> bool {
    let mut present = modes.iter().filter(|mode| **mode != 0).map(|mode| mode & 0o170000);
    let Some(first) = present.next() else { return false };
    present.any(|mode| mode != first)
}

fn validate_score(bytes: &[u8], xy: &[u8]) -> Result<(), StatusError> {
    let Some((&kind, digits)) = bytes.split_first() else {
        return Err(StatusError::InvalidStatus);
    };
    if !matches!(kind, b'R' | b'C') || !xy.contains(&kind) || digits.is_empty()
        || digits.len() > 3 || !digits.iter().all(u8::is_ascii_digit) {
        return Err(StatusError::InvalidStatus);
    }
    let score = digits.iter().fold(0u32, |score, digit| score * 10 + u32::from(digit - b'0'));
    if score > 100 {
        return Err(StatusError::InvalidStatus);
    }
    Ok(())
}

fn validate_path(bytes: &[u8]) -> Result<&str, StatusError> {
    let text = std::str::from_utf8(bytes).map_err(|_| StatusError::UnsupportedPathEncoding)?;
    if text.is_empty() || text.split('/').any(|segment| matches!(segment, "" | "." | "..")) {
        return Err(StatusError::InvalidStatus);
    }
    let native = Path::new(text);
    if native.components().any(|component| !matches!(component, Component::Normal(_))) {
        return Err(StatusError::InvalidStatus);
    }
    if native.as_os_str().as_encoded_bytes() != bytes {
        return Err(StatusError::UnsupportedPathEncoding);
    }
    Ok(text)
}

fn status_path(bytes: &[u8]) -> Result<StatusPath, StatusError> {
    let text = validate_path(bytes)?;
    Ok(StatusPath {
        native_path: PathBuf::from(text),
        native_origin: None,
        display_path: text.to_owned(),
        segments: text.split('/').map(str::to_owned).collect(),
        staged: None,
        unstaged: None,
        untracked: false,
        conflict: false,
        unsupported_kind: None,
        modes: None,
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/status.rs"]
mod unit_tests;

#[cfg(test)]
#[path = "../../../tests/integration/status.rs"]
mod integration_tests;
