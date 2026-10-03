/** Defines display-only repository files and opaque native listing authority for working-content browsing. */

import type { ReviewUnavailableCode, ReviewUnsupportedReason } from "./diff";
import type { HistoryErrorCode } from "./history";

/** Display paths and segments never authorize filesystem reads. */
export interface RepositoryFile {
  id: string;
  displayPath: string;
  segments: string[];
}

/** Issued directory authority; labels never authorize traversal. */
export interface RepositoryDirectory {
  id: string;
  displayPath: string;
  segments: string[];
}

/** All-null starts a new listing. Null directory selects its root; cursors are directory-scoped. */
export interface RepositoryFilesRequest {
  listingId: string | null;
  directoryId: string | null;
  cursor: string | null;
}

/** A listing remains valid through later refreshes until bounded native eviction or reselection. */
export interface RepositoryFileSelection extends RepositoryFile {
  listingId: string;
}

export type RepositoryFilesResult =
  | {
      kind: "files";
      entryId: string;
      listingId: string;
      directoryId: string | null;
      files: RepositoryFile[];
      directories: RepositoryDirectory[];
      /** Null completes this directory only, not the repository. */
      cursor: string | null;
    }
  | { kind: "unavailable"; code: HistoryErrorCode; message: string }
  | { kind: "stale_selection" };

export type RepositoryFileResult =
  | { kind: "text"; entryId: string; listingId: string; fileId: string; displayPath: string; content: string }
  | { kind: "unsupported"; reason: ReviewUnsupportedReason }
  | { kind: "unavailable"; code: ReviewUnavailableCode }
  | { kind: "stale_selection" };
