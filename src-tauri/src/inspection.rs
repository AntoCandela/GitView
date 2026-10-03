//! Discovers bounded native worktree choices and reads parent-specific committed-file lists.
//!
//! Issued worktree IDs retain verified native identity; displayed paths never authorize I/O.

use std::{collections::HashSet, path::PathBuf};
use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;
use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component, Event, OperationContext};
use crate::{git::{GitProbe, RepositoryKind}, history::{reader, HistoryErrorCode, HistoryPageResult, RefKind}, git::process::{GitProcess, ProbeDeadline}, workspace::{NativeIdentity, SelectedContext}};

const MAX_WORKTREES: usize = 64;
pub(crate) const MAX_FILES: usize = 20_000;
const ENCODING_MESSAGE: &str = "Paths with unsupported text encoding cannot be displayed.";

#[derive(Clone, Debug, Serialize)]
pub struct BranchOption { pub name: String }
#[derive(Clone, Debug, Serialize)]
pub struct WorktreeOption { pub id: String, pub label: String, pub branch: Option<String>, pub current: bool }
/// Options contain display-only labels and opaque worktree authority, never selectable native paths.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextOptionsResult {
    Options { branches: Vec<BranchOption>, worktrees: Vec<WorktreeOption> },
    Unavailable { code: HistoryErrorCode, message: &'static str },
    Error { code: HistoryErrorCode, message: &'static str },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommittedFileKind { Added, Modified, Deleted, TypeChange }
/// Paths and segments are display data; only the service-issued ID authorizes committed review.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommittedFile { pub id: String, pub display_path: String, pub segments: Vec<String>, pub kind: CommittedFileKind }
/// Null parent selects the first raw parent, or the empty tree only for a verified root.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitFilesResult {
    Files { #[serde(rename = "commitOid")] commit_oid: String, #[serde(rename = "parentOid")] parent_oid: Option<String>, parents: Vec<String>, files: Vec<CommittedFile> },
    Unavailable { code: HistoryErrorCode, message: &'static str },
    Error { code: HistoryErrorCode, message: &'static str },
}
macro_rules! inspection_failure {
    ($result:ident) => {
        impl $result {
            pub(crate) fn failure(code: HistoryErrorCode) -> Self {
                match code.result() {
                    HistoryPageResult::Unavailable { code, message } => Self::Unavailable { code, message },
                    HistoryPageResult::Error { code, message } => Self::Error { code, message },
                    HistoryPageResult::Page { .. } => unreachable!(),
                }
            }
        }
    }
}
inspection_failure!(ContextOptionsResult);
inspection_failure!(CommitFilesResult);

#[derive(Clone)]
pub(crate) struct WorktreeAuthority { pub(crate) option: WorktreeOption, pub(crate) root: PathBuf, pub(crate) git_dir: PathBuf, pub(crate) identity: NativeIdentity, pub(crate) kind: RepositoryKind }
#[derive(Default)]
struct ContextState { epoch: u64, selection_epoch: u64, entry_id: String, generation: u64, worktrees: Vec<WorktreeAuthority> }
#[derive(Default)]
pub(crate) struct ContextController { state: Mutex<ContextState> }
impl ContextController {
    pub(crate) fn begin(&self) -> u64 { let mut state = self.state.lock(); state.epoch += 1; state.epoch }
    pub(crate) fn publish(&self, epoch: u64, entry_id: &str, generation: u64, branches: Vec<BranchOption>, worktrees: Vec<WorktreeAuthority>) -> ContextOptionsResult {
        let mut state = self.state.lock();
        if state.epoch != epoch { return ContextOptionsResult::failure(HistoryErrorCode::StaleSelection); }
        let options = worktrees.iter().map(|worktree| worktree.option.clone()).collect();
        state.entry_id = entry_id.to_owned(); state.generation = generation; state.worktrees = worktrees;
        ContextOptionsResult::Options { branches, worktrees: options }
    }
    pub(crate) fn resolve(&self, entry_id: &str, generation: u64, id: &str) -> Option<(u64, WorktreeAuthority)> {
        let mut state = self.state.lock();
        if state.entry_id != entry_id || state.generation != generation { return None; }
        let authority = state.worktrees.iter().find(|worktree| worktree.option.id == id)?.clone();
        state.selection_epoch += 1;
        Some((state.selection_epoch, authority))
    }
    pub(crate) fn selection_current(&self, epoch: u64) -> bool { self.state.lock().selection_epoch == epoch }
}

pub(crate) async fn read_contexts(process: &GitProcess, probe: &GitProbe, context: &SelectedContext) -> Result<(Vec<BranchOption>, Vec<WorktreeAuthority>), HistoryErrorCode> {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
    let result = read_native_contexts(process, probe, context).await;
    if let Some(trace) = &mut trace {
        match &result {
            Ok(_) => trace.finish(Event::Completed, None, Default::default()),
            Err(code) => trace.outcome(&code.result()),
        }
    }
    result
}

async fn read_native_contexts(process: &GitProcess, probe: &GitProbe, context: &SelectedContext) -> Result<(Vec<BranchOption>, Vec<WorktreeAuthority>), HistoryErrorCode> {
    let deadline = ProbeDeadline::new();
    reader::verify_context(process, context, deadline).await?;
    let source_common = common_git_identity(process, context, deadline).await?;
    let refs = reader::required(process, &context.root, &["for-each-ref", "--count=1025", "--format=%(refname)%00%(objectname)%00%(objecttype)%00", "refs/heads/"], None, deadline).await?;
    let branches = reader::parse_refs(&refs)?.into_iter().filter(|reference| reference.kind == RefKind::LocalBranch).map(|reference| BranchOption { name: reference.name }).collect();
    let output = reader::required(process, &context.root, &["worktree", "list", "--porcelain", "-z"], None, deadline).await?;
    let records = parse_worktrees(&output)?;
    let mut worktrees = Vec::with_capacity(records.len());
    for (root, branch) in records {
        // A stale registration can point at an unrelated repository or one of its subdirectories.
        let listed_root = match std::fs::canonicalize(&root) {
            Ok(root) => root,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(HistoryErrorCode::Inaccessible),
        };
        let facts = probe.probe_with_deadline(&root, deadline).await.map_err(probe_error)?;
        if facts.root != listed_root { continue; }
        let identity = NativeIdentity::capture(&facts.root, &facts.git_dir).map_err(|_| HistoryErrorCode::Inaccessible)?;
        let label = facts.root.file_name().and_then(|name| name.to_str()).unwrap_or(&facts.repository_label).to_owned();
        let target = SelectedContext { entry_id: String::new(), root: facts.root, git_dir: facts.git_dir, identity, kind: facts.kind };
        reader::verify_context(process, &target, deadline).await?;
        let target_common = common_git_identity(process, &target, deadline).await?;
        reader::verify_context(process, &target, deadline).await?;
        if target_common != source_common { continue; }
        worktrees.push(WorktreeAuthority { option: WorktreeOption { id: Uuid::new_v4().to_string(), label, branch, current: target.root == context.root && target.git_dir == context.git_dir }, root: target.root, git_dir: target.git_dir, identity: target.identity, kind: target.kind });
    }
    if common_git_identity(process, context, deadline).await? != source_common { return Err(HistoryErrorCode::Inaccessible); }
    reader::verify_context(process, context, deadline).await?;
    Ok((branches, worktrees))
}

async fn common_git_identity(process: &GitProcess, context: &SelectedContext, deadline: ProbeDeadline) -> Result<(PathBuf, crate::workspace::DirectoryIdentity), HistoryErrorCode> {
    let output = reader::required(process, &context.root, &["rev-parse", "--path-format=absolute", "--git-common-dir"], None, deadline).await?;
    let bytes = output.strip_suffix(b"\n").ok_or(HistoryErrorCode::InvalidOutput)?;
    let common = PathBuf::from(std::str::from_utf8(bytes).map_err(|_| HistoryErrorCode::InvalidOutput)?);
    if !common.is_absolute() || bytes.contains(&0) { return Err(HistoryErrorCode::InvalidOutput); }
    let root = context.root.clone();
    let task = tokio::task::spawn_blocking(move || {
        let common = std::fs::canonicalize(common).map_err(|_| HistoryErrorCode::Inaccessible)?;
        let identity = NativeIdentity::capture(&root, &common).map_err(|_| HistoryErrorCode::Inaccessible)?;
        Ok::<_, HistoryErrorCode>((common, identity))
    });
    let (common, identity) = tokio::time::timeout_at(deadline.instant(), task).await.map_err(|_| HistoryErrorCode::Timeout)?
        .map_err(|_| HistoryErrorCode::Inaccessible)??;
    if identity.root != context.identity.root { return Err(HistoryErrorCode::Inaccessible); }
    Ok((common, identity.git_dir))
}

fn parse_worktrees(bytes: &[u8]) -> Result<Vec<(PathBuf, Option<String>)>, HistoryErrorCode> {
    if bytes.is_empty() { return Err(HistoryErrorCode::InvalidOutput); }
    let body = bytes.strip_suffix(b"\0\0").ok_or(HistoryErrorCode::InvalidOutput)?;
    let mut records = Vec::new();
    let mut fields = body.split(|byte| *byte == 0).peekable();
    let mut seen = HashSet::new();
    while let Some(field) = fields.next() {
        if records.len() == MAX_WORKTREES { return Err(HistoryErrorCode::ResourceLimit); }
        let root = field.strip_prefix(b"worktree ").ok_or(HistoryErrorCode::InvalidOutput)?;
        let root = PathBuf::from(std::str::from_utf8(root).map_err(|_| HistoryErrorCode::InvalidOutput)?);
        if !root.is_absolute() || !seen.insert(root.clone()) { return Err(HistoryErrorCode::InvalidOutput); }
        let mut branch = None;
        while let Some(field) = fields.next() {
            if field.is_empty() { break; }
            if let Some(value) = field.strip_prefix(b"branch ") {
                let value = std::str::from_utf8(value).map_err(|_| HistoryErrorCode::InvalidOutput)?;
                if branch.is_some() || !reader::valid_ref_name(value) { return Err(HistoryErrorCode::InvalidOutput); }
                branch = Some(value.strip_prefix("refs/heads/").ok_or(HistoryErrorCode::InvalidOutput)?.to_owned());
            } else if let Some(value) = field.strip_prefix(b"HEAD ") { reader::oid(value)?; }
            else if field != b"bare" && field != b"detached" && field != b"locked" && field != b"prunable" && !field.starts_with(b"locked ") && !field.starts_with(b"prunable ") { return Err(HistoryErrorCode::InvalidOutput); }
        }
        records.push((root, branch));
    }
    Ok(records)
}

pub(crate) async fn read_commit_files(process: &GitProcess, context: &SelectedContext, commit_oid: &str, parent_oid: Option<&str>) -> CommitFilesResult {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
    let result = read_files(process, context, commit_oid, parent_oid).await;
    let result = match result {
        Ok(result) => result,
        Err(FileReadError::Code(code)) => CommitFilesResult::failure(code),
        Err(FileReadError::Encoding) => CommitFilesResult::Error { code: HistoryErrorCode::InvalidOutput, message: ENCODING_MESSAGE },
    };
    if let Some(trace) = &mut trace { trace.outcome(&result); }
    result
}
enum FileReadError { Code(HistoryErrorCode), Encoding }
impl From<HistoryErrorCode> for FileReadError { fn from(code: HistoryErrorCode) -> Self { Self::Code(code) } }
async fn read_files(process: &GitProcess, context: &SelectedContext, commit_oid: &str, parent_oid: Option<&str>) -> Result<CommitFilesResult, FileReadError> {
    reader::oid(commit_oid.as_bytes())?;
    if let Some(parent) = parent_oid { reader::oid(parent.as_bytes())?; }
    let deadline = ProbeDeadline::new();
    reader::verify_context(process, context, deadline).await?;
    let requested = vec![commit_oid.to_owned()];
    let input = format!("{commit_oid}\n");
    let raw = reader::required(process, &context.root, &["cat-file", "--batch"], Some(input.as_bytes()), deadline).await?;
    let commit = reader::parse_batch(&raw, &requested)?.remove(0);
    let parents: Vec<_> = commit.parents.into_iter().map(|parent| parent.oid).collect();
    let parent = match parent_oid {
        Some(parent) if parents.iter().any(|known| known == parent) => Some(parent),
        Some(_) => return Err(HistoryErrorCode::InvalidOutput.into()),
        None => parents.first().map(String::as_str),
    };
    if let Some(parent) = parent {
        // Raw parent verification prevents shallow history from masquerading as an empty-tree diff.
        let input = format!("{parent}\n");
        let raw = reader::required(process, &context.root, &["cat-file", "--batch"], Some(input.as_bytes()), deadline).await?;
        reader::parse_batch(&raw, &[parent.to_owned()])?;
    }
    let fixed = ["diff-tree", "--no-commit-id", "--name-status", "-z", "-r", "--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=none"];
    let mut args = Vec::with_capacity(fixed.len() + 4); args.extend_from_slice(&fixed);
    if let Some(parent) = parent { args.extend_from_slice(&[parent, commit_oid, "--"]); }
    else { args.extend_from_slice(&["--root", commit_oid, "--"]); }
    let bytes = reader::required(process, &context.root, &args, None, deadline).await?;
    let files = parse_files(&bytes)?;
    reader::verify_context(process, context, deadline).await?;
    Ok(CommitFilesResult::Files { commit_oid: commit_oid.to_owned(), parent_oid: parent.map(str::to_owned), parents, files })
}
fn parse_files(bytes: &[u8]) -> Result<Vec<CommittedFile>, FileReadError> {
    if bytes.is_empty() { return Ok(Vec::new()); }
    let bytes = bytes.strip_suffix(b"\0").ok_or(HistoryErrorCode::InvalidOutput)?;
    let mut fields = bytes.split(|byte| *byte == 0);
    let mut files = Vec::new(); let mut seen = HashSet::new();
    while let Some(status) = fields.next() {
        if files.len() == MAX_FILES { return Err(HistoryErrorCode::ResourceLimit.into()); }
        let kind = match status { b"A" => CommittedFileKind::Added, b"M" => CommittedFileKind::Modified, b"D" => CommittedFileKind::Deleted, b"T" => CommittedFileKind::TypeChange, _ => return Err(HistoryErrorCode::InvalidOutput.into()) };
        let path = fields.next().ok_or(HistoryErrorCode::InvalidOutput)?;
        let path = std::str::from_utf8(path).map_err(|_| FileReadError::Encoding)?;
        if path.is_empty() || path.starts_with('/') || !seen.insert(path) { return Err(HistoryErrorCode::InvalidOutput.into()); }
        let segments: Vec<String> = path.split('/').map(str::to_owned).collect();
        if segments.iter().any(|segment| segment.is_empty() || segment == "." || segment == "..") { return Err(HistoryErrorCode::InvalidOutput.into()); }
        files.push(CommittedFile { id: String::new(), display_path: path.to_owned(), segments, kind });
    }
    Ok(files)
}

fn probe_error(error: crate::git::GitError) -> HistoryErrorCode {
    use crate::git::GitError;
    match error {
        GitError::GitUnavailable => HistoryErrorCode::GitUnavailable,
        GitError::UnsafeRepository => HistoryErrorCode::UnsafeRepository,
        GitError::ProbeTimeout => HistoryErrorCode::Timeout,
        GitError::UnsupportedPathEncoding => HistoryErrorCode::InvalidOutput,
        _ => HistoryErrorCode::Inaccessible,
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/integration/inspection.rs"]
mod integration_tests;
