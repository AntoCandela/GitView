/** Exercises ordered native worktree/repository intents and authoritative snapshot publication. */

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { RepositoryMutationOutcome, SelectOutcome, WorkspaceSnapshot } from "../../src/contracts/repositories";
import { useWorkspace } from "../../src/features/repositories/useWorkspace";
import type { WorkspaceError } from "../../src/features/repositories/workspaceError";
import { deferred } from "../support/deferred";
import { reviewClient } from "../support/review";

afterEach(cleanup);

test("a later repository intent waits for worktree admission but its older reply never publishes intermediate selection", async () => {
  const worktree = deferred<RepositoryMutationOutcome>();
  const repository = deferred<SelectOutcome>();
  const entry = { id: "one", kind: "working_tree" as const, repositoryLabel: "one", locationLabel: "opaque display", head: { kind: "branch" as const, name: "main" }, availability: "available" as const };
  const initial: WorkspaceSnapshot = { revision: 1, entries: [entry, { ...entry, id: "two" }], activeContextId: "one", restoring: false, persistenceError: null };
  const client = reviewClient();
  client.snapshot = async () => initial;
  let repositoryStarted = false;
  client.selectWorktree = () => worktree.promise;
  client.selectContext = () => { repositoryStarted = true; return repository.promise; };
  client.refreshEntryAvailability = async () => ({ ...initial, revision: 4, activeContextId: "two" });
  const { result } = renderHook(() => useWorkspace(client));
  await act(async () => { await Promise.resolve(); });
  await act(async () => { result.current.selectWorktree("one", "native-worktree"); result.current.selectRepository("two"); });
  expect(repositoryStarted).toBe(false);
  expect(result.current.pendingId).toBe("two");
  await act(async () => worktree.resolve({ kind: "updated", snapshot: { ...initial, revision: 2, entries: [...initial.entries, { ...entry, id: "admitted-worktree" }], activeContextId: "admitted-worktree" } }));
  expect(repositoryStarted).toBe(true);
  expect(result.current.snapshot.activeContextId).toBe("one");
  expect(result.current.pendingId).toBe("two");
  await act(async () => repository.resolve({ kind: "selected", snapshot: { ...initial, revision: 3, activeContextId: "two" } }));
  expect(result.current.snapshot.activeContextId).toBe("two");
  expect(result.current.pendingId).toBeNull();
});

test("rename rejection remains a coded fact rather than native or translated prose", async () => {
  const client = reviewClient();
  client.renameRepository = async () => ({ kind: "rejected", code: "invalid_display_name", snapshot: await client.snapshot() });
  const { result } = renderHook(() => useWorkspace(client));
  await act(async () => { await Promise.resolve(); });
  let rejection: WorkspaceError | null = null;
  await act(async () => { rejection = await result.current.renameRepository("one", ""); });
  expect(rejection).toEqual({ domain: "rejection", code: "invalid_display_name" });
});
