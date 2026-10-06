/** Read-only PR DTOs: opaque native handles authorize reads; provider labels and paths never do. */
export type PrCode = 'integration_unavailable' | 'gh_missing' | 'gh_unsupported' | 'auth_required' | 'auth_unavailable'
  | 'access_denied' | 'repository_unavailable' | 'network' | 'rate_limited' | 'timeout' | 'resource_limit'
  | 'invalid_output' | 'unresolved_mapping' | 'missing_objects' | 'unsupported_comparison'
  | 'stale_context' | 'stale_cursor' | 'changed_snapshot';
export interface PrFailure { kind: 'unavailable' | 'error' | 'stale'; code: PrCode; retryAt?: number }
export type PrCollectionKind = 'commits' | 'timeline' | 'threads' | 'thread_comments' | 'reviewers' | 'labels';
export interface PrCollection<T> {
  items: T[]; totalCount: number | null; nextCursor: string | null;
  completeness: 'complete' | 'more' | 'limited'; limitReason?: 'provider' | 'resource'; observedRevision: number;
}
export interface GithubRepository { id: string; host: 'github.com'; owner: string; name: string; url: string }
export type PrProse = { kind: 'available' | 'limited'; text: string } | { kind: 'empty' } | { kind: 'unavailable'; code: PrCode };
export interface PrPerson { providerId: string | null; login: string | null; displayName: string | null }
export type PrReviewer = { kind: 'user'; actor: PrPerson; requested: boolean; submitted: PrOverview['reviewDecision'] }
  | { kind: 'team'; providerId: string; name: string; slug: string; requested: boolean };
export interface PrLabel { name: string; color: string | null }
export interface PrOverview {
  number: number; baseRepository: GithubRepository; headRepository: GithubRepository | null;
  headRef: string | null; headOid: string | null; baseRef: string; baseOid: string | null;
  title: string; url: string; createdAt: string; updatedAt: string; closedAt: string | null; mergedAt: string | null;
  counts: { commits: number | null; files: number | null; additions: number | null; deletions: number | null }; body: PrProse; author: PrPerson | null; lifecycle: 'open' | 'closed' | 'merged';
  draft: boolean; reviewDecision: 'approved' | 'changes_requested' | 'review_required' | null;
  reviewers: PrCollection<PrReviewer>; labels: PrCollection<PrLabel>;
}
export interface PrSnapshot {
  sessionId: string; revision: number; prId: string; overview: PrOverview; observedAt: number;
  freshness: 'fresh' | 'stale'; availability: PrFailure | null;
  sections: Record<'commits' | 'timeline' | 'threads' | 'threadComments' | 'reviewers' | 'labels', 'available' | 'unavailable' | 'not_loaded'>;
}
export interface PrCandidate { candidateId: string; number: number; title: string; baseRepository: GithubRepository; baseRef: string; headRepository: GithubRepository | null; headRef: string | null; lifecycle: PrOverview["lifecycle"] }
export interface PrAssociation {
  associationId: string; branchLabel: string | null; state: 'single' | 'none' | 'ambiguous' | 'unresolved' | 'unavailable'; candidates: PrCandidate[];
  historical: PrCandidate[]; selectedCandidateId: string | null; complete: boolean; baseRepositories: GithubRepository[];
  headMappings: Array<{ repository: GithubRepository; headRef: string }>; failure: PrFailure | null; observedAt: number; freshness: 'fresh' | 'stale';
}
export type PrComparisonSelection = { kind: 'aggregate' } | { kind: 'commit'; commitId: string; parentIndex: number | null };
export interface PrFile {
  fileId: string; displayPath: string; previousDisplayPath: string | null;
  kind: 'added' | 'modified' | 'deleted' | 'renamed' | 'copied' | 'type_change' | 'unknown';
  additions: number | null; deletions: number | null; patch: ProviderPatch | null;
}
export interface PrComparison {
  comparisonId: string; sessionId: string; revision: number; source: 'local_git' | 'github_patch'; scope: 'commit' | 'aggregate';
  base: { kind: 'parent' | 'empty_tree' | 'merge_base' | 'provider'; oid: string | null };
  observedAt: number; observedHeadOid: string | null; observedBaseOid: string | null; parentOid: string | null;
  headOid: string | null; files: PrCollection<PrFile>; fullContent: boolean;
}
export type PrItem =
  | { kind: 'commit'; commitId: string; oid: string; title: string; parentCount: number; authoredAt: string | null; committedAt: string | null }
  | { kind: 'timeline'; event: PrTimelineItem }
  | { kind: 'thread'; thread: PrThread }
  | { kind: 'comment'; comment: PrComment }
  | { kind: 'reviewer'; reviewer: PrReviewer }
  | { kind: 'label'; label: PrLabel };
