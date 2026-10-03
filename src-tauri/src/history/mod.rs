//! Owns bounded pinned history snapshots and opaque continuation authority.
//!
//! Renderer cursors cannot become revisions, offsets, paths or Git arguments.
//! Working-tree observations never invalidate an otherwise current history snapshot.

use std::{sync::Arc, time::Duration};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::time::Instant;
use uuid::Uuid;
use crate::diagnostic_operation::OperationTrace;
use crate::diagnostics::{Component, OperationContext};
use crate::git::process::GitProcess;
use crate::workspace::SelectedContext;

pub(crate) mod reader;

const SNAPSHOT_IDLE_TTL: Duration = Duration::from_secs(300);
pub(crate) const PAGE_SIZE: usize = 100;
const MAX_OFFSET: usize = 100_000;

/// Availability is independent of topology: absent page rows never erase a real parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentState { Loaded, OutsidePage, Unavailable }
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryParent { pub oid: String, pub state: ParentState }
/// Parents retain raw Git header order; root is true only when the object has no parent headers.
/// A subject is omitted when its declared encoding or bounded display text is unsupported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryCommit { pub oid: String, pub subject: Option<String>, pub parents: Vec<HistoryParent>, pub root: bool }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RefKind { LocalBranch, RemoteTracking, Tag }
/// A locally known label pinned to a commit; annotated tags are peeled without following moving names.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRef { pub kind: RefKind, pub name: String, pub commit_oid: String }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadScope { Worktree, Repository }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadState { Attached, Detached, Unborn, Unresolved }
/// Bare HEAD belongs to the repository; working and linked trees retain their own HEAD scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryHead { pub scope: HeadScope, pub state: HeadState, pub branch: Option<String>, pub oid: Option<String> }
/// Complete means traversal ended locally; shallow or missing ancestry always takes precedence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness { Complete, Paged, ShallowOrMissing }

/// Refs and HEAD are immutable across cursor pages; only an explicit first-page read refreshes them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub entry_id: String,
    pub cursor: Option<String>,
    pub commits: Vec<HistoryCommit>,
    pub refs: Vec<HistoryRef>,
    pub head: HistoryHead,
    pub has_more: bool,
    pub completeness: Completeness,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryErrorCode { Inaccessible, GitUnavailable, UnsafeRepository, Timeout, InvalidOutput, ResourceLimit, StaleSelection, StaleCursor, MissingObjects }

