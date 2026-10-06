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
  const initial: WorkspaceSnapshot = { contextEpoch: "initial", revision: 1, entries: [entry, { ...entry, id: "two" }], activeContextId: "one", restoring: false, persistenceError: null };
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

test("an authoritative same-ID external context epoch retires local reading scopes", async () => {
  const client = reviewClient();
  const { result } = renderHook(() => useWorkspace(client));
  await act(async () => { await Promise.resolve(); });
  const generation = result.current.selectionGeneration;
  const external = { ...await client.snapshot(), revision: 8, contextEpoch: "external-reselection" };
  act(() => result.current.acceptExternalSnapshot(external));
  expect(result.current.snapshot.contextEpoch).toBe("external-reselection");
  expect(result.current.selectionGeneration).toBeGreaterThan(generation);
  act(() => result.current.acceptExternalSnapshot({ ...external, revision: 7, contextEpoch: "stale" }));
  expect(result.current.snapshot.contextEpoch).toBe("external-reselection");
});

test("native selection refresh arriving before its delayed persistence reply remains authoritative", async () => {
  const selection = deferred<SelectOutcome>();
  const client = reviewClient();
  const entry = { id: "one", kind: "working_tree" as const, repositoryLabel: "Repository", locationLabel: "Worktree",
    head: { kind: "branch" as const, name: "old-head" }, availability: "available" as const };
  const initial: WorkspaceSnapshot = { revision: 1, contextEpoch: "initial", activeContextId: "one", entries: [entry], restoring: false, persistenceError: null };
  client.snapshot = async () => initial;
  client.selectContext = () => selection.promise;
  client.refreshEntryAvailability = async () => { throw new Error("Selection refresh is native-owned"); };
  const { result } = renderHook(() => useWorkspace(client));
  await act(async () => { await Promise.resolve(); });
  act(() => result.current.selectRepository("one"));
  await act(async () => { await Promise.resolve(); });
  const selecting: WorkspaceSnapshot = { ...initial, revision: 2, contextEpoch: "selected",
    entries: [{ ...entry, availability: "checking" }] };
  act(() => result.current.acceptExternalSnapshot(selecting));
  const refreshed: WorkspaceSnapshot = { ...selecting, revision: 3, entries: [{ ...entry, head: { kind: "branch", name: "fresh-head" } }] };
  act(() => result.current.acceptExternalSnapshot(refreshed));
  await act(async () => selection.resolve({ kind: "selected", snapshot: selecting }));
  expect(result.current.snapshot).toEqual(refreshed);
  expect(result.current.pendingId).toBeNull();
  expect(result.current.error).toBeNull();
});