export interface PrTimelineItem {
  id: string; kind: 'opened' | 'draft_changed' | 'commit' | 'force_pushed' | 'base_changed' | 'review_requested' | 'review_removed'
    | 'review_submitted' | 'review_dismissed' | 'comment' | 'closed' | 'reopened' | 'merged' | 'unsupported';
  occurredAt: string | null; actor: PrPerson | null; providerOrder: number; linkId: string | null;
  details: { kind: 'activity'; body: PrProse } | { kind: 'entity'; entityId: string }
    | { kind: 'commit'; commitOid: string; authoredAt: string | null; committedAt: string | null }
    | { kind: 'review'; reviewId: string; state: string; body: PrProse; threadIds: string[] };
}
export interface PrThread {
  id: string; anchorId: string | null; resolved: boolean | null; outdated: boolean | null; path: string | null;
  originalCommitOid: string | null; currentCommitOid: string | null; side: 'old' | 'new' | null;
  startLine: number | null; line: number | null; diffExcerpt: PrProse; comments: PrCollection<PrComment>; linkId: string | null;
}
export interface PrComment {
  id: string; author: PrPerson | null; createdAt: string | null; updatedAt: string | null;
  body: PrProse; replyToId: string | null; sourceKind: 'issue' | 'review' | 'thread';
}
export type ProviderPatch = { kind: 'hunks'; hunks: Array<{
  oldStart: number; oldCount: number; newStart: number; newCount: number;
  rows: Array<{ kind: 'context' | 'add' | 'remove'; text: string; oldLine: number | null; newLine: number | null }>;
}>; completeness: 'provider_excerpt' } | { kind: 'unavailable'; reason: PrCode };
export type PrFileContent =
  | ({ kind: 'text' } & import('./diff').ReviewText)
  | { kind: 'patch'; patch: ProviderPatch }
  | { kind: 'unsupported'; code: PrCode };
export type PrStatusResult = { kind: 'ready' } | PrFailure;
export type PrAssociationResult = ({ kind: 'association' } & PrAssociation) | PrFailure;
export type PrOpenResult = ({ kind: 'snapshot' } & PrSnapshot) | PrFailure;
export type PrCompareResult = ({ kind: 'comparison' } & PrComparison) | PrFailure;
export type PrPageResult = { kind: 'page'; collection: PrCollection<PrItem> } | PrFailure;
export type PrFilesResult = { kind: 'files'; collection: PrCollection<PrFile> } | PrFailure;
export type PrAnchorResult = { kind: 'resolved'; comparison: PrComparison; fileId: string; side: 'old' | 'new'; line: number }
  | { kind: 'fallback'; excerpt: PrProse; linkId: string | null; code: PrCode } | PrFailure;
export type PrFileResult = { kind: 'file'; comparisonId: string; fileId: string; content: PrFileContent } | PrFailure;
export interface PullRequestClient {
  status(entryId: string): Promise<PrStatusResult>;
  associations(entryId: string, branch: string | null): Promise<PrAssociationResult>;
  mapHead(entryId: string, associationId: string, owner: string, repository: string, headRef: string): Promise<PrAssociationResult>;
  choose(entryId: string, associationId: string, candidateId: string): Promise<{ kind: 'chosen'; prId: string } | PrFailure>;
  open(entryId: string, prId: string): Promise<PrOpenResult>;
  page(entryId: string, sessionId: string, collection: PrCollectionKind, cursor: string | null, threadId?: string): Promise<PrPageResult>;
  refresh(entryId: string, sessionId: string): Promise<PrOpenResult>;
  compare(entryId: string, sessionId: string, selection: PrComparisonSelection): Promise<PrCompareResult>;
  filesPage(entryId: string, comparisonId: string, cursor: string): Promise<PrFilesResult>;
  resolveAnchor(entryId: string, sessionId: string, anchorId: string): Promise<PrAnchorResult>;
  file(entryId: string, comparisonId: string, fileId: string): Promise<PrFileResult>;
  release(entryId: string, sessionId: string): Promise<{ kind: 'released' } | PrFailure>;
  openLink(entryId: string, sessionId: string, linkId: string): Promise<{ kind: 'opened' | 'blocked' } | PrFailure>;
}

export type PullRequestCommand = 'pr_status' | 'pr_associations' | 'pr_map_head' | 'pr_choose' | 'pr_open'
  | 'pr_page' | 'pr_refresh' | 'pr_compare' | 'pr_files_page' | 'pr_resolve_anchor' | 'pr_file' | 'pr_release' | 'pr_open_link';
