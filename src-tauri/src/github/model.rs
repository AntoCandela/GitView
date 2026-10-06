//! Closed PR request and presentation contracts; opaque IDs carry authority, display strings do not.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrCode {
    IntegrationUnavailable, GhMissing, GhUnsupported, AuthRequired, AuthUnavailable, AccessDenied,
    RepositoryUnavailable, Network, RateLimited, Timeout, ResourceLimit, InvalidOutput, UnresolvedMapping,
    MissingObjects, UnsupportedComparison, StaleContext, StaleCursor, ChangedSnapshot,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure { pub kind: FailureKind, pub code: PrCode, #[serde(skip_serializing_if = "Option::is_none")] pub retry_at: Option<u64> }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind { Unavailable, Error, Stale }
impl PrCode {
    pub fn failure(self) -> Failure {
        Failure { kind: match self {
            Self::StaleContext | Self::StaleCursor | Self::ChangedSnapshot => FailureKind::Stale,
            Self::InvalidOutput | Self::ResourceLimit => FailureKind::Error,
            _ => FailureKind::Unavailable,
        }, code: self, retry_at: None }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionKind { Commits, Timeline, Threads, ThreadComments, Reviewers, Labels }
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum ComparisonSelection { Aggregate {}, Commit { commit_id: String, parent_index: Option<u32> } }

/// Constructed by fixed host commands, never decoded as a renderer-supplied command name.
#[derive(Clone, Debug)]
pub enum PrRequest {
    Status,
    Associations { branch: Option<String> },
    MapHead { association_id: String, owner: String, repository: String, head_ref: String },
    Choose { association_id: String, candidate_id: String },
    Open { pr_id: String },
    Page { session_id: String, collection: CollectionKind, cursor: Option<String>, thread_id: Option<String> },
    Refresh { session_id: String },
    Compare { session_id: String, selection: ComparisonSelection },
    FilesPage { comparison_id: String, cursor: String },
    ResolveAnchor { session_id: String, anchor_id: String },
    File { comparison_id: String, file_id: String },
    Release { session_id: String },
    OpenLink { session_id: String, link_id: String },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection<T> {
    pub items: Vec<T>, pub total_count: Option<u64>, pub next_cursor: Option<String>, pub completeness: Completeness,
    #[serde(skip_serializing_if = "Option::is_none")] pub limit_reason: Option<LimitReason>, pub observed_revision: u64,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness { Complete, More, Limited }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitReason { Provider, Resource }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepository { pub id: String, pub host: GithubHost, pub owner: String, pub name: String, pub url: String }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum GithubHost { #[serde(rename = "github.com")] GithubCom }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prose { Available { text: String }, Empty, Limited { text: String }, Unavailable { code: PrCode } }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person { pub provider_id: Option<String>, pub login: Option<String>, pub display_name: Option<String> }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Reviewer {
    User { actor: Person, requested: bool, submitted: Option<ReviewDecision> },
    Team { provider_id: String, name: String, slug: String, requested: bool },
}
#[derive(Clone, Debug, Serialize)]
pub struct Label { pub name: String, pub color: Option<String> }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle { Open, Closed, Merged }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision { Approved, ChangesRequested, ReviewRequired }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub number: u64, pub base_repository: GithubRepository, pub head_repository: Option<GithubRepository>,
    pub head_ref: Option<String>, pub head_oid: Option<String>, pub base_ref: String, pub base_oid: Option<String>,
    pub title: String, pub url: String, pub created_at: String, pub updated_at: String,
    pub closed_at: Option<String>, pub merged_at: Option<String>, pub counts: PrCounts, pub body: Prose, pub author: Option<Person>, pub lifecycle: Lifecycle, pub draft: bool,
    pub review_decision: Option<ReviewDecision>, pub reviewers: Collection<Reviewer>, pub labels: Collection<Label>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PrCounts { pub commits: Option<u64>, pub files: Option<u64>, pub additions: Option<u64>, pub deletions: Option<u64> }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness { Fresh, Stale }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionState { Available, Unavailable, NotLoaded }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sections {
    pub commits: SectionState, pub timeline: SectionState, pub threads: SectionState,
    pub thread_comments: SectionState, pub reviewers: SectionState, pub labels: SectionState,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub session_id: String, pub revision: u64, pub pr_id: String, pub overview: Overview, pub observed_at: u64,
    pub freshness: Freshness, pub availability: Option<Failure>, pub sections: Sections,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate { pub candidate_id: String, pub number: u64, pub title: String, pub base_repository: GithubRepository, pub base_ref: String, pub head_repository: Option<GithubRepository>, pub head_ref: Option<String>, pub lifecycle: Lifecycle }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssociationState { Single, None, Ambiguous, Unresolved, Unavailable }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Association { pub association_id: String, pub branch_label: Option<String>, pub state: AssociationState, pub candidates: Vec<Candidate>, pub historical: Vec<Candidate>, pub selected_candidate_id: Option<String>, pub complete: bool, pub base_repositories: Vec<GithubRepository>, pub head_mappings: Vec<AssociationHead>, pub failure: Option<Failure>, pub observed_at: u64, pub freshness: Freshness }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationHead { pub repository: GithubRepository, pub head_ref: String }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrFile { pub file_id: String, pub display_path: String, pub previous_display_path: Option<String>, pub kind: FileKind, pub additions: Option<u64>, pub deletions: Option<u64>, pub patch: Option<ProviderPatch> }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind { Added, Modified, Deleted, Renamed, Copied, TypeChange, Unknown }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonSource { LocalGit, GithubPatch }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonScope { Commit, Aggregate }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaseKind { Parent, EmptyTree, MergeBase, Provider }
#[derive(Clone, Debug, Serialize)]
pub struct ComparisonBase { pub kind: BaseKind, pub oid: Option<String> }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub comparison_id: String, pub session_id: String, pub revision: u64, pub source: ComparisonSource,
    pub scope: ComparisonScope, pub observed_at: u64, pub observed_head_oid: Option<String>, pub observed_base_oid: Option<String>, pub parent_oid: Option<String>, pub base: ComparisonBase, pub head_oid: Option<String>, pub files: Collection<PrFile>, pub full_content: bool,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Item {
    Commit { commit_id: String, oid: String, title: String, parent_count: u32, authored_at: Option<String>, committed_at: Option<String> },
    Timeline { event: TimelineItem }, Thread { thread: Thread }, Comment { comment: Comment },
    Reviewer { reviewer: Reviewer }, Label { label: Label },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem { pub id: String, pub kind: TimelineKind, pub occurred_at: Option<String>, pub actor: Option<Person>, pub provider_order: u64, pub details: TimelineDetails, pub link_id: Option<String> }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind { Opened, DraftChanged, Commit, ForcePushed, BaseChanged, ReviewRequested, ReviewRemoved, ReviewSubmitted, ReviewDismissed, Comment, Closed, Reopened, Merged, Unsupported }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum TimelineDetails {
    Activity { body: Prose }, Entity { entity_id: String },
    Commit { commit_oid: String, authored_at: Option<String>, committed_at: Option<String> },
    Review { review_id: String, state: String, body: Prose, thread_ids: Vec<String> },
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String, pub anchor_id: Option<String>, pub resolved: Option<bool>, pub outdated: Option<bool>, pub path: Option<String>,
    pub original_commit_oid: Option<String>, pub current_commit_oid: Option<String>, pub side: Option<Side>,
    pub start_line: Option<u32>, pub line: Option<u32>, pub diff_excerpt: Prose, pub comments: Collection<Comment>, pub link_id: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String, pub author: Option<Person>, pub created_at: Option<String>, pub updated_at: Option<String>,
    pub body: Prose, pub reply_to_id: Option<String>, pub source_kind: CommentSource,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentSource { Issue, Review, Thread }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum FileContent {
    Text { from_content: String, to_content: String, hunks: Vec<crate::diff::TextHunk> },
    Patch { patch: ProviderPatch }, Unsupported { code: PrCode },
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderPatch { Hunks { hunks: Vec<PatchHunk>, completeness: PatchCompleteness }, Unavailable { reason: PrCode } }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchCompleteness { ProviderExcerpt }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchHunk { pub old_start: u32, pub old_count: u32, pub new_start: u32, pub new_count: u32, pub rows: Vec<PatchRow> }
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchRow { pub kind: PatchRowKind, pub text: String, pub old_line: Option<u32>, pub new_line: Option<u32> }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchRowKind { Context, Add, Remove }
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side { Old, New }
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum PrSuccess {
    Ready, Association { #[serde(flatten)] observation: Association }, Chosen { pr_id: String },
    Snapshot { #[serde(flatten)] snapshot: Snapshot }, Page { collection: Collection<Item> },
    Comparison { #[serde(flatten)] comparison: Comparison }, Files { collection: Collection<PrFile> },
    Resolved { comparison: Comparison, file_id: String, side: Side, line: u32 },
    Fallback { excerpt: Prose, link_id: Option<String>, code: PrCode },
    File { comparison_id: String, file_id: String, content: FileContent }, Released, Opened, Blocked,
}
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum PrResult { Success(PrSuccess), Failure(Failure) }
impl From<PrCode> for PrResult { fn from(code: PrCode) -> Self { Self::Failure(code.failure()) } }
impl From<PrSuccess> for PrResult { fn from(value: PrSuccess) -> Self { Self::Success(value) } }

impl crate::diagnostic_operation::DiagnosticOutcome for PrResult {
    fn diagnostic_outcome(&self) -> (crate::diagnostics::Event, Option<crate::diagnostics::Code>) {
        use crate::diagnostics::{Event, Code};
        match self {
            Self::Success(_) => (Event::Completed, None),
            Self::Failure(failure) => match failure.kind {
                FailureKind::Stale => (Event::Superseded, Some(Code::PrStaleContext)),
                _ => (Event::Failed, Some(match failure.code {
                    PrCode::IntegrationUnavailable => Code::IntegrationUnavailable,
                    PrCode::ResourceLimit => Code::ResourceLimit,
                    PrCode::Timeout => Code::Timeout,
                    PrCode::InvalidOutput => Code::InvalidOutput,
                    _ => Code::PrUnavailable,
                })),
            },
        }
    }
}
