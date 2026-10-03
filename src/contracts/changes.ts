/** Defines complete renderer-safe status snapshots; display labels never authorize native I/O. */

export type ChangeKind = "modified" | "added" | "deleted";
export type UnsupportedKind = "rename_or_copy" | "submodule" | "type_change";
export type ObservationErrorCode =
  | "inaccessible" | "git_unavailable" | "unsafe_repository" | "timeout"
  | "invalid_status" | "unsupported_path_encoding" | "resource_limit" | "unsupported_configuration";

/** One native path with independent index, worktree and untracked categories. */
export interface ChangedPath {
  /** Opaque revision-bound host identity, not a filesystem path. */
  pathId: string;
  /** Stable per-entry native-path identity, retained across temporary disappearance. */
  stablePathId: string;
  displayPath: string;
  /** Native path components; do not derive hierarchy by splitting displayPath. */
  segments: string[];
  staged: ChangeKind | null;
  unstaged: ChangeKind | null;
  untracked: boolean;
  conflict: boolean;
  unsupportedKind: UnsupportedKind | null;
}

/** Revisions are ordered only within an entry; only ready with no files means Clean. */
export type ObservationSnapshot = {
  entryId: string;
  observationRevision: number;
} & (
  | { kind: "checking" }
  | { kind: "ready"; files: ChangedPath[] }
  | { kind: "unavailable"; errorCode: ObservationErrorCode }
  | { kind: "bare" }
);
