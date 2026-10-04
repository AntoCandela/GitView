/**
 * Defines renderer-safe workspace DTOs and the desktop client contract.
 * Rust owns repository identity and state; location labels are display text,
 * never native paths to send back to the host.
 */

import type { Locale } from "../i18n";
import type { ObservationSnapshot } from "./changes";
import type { RepositoryFileResult, RepositoryFilesRequest, RepositoryFilesResult } from "./browsing";
import type { ReviewCategory, ReviewResult } from "./diff";
import type { HistoryPageResult } from "./history";
import type { CommitFilesResult, CommitReviewResult, ContextOptionsResult } from "./inspection";

/** Restored HEAD is unknown until native verification; unborn names a branch without a commit. */
export type HeadContext =
  | { kind: "unknown" }
  | { kind: "branch"; name: string }
  | { kind: "detached"; shortOid: string }
  | { kind: "unborn"; name: string };

/** One admitted native context; availability says nothing about worktree cleanliness. */
export interface RepositoryEntry {
  /** Opaque host-issued identity, stable when the same context is reopened. */
  id: string;
  kind: "unknown" | "working_tree" | "bare";
  repositoryLabel: string;
  /** Human-readable location only; native filesystem identity stays in Rust. */
  locationLabel: string;
  head: HeadContext;
  availability: "available" | "checking" | "unavailable";
}

/** Sanitized native storage feedback, independent of repository operation errors. */
export interface PersistenceError {
  code: "load_failed" | "unsupported_version" | "save_failed" | "storage_unavailable";
  message: string;
}

/** Complete authoritative state, ordered by the host's monotonic workspace revision. */
export interface WorkspaceSnapshot {
  revision: number;
  entries: RepositoryEntry[];
  /** Selection is independent of admission; opening an entry does not select it. */
  activeContextId: string | null;
  /** Initial saved-location checks are still running; cached Git facts are never restored. */
  restoring: boolean;
  /** Only a successful native save clears a save failure; dismissal does not change it. */
  persistenceError: PersistenceError | null;
}

/** Closed native admission vocabulary; these are stable facts, not display text. */
export type GitErrorCode =
  | "git_unavailable"
  | "not_repository"
  | "inaccessible"
  | "unsafe_repository"
  | "probe_timeout"
  | "repository_changed"
  | "unsupported_path_encoding"
  | "repository_unavailable";

export type WorkspaceRejectionCode = GitErrorCode | "invalid_display_name" | "superseded_selection";

/** Picker/admission result, always accompanied by current state, even after rejection. */
export type OpenOutcome =
  | { kind: "cancelled"; snapshot: WorkspaceSnapshot }
  | { kind: "opened" | "reused"; entryId: string; snapshot: WorkspaceSnapshot }
  | {
      kind: "rejected";
      code: GitErrorCode;
      snapshot: WorkspaceSnapshot;
    };

/** A missing entry leaves host state unchanged and returns the current snapshot. */
export type SelectOutcome =
  | { kind: "selected"; snapshot: WorkspaceSnapshot }
  | { kind: "not_found"; snapshot: WorkspaceSnapshot };

/** Display-name edits and sidebar removal affect app state only, never filesystem paths. */
export type RepositoryMutationOutcome =
  | { kind: "updated" | "not_found"; snapshot: WorkspaceSnapshot }
  | { kind: "rejected"; code: WorkspaceRejectionCode; snapshot: WorkspaceSnapshot };

/**
 * Desktop operations return complete snapshots rather than renderer-owned mutations.
 * Promise rejection means transport/command failure; domain outcomes stay typed.
 */
export interface RepositoryClient {
  snapshot(): Promise<WorkspaceSnapshot>;
  /** Ordered OS UI-language preferences; never logged or used for Git subprocesses. */
  preferredLanguages(): Promise<{ languages: string[] }>;
  /** Opens the native folder picker; cancellation is a normal outcome. */
  openChosenRepository(locale: Locale): Promise<OpenOutcome>;
  /** Activates an admitted context; callers must preserve user-intent order. */
  selectContext(entryId: string): Promise<SelectOutcome>;
  /** Reprobes a location without changing selection; the host rejects stale probes. */
  refreshEntryAvailability(entryId: string): Promise<WorkspaceSnapshot>;
  /** Reads the selected entry's cached observation; native scans run independently. */
  observeSelectedContext(entryId: string): Promise<ObservationSnapshot>;
  /** Reads one category using its current observation token; this never mutates Git or files. */
  reviewFile(entryId: string, observationRevision: number, pathId: string, category: ReviewCategory): Promise<ReviewResult>;
  /** Reads pinned ancestry for a known local branch; this never checks out or mutates Git. */
  historyPage(entryId: string, cursor: string | null, branch?: string | null): Promise<HistoryPageResult>;
  /** Lists known local branches and existing native worktrees, including unadmitted worktrees. */
  listContexts(entryId: string): Promise<ContextOptionsResult>;
  /** Admits/selects an existing worktree context in app state only; preserves user-intent order. */
  selectWorktree(entryId: string, worktreeId: string): Promise<RepositoryMutationOutcome>;
  /** Null selects the first raw parent, or the empty tree for a verified root. */
  commitFiles(entryId: string, commitOid: string, parentOid: string | null): Promise<CommitFilesResult>;
  /** Reads a native-authorized file from the exact raw-parent to commit comparison. */
  reviewCommitFile(entryId: string, commitOid: string, parentOid: string | null, fileId: string): Promise<CommitReviewResult>;
  /** Pages an issued directory, including ignored files but excluding Git internals. */
  listRepositoryFiles(entryId: string, request: RepositoryFilesRequest): Promise<RepositoryFilesResult>;
  /** Reads current working text using issued authority, never a renderer-supplied path. */
  reviewRepositoryFile(entryId: string, listingId: string, fileId: string): Promise<RepositoryFileResult>;
  /** Changes only the app display name and saves the current workspace choices. */
  renameRepository(entryId: string, displayName: string): Promise<RepositoryMutationOutcome>;
  /** Removes a sidebar entry without deleting files; native state chooses any replacement. */
  removeRepository(entryId: string): Promise<RepositoryMutationOutcome>;
}
