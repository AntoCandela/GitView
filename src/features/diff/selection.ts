/** Defines renderer-only pinned selection and callbacks shared by history and app composition. */
import type { CommitReviewIdentity } from "../../contracts/inspection";

export type CommitReviewSelection = Pick<CommitReviewIdentity,
  "fileId" | "commitOid" | "parentOid" | "displayPath" | "fromAbsent" | "toAbsent"> & { segments: string[]; upstream?: { token: string; direction: "incoming" | "outgoing" } };

/** Controls the pinned committed-file comparison shown beside history. */
export interface CommitComparisonControls {
  selection: CommitReviewSelection | null;
  onSelect: (selection: CommitReviewSelection) => void;
  /** Captures current preview intent; the returned callback ignores later completions after another choice. */
  captureAutoSelection: () => (selection: CommitReviewSelection) => void;
  onInvalidate: () => void;
}
