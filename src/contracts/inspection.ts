/** Defines read-only branch/worktree choices and parent-specific committed-file lists. */

import type { HistoryPageResult } from "./history";
import type { ReviewUnavailableCode, ReviewUnsupportedReason, ReviewText } from "./diff";

export type InspectionFailure = Exclude<HistoryPageResult, { kind: "page" }>;

export type ContextOptionsResult =
  | { kind: "options"; branches: Array<{ name: string }>; worktrees: Array<{
      id: string; label: string; branch: string | null; current: boolean;
    }> }
  | InspectionFailure;

export interface CommittedFile {
  id: string;
  displayPath: string;
  segments: string[];
  kind: "added" | "modified" | "deleted" | "type_change";
}

/** A null request chooses the first raw parent, or the empty tree for a verified root. */
export type CommitFilesResult =
  | { kind: "files"; commitOid: string; parentOid: string | null; parents: string[]; files: CommittedFile[] }
  | InspectionFailure;

/** Pinned historical endpoints; opaque file IDs authorize reads, display paths never do. */
export interface CommitReviewIdentity {
  entryId: string;
  fileId: string;
  commitOid: string;
  parentOid: string | null;
  displayPath: string;
  contextLabel: string;
  fromAbsent: boolean;
  toAbsent: boolean;
}

export type CommitReviewResult =
  | ({ kind: "text" } & ReviewText & CommitReviewIdentity)
  | { kind: "unsupported"; reason: ReviewUnsupportedReason; identity: CommitReviewIdentity }
  | { kind: "unavailable"; code: ReviewUnavailableCode; identity: CommitReviewIdentity }
  | { kind: "stale_selection" };
