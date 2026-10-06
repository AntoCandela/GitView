/** Exercises retained PR snapshots, account failures and native session release across view lifetimes. */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { PrOpenResult } from "../../src/contracts/pullRequests";
import { usePullRequest } from "../../src/features/pull-requests/usePullRequest";
import { deferred } from "../support/deferred";
import { prClient, prSnapshot } from "../support/pullRequests";

afterEach(cleanup);

test("failed refresh keeps the old endpoints and observation time explicitly stale", async () => {
  const client = prClient();
  const pending = deferred<PrOpenResult>();
  vi.mocked(client.refresh).mockReturnValue(pending.promise);
  const { result } = renderHook(() => usePullRequest(client, "entry", 1, "pr"));
  await act(async () => {});
  act(() => { result.current.refresh(); result.current.refresh(); });
  expect(client.refresh).toHaveBeenCalledTimes(1);
  expect(result.current.snapshot?.revision).toBe(1);
  await act(async () => pending.resolve({ kind: "error", code: "network" }));
  expect(result.current.snapshot).toMatchObject({ revision: 1, observedAt: 1000, freshness: "stale", overview: { headOid: "head-1" } });
  expect(result.current.error).toEqual({ kind: "error", code: "network" });
});

test.each(["auth_required", "auth_unavailable", "access_denied", "repository_unavailable", "stale_context"] as const)("%s clears private displayed content", async code => {
  const client = prClient();
  vi.mocked(client.refresh).mockResolvedValue({ kind: "unavailable", code });
  const { result } = renderHook(() => usePullRequest(client, "entry", 1, "pr"));
  await act(async () => {});
  await act(async () => result.current.refresh());
  expect(result.current.snapshot).toBeNull();
  expect(result.current.error?.code).toBe(code);
  expect(client.release).toHaveBeenCalledWith("entry", "session");
});

test("context replacement clears immediately and releases a late open session", async () => {
  const client = prClient();
  const old = deferred<PrOpenResult>();
  vi.mocked(client.open).mockReturnValueOnce(old.promise).mockResolvedValueOnce(prSnapshot("new"));
  const { result, rerender } = renderHook(({ generation }) => usePullRequest(client, "entry", generation, "pr"), { initialProps: { generation: 1 } });
  rerender({ generation: 2 });
  expect(result.current.snapshot).toBeNull();
  await act(async () => {});
  await act(async () => old.resolve(prSnapshot("old")));
  expect(result.current.snapshot?.sessionId).toBe("new");
  expect(client.release).toHaveBeenCalledWith("entry", "old");
});

test("closing a review releases its session and ignores late refresh", async () => {
  const client = prClient();
  const pending = deferred<PrOpenResult>();
  vi.mocked(client.refresh).mockReturnValue(pending.promise);
  const { result, unmount } = renderHook(() => usePullRequest(client, "entry", 1, "pr"));
  await act(async () => {});
  act(() => result.current.refresh());
  unmount();
  expect(client.release).toHaveBeenCalledWith("entry", "session");
  await act(async () => pending.resolve(prSnapshot("session", 2)));
  expect(client.release).toHaveBeenCalledTimes(1);
});

test("replacement refresh adopts one coherent snapshot and strips private transport failures", async () => {
  const client = prClient();
  const { result } = renderHook(() => usePullRequest(client, "entry", 1, "pr"));
  await act(async () => {});
  await act(async () => result.current.refresh());
  expect(result.current.snapshot).toMatchObject({ revision: 2, overview: { headOid: "head-2" } });
  vi.mocked(client.refresh).mockRejectedValue(new Error("private payload"));
  await act(async () => result.current.refresh());
  expect(result.current.error).toEqual({ kind: "transport_unavailable" });
  expect(result.current.snapshot?.freshness).toBe("stale");
});

test("StrictMode cleanup releases the abandoned open without clearing the current review", async () => {
  const client = prClient();
  const abandoned = deferred<PrOpenResult>();
  vi.mocked(client.open).mockReturnValueOnce(abandoned.promise).mockResolvedValueOnce(prSnapshot("current"));
  const { result } = renderHook(() => usePullRequest(client, "entry", 1, "pr"), {
    reactStrictMode: true,
  });
  await act(async () => {});
  await act(async () => abandoned.resolve(prSnapshot("abandoned")));
  expect(result.current.snapshot?.sessionId).toBe("current");
  expect(client.release).toHaveBeenCalledExactlyOnceWith("entry", "abandoned");
});

test("deselecting the PR clears the review and does not start requests", async () => {
  const client = prClient();
  const { result, rerender } = renderHook(({ prId }: { prId: string | null }) => usePullRequest(client, "entry", 1, prId), { initialProps: { prId: "pr" as string | null } });
  await act(async () => {});
  rerender({ prId: null });
  expect(result.current.snapshot).toBeNull();
  expect(result.current.loading).toBe(false);
  act(() => result.current.refresh());
  expect(client.open).toHaveBeenCalledTimes(1);
  expect(client.refresh).not.toHaveBeenCalled();
  expect(client.release).toHaveBeenCalledWith("entry", "session");
});

test("a wrong-session refresh cannot relabel retained review authority", async () => {
  const client = prClient();
  vi.mocked(client.refresh).mockResolvedValue(prSnapshot("wrong", 2));
  const { result } = renderHook(() => usePullRequest(client, "entry", 1, "pr"));
  await act(async () => {});
  await act(async () => result.current.refresh());
  expect(result.current.snapshot).toMatchObject({ sessionId: "session", revision: 1, freshness: "stale" });
  expect(result.current.error?.code).toBe("invalid_output");
  expect(client.release).toHaveBeenCalledWith("entry", "wrong");
});
