/** Exercises pinned paging, explicit snapshot replacement and render-time scope rejection. */

import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode, type ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { HistoryPageResult } from "../../src/contracts/history";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { useHistory } from "../../src/features/history/useHistory";
import { deferred } from "../support/deferred";
import { historyClient, historyCommit, historyOids, historyPage } from "../support/history";

afterEach(cleanup);
interface Props { client: RepositoryClient; entryId: string; generation: number }
function readHistory({ client, entryId, generation }: Props) {
  return useHistory(client, entryId, generation);
}

test("continuations append commits and retain pinned HEAD and refs until explicit refresh", async () => {
  const { first, root } = historyOids;
  const pinned = historyPage([historyCommit(first, [{ oid: root, state: "outside_page" }])], {
    cursor: "opaque:next", hasMore: true, completeness: "paged",
    refs: [{ kind: "local_branch", name: "main", commitOid: first }],
  });
  const moved = historyPage([historyCommit(root)], {
    refs: [{ kind: "local_branch", name: "main", commitOid: root }],
    head: { scope: "worktree", state: "detached", branch: null, oid: root },
  });
  const read = vi.fn<RepositoryClient["historyPage"]>()
    .mockResolvedValueOnce({ kind: "page", page: pinned })
    .mockResolvedValueOnce({ kind: "page", page: moved })
    .mockResolvedValueOnce({ kind: "page", page: moved });
  const props: Props = { client: historyClient(read), entryId: "one", generation: 0 };
  const { result, rerender } = renderHook(readHistory, { initialProps: props });
  await act(async () => { await Promise.resolve(); });
  act(() => { result.current.loadMore(); result.current.loadMore(); });
  await act(async () => { await Promise.resolve(); });
  expect(result.current.page?.commits.map((commit) => commit.oid)).toEqual([first, root]);
  expect(result.current.page?.head).toEqual(pinned.head);
  expect(result.current.page?.refs).toEqual(pinned.refs);
  expect(read.mock.calls).toEqual([["one", null], ["one", "opaque:next"]]);
  rerender(props);
  expect(read).toHaveBeenCalledTimes(2);
  act(() => result.current.refresh());
  expect(result.current.page).toBeNull();
  await act(async () => { await Promise.resolve(); });
  expect(result.current.page).toEqual(moved);
  expect(read.mock.calls).toEqual([["one", null], ["one", "opaque:next"], ["one", null]]);
});

test.each(["entry", "generation", "client"] as const)("late pages and failures cannot publish after a %s change", async (change) => {
  const old = deferred<HistoryPageResult>();
  const current = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>().mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  const initial: Props = { client: historyClient(read), entryId: "one", generation: 0 };
  const { result, rerender } = renderHook(readHistory, { initialProps: initial });
  const next = change === "entry" ? { ...initial, entryId: "two" }
    : change === "generation" ? { ...initial, generation: 1 }
      : { ...initial, client: historyClient(() => current.promise) };
  rerender(next);
  expect(result.current.page).toBeNull();
  expect(result.current.error).toBeNull();
  await act(async () => current.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.root)], { entryId: next.entryId }) }));
  const verified = result.current.page;
  await act(async () => old.reject(new Error("private transport detail")));
  expect(result.current.page).toBe(verified);
  expect(result.current.error).toBeNull();
});

test("completed graph clears immediately on same-entry reselection and rejects an older success", async () => {
  const older = deferred<HistoryPageResult>();
  const current = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>()
    .mockResolvedValueOnce({ kind: "page", page: historyPage([historyCommit(historyOids.first)]) })
    .mockReturnValueOnce(older.promise).mockReturnValueOnce(current.promise);
  const props = { client: historyClient(read), entryId: "one", generation: 0 };
  const { result, rerender } = renderHook(readHistory, { initialProps: props });
  await act(async () => { await Promise.resolve(); });
  expect(result.current.page?.commits[0].oid).toBe(historyOids.first);
  rerender({ ...props, generation: 1 });
  expect(result.current.page).toBeNull();
  rerender({ ...props, generation: 2 });
  await act(async () => older.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.second)]) }));
  expect(result.current.page).toBeNull();
  await act(async () => current.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.root)]) }));
  expect(result.current.page?.commits[0].oid).toBe(historyOids.root);
});

test("refresh invalidates a pending continuation and its late domain error", async () => {
  const continuation = deferred<HistoryPageResult>();
  const refreshed = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>()
    .mockResolvedValueOnce({ kind: "page", page: historyPage([historyCommit(historyOids.first)], { cursor: "pinned", hasMore: true }) })
    .mockReturnValueOnce(continuation.promise).mockReturnValueOnce(refreshed.promise);
  const { result } = renderHook(readHistory, { initialProps: { client: historyClient(read), entryId: "one", generation: 0 } });
  await act(async () => { await Promise.resolve(); });
  act(() => result.current.loadMore());
  act(() => result.current.refresh());
  expect(result.current.page).toBeNull();
  await act(async () => refreshed.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.root)]) }));
  await act(async () => continuation.resolve({ kind: "error", code: "stale_cursor", message: "The history snapshot expired." }));
  expect(result.current.page?.commits.map((commit) => commit.oid)).toEqual([historyOids.root]);
  expect(result.current.error).toBeNull();
});

test("continuation errors preserve pinned rows and retry the same opaque cursor", async () => {
  const retry = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>()
    .mockResolvedValueOnce({ kind: "page", page: historyPage([historyCommit(historyOids.first)], { cursor: "retry-token", hasMore: true }) })
    .mockResolvedValueOnce({ kind: "error", code: "timeout", message: "History read timed out." })
    .mockReturnValueOnce(retry.promise);
  const { result } = renderHook(readHistory, { initialProps: { client: historyClient(read), entryId: "one", generation: 0 } });
  await act(async () => { await Promise.resolve(); });
  act(() => result.current.loadMore());
  await act(async () => { await Promise.resolve(); });
  expect(result.current.page?.commits[0].oid).toBe(historyOids.first);
  expect(result.current.error?.kind).toBe("error");
  act(() => result.current.loadMore());
  expect(read.mock.calls).toEqual([["one", null], ["one", "retry-token"], ["one", "retry-token"]]);
  await act(async () => retry.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.root)]) }));
  expect(result.current.page?.commits.map((commit) => commit.oid)).toEqual([historyOids.first, historyOids.root]);
  expect(result.current.error).toBeNull();
});

test("wrong-entry responses remain errors rather than a successful graph", async () => {
  const { result } = renderHook(readHistory, { initialProps: {
    client: historyClient(async () => ({ kind: "page", page: historyPage([historyCommit(historyOids.root)], { entryId: "other" }) })), entryId: "one", generation: 0,
  } });
  await act(async () => { await Promise.resolve(); });
  expect(result.current.page).toBeNull();
  expect(result.current.error).toMatchObject({ kind: "error", code: "invalid_output" });
});

test("strict lifecycle replay does not strand the initial history request", async () => {
  const response = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>().mockReturnValue(response.promise);
  const { result } = renderHook(readHistory, {
    initialProps: { client: historyClient(read), entryId: "one", generation: 0 },
    wrapper: ({ children }: { children: ReactNode }) => <StrictMode>{children}</StrictMode>,
  });
  await act(async () => response.resolve({ kind: "page", page: historyPage([historyCommit(historyOids.root)]) }));
  expect(result.current.page?.commits[0].oid).toBe(historyOids.root);
  expect(read).toHaveBeenCalledTimes(1);
});
