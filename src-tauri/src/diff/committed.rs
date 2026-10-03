//! Authorizes bounded parent-to-commit previews and reads only immutable Git objects.
//! Selection generations bind opaque file IDs; display paths never enter IPC as authority.

use std::collections::{HashMap, VecDeque};
use parking_lot::Mutex;
use serde::Serialize;
use uuid::Uuid;
use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component, OperationContext};
use crate::diff::{self, ReviewErrorCode, ReviewFailure, TextContent, TextHunk, UnsupportedReason};
use crate::history::{reader, HistoryErrorCode};
use crate::inspection::{CommitFilesResult, CommittedFileKind, MAX_FILES};
use crate::git::process::{GitProcess, ProbeDeadline};
use crate::workspace::SelectedContext;

const MAX_COMPARISONS: usize = 64;

/// Historical endpoints are pinned object IDs, independent of live status and its revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitReviewIdentity {
    pub entry_id: String,
    pub file_id: String,
    pub commit_oid: String,
    pub parent_oid: Option<String>,
    pub display_path: String,
    pub context_label: String,
    pub from_absent: bool,
    pub to_absent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitReviewResult {
    Text {
        #[serde(flatten)] identity: CommitReviewIdentity,
        hunks: Vec<TextHunk>,
        #[serde(rename = "fromContent")] from_content: String,
        #[serde(rename = "toContent")] to_content: String,
    },
    Unsupported { reason: UnsupportedReason, identity: CommitReviewIdentity },
    Unavailable { code: ReviewErrorCode, identity: CommitReviewIdentity },
    StaleSelection,
}

struct FileAuthority { display_path: String, kind: CommittedFileKind }
struct ComparisonAuthority {
    commit_oid: String,
    parent_oid: Option<String>,
    files: HashMap<String, FileAuthority>,
}
#[derive(Default)]
struct AuthorityState {
    entry_id: String,
    generation: u64,
    comparisons: VecDeque<ComparisonAuthority>,
}
#[derive(Default)]
pub(crate) struct CommitReviewController { state: Mutex<AuthorityState> }
impl CommitReviewController {
    /// Publication is called under the application's selection lock after its final generation guard.
    pub(crate) fn publish(&self, entry_id: &str, generation: u64, mut result: CommitFilesResult) -> CommitFilesResult {
        let CommitFilesResult::Files { commit_oid, parent_oid, files, .. } = &mut result else { return result; };
        let mut state = self.state.lock();
        if state.entry_id != entry_id || state.generation != generation {
            state.entry_id = entry_id.to_owned();
            state.generation = generation;
            state.comparisons.clear();
        }
        // Repeated expansion replaces a comparison, retaining its IDs instead of leaking tokens.
        let previous = state.comparisons.iter().position(|comparison| comparison.commit_oid == *commit_oid && comparison.parent_oid == *parent_oid)
            .and_then(|index| state.comparisons.remove(index));
        let previous: HashMap<_, _> = previous.into_iter().flat_map(|comparison| comparison.files)
            .map(|(id, file)| (file.display_path, (id, file.kind))).collect();
        for file in files.iter_mut() {
            file.id = previous.get(&file.display_path).filter(|(_, kind)| *kind == file.kind)
                .map(|(id, _)| id.clone()).unwrap_or_else(|| Uuid::new_v4().to_string());
        }
        while state.comparisons.len() >= MAX_COMPARISONS
            || state.comparisons.iter().map(|comparison| comparison.files.len()).sum::<usize>() + files.len() > MAX_FILES {
            state.comparisons.pop_front();
        }
        state.comparisons.push_back(ComparisonAuthority {
            commit_oid: commit_oid.clone(), parent_oid: parent_oid.clone(),
            files: files.iter().map(|file| (file.id.clone(), FileAuthority { display_path: file.display_path.clone(), kind: file.kind })).collect(),
        });
        result
    }

    pub(crate) fn resolve(&self, context: &SelectedContext, generation: u64, commit_oid: &str, parent_oid: Option<&str>, file_id: &str) -> Option<(CommitReviewIdentity, CommittedFileKind)> {
        let state = self.state.lock();
        if state.entry_id != context.entry_id || state.generation != generation { return None; }
        let comparison = state.comparisons.iter().find(|comparison| comparison.commit_oid == commit_oid && comparison.parent_oid.as_deref() == parent_oid)?;
        let file = comparison.files.get(file_id)?;
        Some((CommitReviewIdentity {
            entry_id: context.entry_id.clone(), file_id: file_id.to_owned(), commit_oid: comparison.commit_oid.clone(),
            parent_oid: comparison.parent_oid.clone(), display_path: file.display_path.clone(),
            context_label: context.root.file_name().and_then(|name| name.to_str()).unwrap_or("Repository").to_owned(),
            from_absent: file.kind == CommittedFileKind::Added, to_absent: file.kind == CommittedFileKind::Deleted,
        }, file.kind))
    }
}

pub(crate) async fn read_review(process: &GitProcess, context: &SelectedContext, identity: CommitReviewIdentity, kind: CommittedFileKind) -> CommitReviewResult {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
    let result = match read_native(process, context, &identity, kind).await {
        Ok(TextContent { hunks, from_content, to_content }) => CommitReviewResult::Text { identity, hunks, from_content, to_content },
        Err(ReviewFailure::Unsupported(reason)) => CommitReviewResult::Unsupported { reason, identity },
        Err(ReviewFailure::Unavailable(code)) => CommitReviewResult::Unavailable { code, identity },
    };
    if let Some(trace) = &mut trace { trace.outcome(&result); }
    result
}

