/** Exercises complete-snapshot polling, stale-selection rejection and transport recovery. */

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ObservationSnapshot } from "../../src/contracts/changes";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { useObservation } from "../../src/features/changes/useObservation";
import { deferred } from "../support/deferred";

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });

const ready = (entryId: string, observationRevision: number): ObservationSnapshot => ({ entryId, observationRevision, kind: "ready", files: [] });
function clientWith(observeSelectedContext: RepositoryClient["observeSelectedContext"]): RepositoryClient {
  const snapshot = { contextEpoch: "initial", revision: 0, entries: [], activeContextId: null, restoring: false, persistenceError: null };
  return {
    snapshot: async () => snapshot,
    preferredLanguages: async () => ({ languages: [] }),
    openChosenRepository: async () => ({ kind: "cancelled", snapshot }),
    selectContext: async () => ({ kind: "not_found", snapshot }),
    refreshEntryAvailability: async () => snapshot,
    observeSelectedContext,
    reviewFile: async () => ({ kind: "stale_observation" }),
    reviewCommitFile: async () => ({ kind: "stale_selection" }),
    listRepositoryFiles: async (entryId) => ({ kind: "files", entryId, listingId: "empty-listing", directoryId: null, cursor: null, directories: [], files: [] }),
    reviewRepositoryFile: async () => ({ kind: "stale_selection" }),
    historyPage: async () => ({ kind: "error", code: "stale_selection", message: "Selection changed." }),
    listContexts: async () => ({ kind: "options", branches: [], worktrees: [] }),
    selectWorktree: async () => ({ kind: "not_found", snapshot }),
    upstreamFiles: async () => ({ kind: "unavailable", code: "stale_selection", message: "Selection changed." }),
    commitFiles: async () => ({ kind: "error", code: "stale_selection", message: "Selection changed." }),
    renameRepository: async () => ({ kind: "not_found", snapshot }),
    removeRepository: async () => ({ kind: "not_found", snapshot }),
  };
}
async function flush() { await act(async () => { await Promise.resolve(); }); }
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(1000); }); }

test("initial selected context observes immediately and only newer matching revisions replace it", async () => {
  let incoming = ready("one", 4);
  const client = clientWith(async () => incoming);
  const { result } = renderHook(() => useObservation(client, "one", 0));
  await flush();
  expect(result.current).toEqual(ready("one", 4));
  incoming = { entryId: "one", observationRevision: 3, kind: "unavailable", errorCode: "inaccessible" };
  await tick();
  expect(result.current).toEqual(ready("one", 4));
  incoming = ready("wrong-entry", 10);
  await tick();
  expect(result.current).toEqual(ready("one", 4));
  incoming = { entryId: "one", observationRevision: 5, kind: "unavailable", errorCode: "invalid_status" };
  await tick();
  expect(result.current).toEqual(incoming);
});

test("late A cannot render under B and revision ordering restarts for the new entry", async () => {
  const replyA = deferred<ObservationSnapshot>();
  const replyB = deferred<ObservationSnapshot>();
  const client = clientWith((id) => id === "one" ? replyA.promise : replyB.promise);
  const { result, rerender } = renderHook(({ id, generation }) => useObservation(client, id, generation), { initialProps: { id: "one", generation: 0 } });
  rerender({ id: "two", generation: 1 });
  expect(result.current?.kind).toBe("checking");
  await act(async () => replyA.resolve(ready("one", 20)));
  expect(result.current?.kind).toBe("checking");
  await act(async () => replyB.resolve(ready("two", 1)));
  expect(result.current).toEqual(ready("two", 1));
});

test("pending selection clears the old snapshot before the next request completes", async () => {
  const client = clientWith(async (id) => ready(id, 1));
  const initialProps: { id: string | null; generation: number } = { id: "one", generation: 0 };
  const { result, rerender } = renderHook(({ id, generation }) => useObservation(client, id, generation), { initialProps });
  await flush();
  expect(result.current?.kind).toBe("ready");
  rerender({ id: null, generation: 1 });
  expect(result.current).toBeNull();
});

test("same-ID reselection invalidates an in-flight old generation", async () => {
  const old = deferred<ObservationSnapshot>();
  const current = deferred<ObservationSnapshot>();
  const observe = vi.fn<RepositoryClient["observeSelectedContext"]>().mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  const client = clientWith(observe);
  const { result, rerender } = renderHook(({ generation }) => useObservation(client, "one", generation), { initialProps: { generation: 0 } });
  rerender({ generation: 1 });
  await act(async () => old.resolve(ready("one", 100)));
  expect(result.current?.kind).toBe("checking");
  await act(async () => current.resolve(ready("one", 1)));
  expect(result.current).toEqual(ready("one", 1));
});

