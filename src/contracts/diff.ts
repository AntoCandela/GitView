/** Defines read-only revision-bound file comparisons; display labels never authorize native reads. */

export type ReviewCategory = "staged" | "unstaged" | "untracked";

/** Endpoints name Git snapshots; absence is explicit rather than an empty-file guess. */
export interface ReviewIdentity {
  entryId: string;
  pathId: string;
  category: ReviewCategory;
  displayPath: string;
  contextLabel: string;
  from: "HEAD" | "index" | "absent";
  to: "index" | "working_files";
  fromAbsent: boolean;
  toAbsent: boolean;
}

export interface TextHunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: Array<{
    kind: "context" | "addition" | "removal";
    text: string;
    noFinalNewline?: boolean;
  }>;
}

/** Exact bounded UTF-8 snapshots verified together with the native hunks. */
export interface ReviewText {
  hunks: TextHunk[];
  fromContent: string;
  toContent: string;
}

export type ReviewUnsupportedReason =
  | "conflict" | "rename_or_copy" | "type_change" | "submodule" | "binary"
  | "large_or_truncated" | "unborn_head" | "unsupported_encoding" | "other";
export type ReviewUnavailableCode =
  | "inaccessible" | "git_unavailable" | "unsafe_repository" | "timeout"
  | "changed_during_read" | "invalid_output";

/** Stale outcomes carry no content; only a subsequent current observation may authorize another read. */
export type ReviewResult =
  | ({ kind: "text" } & ReviewText & ReviewIdentity)
  | { kind: "unsupported"; reason: ReviewUnsupportedReason; identity: ReviewIdentity }
  | { kind: "unavailable"; code: ReviewUnavailableCode; identity: ReviewIdentity }
  | { kind: "stale_selection" }
  | { kind: "stale_observation" };

/** Local reading choice follows native stable identity, never a display path or revision token. */
export interface ReviewSelection {
  stablePathId: string;
  category: ReviewCategory;
  displayPath: string;
}
