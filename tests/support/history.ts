/** Builds renderer ancestry fixtures with explicit raw parents and no native filesystem authority. */

import type { HistoryCommit, HistoryPage } from "../../src/contracts/history";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { reviewClient } from "./review";

export const historyOids = {
  merge: "a".repeat(40), first: "b".repeat(40), second: "c".repeat(40), root: "d".repeat(40), missing: "e".repeat(40),
};

export function historyCommit(oid: string, parents: HistoryCommit["parents"] = [], subject: string | null = null): HistoryCommit {
  return { oid, parents, subject, root: parents.length === 0 };
}

export function historyPage(commits: HistoryCommit[] = [], overrides: Partial<HistoryPage> = {}): HistoryPage {
  return {
    upstream: { state: "no_upstream", freshness: "unavailable", branch: "main", upstream: null, ahead: 0, behind: 0, incoming: null, outgoing: null },
    entryId: "one", cursor: null, commits, refs: [], hasMore: false, completeness: "complete",
    head: { scope: "worktree", state: "attached", branch: "main", oid: commits[0]?.oid ?? null },
    ...overrides,
  };
}

export function historyClient(historyPage: RepositoryClient["historyPage"]): RepositoryClient {
  return { ...reviewClient(), historyPage };
}

export function mergeHistory(): HistoryCommit[] {
  const { merge, first, second, root } = historyOids;
  return [
    historyCommit(merge, [{ oid: first, state: "loaded" }, { oid: second, state: "loaded" }], "Merge topic"),
    historyCommit(first, [{ oid: root, state: "loaded" }], "Main work"),
    historyCommit(second, [{ oid: root, state: "loaded" }], "Topic work"),
    historyCommit(root, [], "Initial commit"),
  ];
}
