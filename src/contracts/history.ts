/** Defines pinned read-only commit ancestry pages; cursors are opaque native query authority. */

export interface HistoryCommit {
  oid: string;
  subject: string | null;
  parents: Array<{ oid: string; state: "loaded" | "outside_page" | "unavailable" }>;
  /** Only true for a verified raw commit with no parents, never a pagination boundary. */
  root: boolean;
}

export interface HistoryPage {
  entryId: string;
  cursor: string | null;
  commits: HistoryCommit[];
  refs: Array<{ kind: "local_branch" | "remote_tracking" | "tag"; name: string; commitOid: string }>;
  head: {
    scope: "worktree" | "repository";
    state: "attached" | "detached" | "unborn" | "unresolved";
    branch: string | null;
    oid: string | null;
  };
  hasMore: boolean;
  completeness: "complete" | "paged" | "shallow_or_missing";
}

export type HistoryErrorCode = "inaccessible" | "git_unavailable" | "unsafe_repository" | "timeout" | "invalid_output"
  | "resource_limit" | "stale_selection" | "stale_cursor" | "missing_objects";

/** Domain errors contain fixed sanitized messages; rejected promises mean transport interruption. */
export type HistoryPageResult =
  | { kind: "page"; page: HistoryPage }
  | {
      kind: "unavailable" | "error";
      code: HistoryErrorCode;
      message: string;
    };