/// Failures never contain a native path, reference, Git stderr or raw storage error.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryPageResult {
    Page { page: HistoryPage },
    Unavailable { code: HistoryErrorCode, message: &'static str },
    Error { code: HistoryErrorCode, message: &'static str },
}
impl HistoryErrorCode {
    pub(crate) fn result(self) -> HistoryPageResult {
        let message = match self {
            Self::Inaccessible => "History cannot be read at this location.",
            Self::GitUnavailable => "Installed Git could not be started.",
            Self::UnsafeRepository => "Git refused to read this repository because of its ownership.",
            Self::Timeout => "Git took too long to read history.",
            Self::InvalidOutput => "Git returned an invalid history response.",
            Self::ResourceLimit => "History exceeds the bounded read limits.",
            Self::StaleSelection => "The selected repository changed.",
            Self::StaleCursor => "This history continuation has expired. Refresh history.",
            Self::MissingObjects => "History objects are unavailable locally.",
        };
        if matches!(self, Self::InvalidOutput | Self::ResourceLimit) { HistoryPageResult::Error { code: self, message } }
        else { HistoryPageResult::Unavailable { code: self, message } }
    }
}

struct PinnedSnapshot { refs: Vec<HistoryRef>, head: HistoryHead, seeds: Vec<String>, shallow: bool }
struct Continuation { token: String, offset: usize }
#[derive(Default)]
struct HistoryState {
    epoch: u64,
    entry_id: String,
    selection_generation: u64,
    snapshot: Option<Arc<PinnedSnapshot>>,
    branch: Option<String>,
    continuation: Option<Continuation>,
    expires_at: Option<Instant>,
}
#[derive(Default)]
pub(crate) struct HistoryController { state: Mutex<HistoryState> }
pub(crate) struct HistoryTicket {
    epoch: u64,
    entry_id: String,
    selection_generation: u64,
    cursor: Option<String>,
    offset: usize,
    branch: Option<String>,
    snapshot: Option<Arc<PinnedSnapshot>>,
}
pub(crate) struct HistoryCandidate { snapshot: Arc<PinnedSnapshot>, page: HistoryPage }

impl HistoryController {
    pub(crate) fn begin(&self, entry_id: &str, selection_generation: u64, cursor: Option<&str>, branch: Option<&str>) -> Result<HistoryTicket, HistoryErrorCode> {
        let mut state = self.state.lock();
        let offset = if let Some(cursor) = cursor {
            if cursor.len() != 36 || state.entry_id != entry_id || state.selection_generation != selection_generation || state.branch.as_deref() != branch
                || state.expires_at.is_none_or(|deadline| Instant::now() >= deadline) {
                return Err(HistoryErrorCode::StaleCursor);
            }
            let continuation = state.continuation.as_ref().filter(|next| next.token == cursor).ok_or(HistoryErrorCode::StaleCursor)?;
            if continuation.offset > MAX_OFFSET { return Err(HistoryErrorCode::ResourceLimit); }
            continuation.offset
        } else {
            state.epoch += 1;
            state.entry_id = entry_id.to_owned();
            state.selection_generation = selection_generation;
            state.branch = branch.map(str::to_owned);
            state.snapshot = None;
            state.continuation = None;
            state.expires_at = None;
            0
        };
        Ok(HistoryTicket { epoch: state.epoch, entry_id: entry_id.to_owned(), selection_generation,
            cursor: cursor.map(str::to_owned), branch: branch.map(str::to_owned), offset, snapshot: state.snapshot.clone() })
    }

    /// Publication checks the cursor again, rejecting duplicate or superseded completions.
    pub(crate) fn publish(&self, ticket: HistoryTicket, mut candidate: HistoryCandidate) -> HistoryPageResult {
        let mut state = self.state.lock();
        if state.epoch != ticket.epoch || state.entry_id != ticket.entry_id || state.selection_generation != ticket.selection_generation
            || ticket.cursor.as_deref() != state.continuation.as_ref().map(|next| next.token.as_str()) {
            return HistoryErrorCode::StaleCursor.result();
        }
        if ticket.cursor.is_some() && state.expires_at.is_none_or(|deadline| Instant::now() >= deadline) {
            return HistoryErrorCode::StaleCursor.result();
        }
        state.continuation = candidate.page.has_more.then(|| Continuation { token: Uuid::new_v4().to_string(), offset: ticket.offset + PAGE_SIZE });
        candidate.page.cursor = state.continuation.as_ref().map(|next| next.token.clone());
        state.snapshot = Some(candidate.snapshot);
        state.expires_at = Some(Instant::now() + SNAPSHOT_IDLE_TTL);
        HistoryPageResult::Page { page: candidate.page }
    }
}

pub(crate) async fn read_page(process: &GitProcess, context: &SelectedContext, ticket: &HistoryTicket) -> Result<HistoryCandidate, HistoryErrorCode> {
    let mut trace = OperationContext::current().map(|context| OperationTrace::new(context, Component::Git));
    let result = reader::read_page(process, context, ticket).await;
    if let Some(trace) = &mut trace {
        match &result {
            Ok(_) => trace.finish(crate::diagnostics::Event::Completed, None, Default::default()),
            Err(error) => trace.outcome(&error.result()),
        }
    }
    result
}

#[cfg(test)]
#[path = "../../tests/unit/history.rs"]
mod unit_tests;
#[cfg(all(test, unix))]
#[path = "../../tests/integration/history.rs"]
mod integration_tests;
