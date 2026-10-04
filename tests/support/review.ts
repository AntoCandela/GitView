/** Provides isolated renderer-safe review fixtures; no real repository or native path is opened. */

import type { ChangedPath, ObservationSnapshot } from "../../src/contracts/changes";
import type { ReviewIdentity, ReviewResult, ReviewSelection } from "../../src/contracts/diff";
import type { RepositoryClient } from "../../src/contracts/repositories";

export const changedFile: ChangedPath = {
  pathId: "path-1", stablePathId: "stable-1", displayPath: "src/example.ts", segments: ["src", "example.ts"],
  staged: "modified", unstaged: "modified", untracked: false, conflict: false, unsupportedKind: null,
};
export const readingSelection: ReviewSelection = { stablePathId: "stable-1", displayPath: "src/example.ts", category: "unstaged" };
export const reviewIdentity: ReviewIdentity = {
  entryId: "one", pathId: "path-1", category: "unstaged", displayPath: "src/example.ts", contextLabel: "Sample repository",
  from: "index", to: "working_files", fromAbsent: false, toAbsent: false,
};
export function textReview(text: string, identity: Partial<ReviewIdentity> = {}): Extract<ReviewResult, { kind: "text" }> {
  return { ...reviewIdentity, ...identity, kind: "text", fromContent: "", toContent: `${text}\n`,
    hunks: [{ oldStart: 0, oldCount: 0, newStart: 1, newCount: 1, lines: [{ kind: "addition", text }] }] };
}
export function readyObservation(files: ChangedPath[] = [changedFile], observationRevision = 1, entryId = "one"): ObservationSnapshot {
  return { kind: "ready", entryId, observationRevision, files };
}
export function reviewClient(reviewFile: RepositoryClient["reviewFile"] = async () => ({ kind: "stale_observation" })): RepositoryClient {
  const snapshot = { revision: 0, entries: [], activeContextId: null, restoring: false, persistenceError: null };
  return {
    snapshot: async () => snapshot,
    preferredLanguages: async () => ({ languages: [] }),
    openChosenRepository: async () => ({ kind: "cancelled", snapshot }),
    selectContext: async () => ({ kind: "not_found", snapshot }),
    refreshEntryAvailability: async () => snapshot,
    observeSelectedContext: async () => readyObservation(),
    reviewFile,
    reviewCommitFile: async () => ({ kind: "stale_selection" }),
    listRepositoryFiles: async (entryId) => ({ kind: "files", entryId, listingId: "empty-listing", directoryId: null, cursor: null, directories: [], files: [] }),
    reviewRepositoryFile: async () => ({ kind: "stale_selection" }),
    listContexts: async () => ({ kind: "options", branches: [{ name: "main" }], worktrees: [] }),
    selectWorktree: async () => ({ kind: "not_found", snapshot }),
    commitFiles: async (_entryId, commitOid, parentOid) => ({ kind: "files", commitOid, parentOid, parents: [], files: [] }),
    historyPage: async (entryId) => ({
      kind: "page", page: {
        entryId, cursor: null, commits: [], refs: [], hasMore: false, completeness: "complete",
        head: { scope: "worktree", state: "unborn", branch: null, oid: null },
      },
    }),
    renameRepository: async () => ({ kind: "not_found", snapshot }),
    removeRepository: async () => ({ kind: "not_found", snapshot }),
  };
}