async fn read_native(process: &GitProcess, context: &SelectedContext, identity: &CommitReviewIdentity, kind: CommittedFileKind) -> Result<TextContent, ReviewFailure> {
    let deadline = ProbeDeadline::new();
    reader::verify_context(process, context, deadline).await.map_err(history_error)?;
    let result = read_endpoints(process, context, identity, kind, deadline).await;
    reader::verify_context(process, context, deadline).await.map_err(history_error)?;
    result
}

async fn read_endpoints(process: &GitProcess, context: &SelectedContext, identity: &CommitReviewIdentity, kind: CommittedFileKind, deadline: ProbeDeadline) -> Result<TextContent, ReviewFailure> {
    let from = match &identity.parent_oid {
        Some(parent) => tree_entry(process, context, parent, &identity.display_path, deadline).await?,
        None => None,
    };
    let to = tree_entry(process, context, &identity.commit_oid, &identity.display_path, deadline).await?;
    if from.is_none() != identity.from_absent || to.is_none() != identity.to_absent {
        return Err(diff::unavailable(ReviewErrorCode::InvalidOutput));
    }
    let modes = [&from, &to];
    if modes.iter().filter_map(|entry| entry.as_ref()).any(|entry| entry.0 == 0o160000) { return Err(diff::unsupported(UnsupportedReason::Submodule)); }
    if kind == CommittedFileKind::TypeChange || modes.iter().filter_map(|entry| entry.as_ref()).any(|entry| !matches!(entry.0, 0o100644 | 0o100755)) {
        return Err(diff::unsupported(UnsupportedReason::TypeChange));
    }
    if let (Some(from), Some(to)) = (&from, &to) {
        if from.0 != to.0 { return Err(diff::unsupported(UnsupportedReason::Other)); }
    }
    let from = object_bytes(process, context, from, deadline).await?;
    let to = object_bytes(process, context, to, deadline).await?;
    let (hunks, endpoints) = diff::compare(process, (from, to), deadline).await?;
    TextContent::from_snapshots(hunks, endpoints)
}

async fn tree_entry(process: &GitProcess, context: &SelectedContext, revision: &str, path: &str, deadline: ProbeDeadline) -> Result<Option<(u32, String)>, ReviewFailure> {
    let output = reader::required(process, &context.root, &["--literal-pathspecs", "ls-tree", "-z", revision, "--", path], None, deadline).await.map_err(history_error)?;
    if output.is_empty() { return Ok(None); }
    let invalid = || diff::unavailable(ReviewErrorCode::InvalidOutput);
    let record = output.strip_suffix(b"\0").ok_or_else(invalid)?;
    let tab = record.iter().position(|byte| *byte == b'\t').ok_or_else(invalid)?;
    if &record[tab + 1..] != path.as_bytes() { return Err(invalid()); }
    let header = std::str::from_utf8(&record[..tab]).map_err(|_| invalid())?;
    let mut fields = header.split(' ');
    let mode = u32::from_str_radix(fields.next().ok_or_else(invalid)?, 8).map_err(|_| invalid())?;
    let object_type = fields.next().ok_or_else(invalid)?;
    let oid = fields.next().ok_or_else(invalid)?;
    reader::oid(oid.as_bytes()).map_err(history_error)?;
    if fields.next().is_some() || !matches!((mode, object_type), (0o100644 | 0o100755 | 0o120000, "blob") | (0o160000, "commit")) { return Err(invalid()); }
    Ok(Some((mode, oid.to_owned())))
}

async fn object_bytes(process: &GitProcess, context: &SelectedContext, entry: Option<(u32, String)>, deadline: ProbeDeadline) -> Result<Vec<u8>, ReviewFailure> {
    let Some((_, oid)) = entry else { return Ok(Vec::new()); };
    let size = reader::required(process, &context.root, &["cat-file", "-s", &oid], None, deadline).await.map_err(history_error)?;
    let size = std::str::from_utf8(&size).ok().and_then(|size| size.strip_suffix('\n')).and_then(|size| size.parse::<usize>().ok())
        .ok_or(diff::unavailable(ReviewErrorCode::InvalidOutput))?;
    if size > diff::CONTENT_LIMIT { return Err(diff::unsupported(UnsupportedReason::LargeOrTruncated)); }
    let bytes = reader::required(process, &context.root, &["cat-file", "blob", &oid], None, deadline).await.map_err(history_error)?;
    if bytes.len() != size { return Err(diff::unavailable(ReviewErrorCode::InvalidOutput)); }
    diff::validate_content(&bytes)?;
    Ok(bytes)
}

fn history_error(code: HistoryErrorCode) -> ReviewFailure {
    match code {
        HistoryErrorCode::GitUnavailable => diff::unavailable(ReviewErrorCode::GitUnavailable),
        HistoryErrorCode::UnsafeRepository => diff::unavailable(ReviewErrorCode::UnsafeRepository),
        HistoryErrorCode::Timeout => diff::unavailable(ReviewErrorCode::Timeout),
        HistoryErrorCode::ResourceLimit => diff::unsupported(UnsupportedReason::LargeOrTruncated),
        HistoryErrorCode::InvalidOutput => diff::unavailable(ReviewErrorCode::InvalidOutput),
        _ => diff::unavailable(ReviewErrorCode::Inaccessible),
    }
}

#[cfg(all(test, unix))]
#[path = "../../tests/integration/committed_review.rs"]
mod integration_tests;