test("transport interruption becomes unavailable and the same cached revision recovers automatically", async () => {
  let offline = false;
  const client = clientWith(async () => { if (offline) throw new Error("offline"); return ready("one", 8); });
  const { result } = renderHook(() => useObservation(client, "one", 0));
  await flush();
  offline = true;
  await tick();
  expect(result.current?.kind).toBe("transport_unavailable");
  offline = false;
  await tick();
  expect(result.current).toEqual(ready("one", 8));
});

test("bare snapshots recover after transport loss without inventing a newer revision", async () => {
  const bare: ObservationSnapshot = { entryId: "one", observationRevision: 2, kind: "bare" };
  let offline = false;
  const client = clientWith(async () => {
    if (offline) throw new Error("offline");
    return bare;
  });
  const { result } = renderHook(() => useObservation(client, "one", 0));
  await flush();
  expect(result.current).toEqual(bare);
  offline = true;
  await tick();
  expect(result.current?.kind).toBe("transport_unavailable");
  offline = false;
  await tick();
  expect(result.current).toEqual(bare);
});

test("slow transport never overlaps or queues ticks and unmount stops polling", async () => {
  const slow = deferred<ObservationSnapshot>();
  const observe = vi.fn<RepositoryClient["observeSelectedContext"]>().mockReturnValue(slow.promise);
  const client = clientWith(observe);
  const { result, unmount } = renderHook(() => useObservation(client, "one", 0));
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(observe).toHaveBeenCalledTimes(1);
  expect(result.current?.kind).toBe("checking");
  await act(async () => slow.resolve(ready("one", 1)));
  unmount();
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(observe).toHaveBeenCalledTimes(1);
});

test("returning to a client with an outstanding observation waits for its lane and wakes only the latest selection", async () => {
  const abandoned = deferred<ObservationSnapshot>();
  const latest = deferred<ObservationSnapshot>();
  const middle = deferred<ObservationSnapshot>();
  const observe = vi.fn<RepositoryClient["observeSelectedContext"]>()
    .mockReturnValueOnce(abandoned.promise).mockReturnValueOnce(latest.promise);
  const original = clientWith(observe);
  const replacement = clientWith(() => middle.promise);
  const { result, rerender } = renderHook(({ client, generation }) => useObservation(client, "one", generation), {
    initialProps: { client: original, generation: 0 },
  });
  rerender({ client: replacement, generation: 1 });
  rerender({ client: original, generation: 2 });
  rerender({ client: original, generation: 3 });
  expect(observe).toHaveBeenCalledTimes(1);
  await act(async () => abandoned.resolve(ready("one", 100)));
  expect(result.current?.kind).toBe("checking");
  expect(observe).toHaveBeenCalledTimes(2);
  await act(async () => latest.resolve(ready("one", 2)));
  await act(async () => middle.reject(new Error("Disconnected middle transport")));
  expect(result.current).toEqual(ready("one", 2));
});

test("replacement clients do not wait for disconnected reads and late failures cannot clear current observation", async () => {
  const abandoned = deferred<ObservationSnapshot>();
  const oldClient = clientWith(() => abandoned.promise);
  const newClient = clientWith(async () => ready("one", 2));
  const { result, rerender } = renderHook(({ client }) => useObservation(client, "one", 0), { initialProps: { client: oldClient } });
  rerender({ client: newClient });
  expect(result.current?.kind).toBe("checking");
  await flush();
  expect(result.current).toEqual(ready("one", 2));
  await act(async () => abandoned.reject(new Error("Disconnected private transport")));
  expect(result.current).toEqual(ready("one", 2));
});

test("hidden observation does not poll or accept an earlier visible response", async () => {
  const pending = deferred<ObservationSnapshot>();
  const observe = vi.fn<RepositoryClient["observeSelectedContext"]>().mockReturnValue(pending.promise);
  const client = clientWith(observe);
  const { result, rerender } = renderHook(({ enabled }) => useObservation(client, "one", 0, enabled),
    { initialProps: { enabled: true } });
  rerender({ enabled: false });
  await act(async () => pending.resolve(ready("one", 9)));
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(result.current).toBeNull();
  expect(observe).toHaveBeenCalledTimes(1);
});
