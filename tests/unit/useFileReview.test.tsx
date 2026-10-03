/** Exercises live review ordering, coalescing and category disappearance through the public hook. */

import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode, type ReactNode } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { ReviewResult, ReviewSelection } from "../../src/contracts/diff";
import type { RepositoryClient } from "../../src/contracts/repositories";
import type { ObservationView } from "../../src/features/changes/useObservation";
import { useFileReview } from "../../src/features/diff/useFileReview";
import { deferred } from "../support/deferred";
import { changedFile, readingSelection, readyObservation, reviewClient, reviewIdentity, textReview } from "../support/review";

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });
interface Props { client: RepositoryClient; entryId: string; generation: number; observation: ObservationView; selection: ReviewSelection | null }
function initial(client: RepositoryClient): Props {
  return { client, entryId: "one", generation: 0, observation: readyObservation(), selection: readingSelection };
}
function readReview(props: Props) {
  return useFileReview(props.client, props.entryId, props.generation, props.observation, props.selection);
}

 test("intermediate revisions coalesce to the latest token and late text never appears current", async () => {
  const first = deferred<ReviewResult>();
  const latest = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValueOnce(first.promise).mockReturnValueOnce(latest.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  rerender({ ...props, observation: readyObservation([{ ...changedFile, pathId: "path-2" }], 2) });
  rerender({ ...props, observation: readyObservation([{ ...changedFile, pathId: "path-3" }], 3) });
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => first.resolve(textReview("superseded")));
  expect(result.current.kind).toBe("checking");
  expect(read).toHaveBeenLastCalledWith("one", 3, "path-3", "unstaged");
  await act(async () => latest.resolve(textReview("current", { pathId: "path-3" })));
  expect(result.current).toEqual(textReview("current", { pathId: "path-3" }));
});

test("continuous same-revision polls cannot starve a delayed review and byte-only edits refresh without flicker", async () => {
  const first = deferred<ReviewResult>();
  const next = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValueOnce(first.promise).mockReturnValueOnce(next.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  for (let poll = 0; poll < 5; poll++) {
    rerender({ ...props, selection: { ...readingSelection }, observation: readyObservation([{ ...changedFile }]) });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  }
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => first.resolve(textReview("verified bytes")));
  expect(result.current).toEqual(textReview("verified bytes"));
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(read).toHaveBeenCalledTimes(2);
  for (let poll = 0; poll < 5; poll++) {
    rerender({ ...props, observation: readyObservation([{ ...changedFile }]) });
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(result.current).toEqual(textReview("verified bytes"));
  }
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => next.resolve(textReview("edited bytes, same modified category")));
  expect(result.current).toEqual(textReview("edited bytes, same modified category"));
});

test("new native authority retains last-verified text while its replacement read is pending", async () => {
  const next = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockResolvedValueOnce(textReview("old")).mockReturnValueOnce(next.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  await act(async () => { await Promise.resolve(); });
  rerender({ ...props, observation: readyObservation([{ ...changedFile, pathId: "path-2" }], 2) });
  expect(result.current).toEqual({ kind: "updating", previous: textReview("old") });
  await act(async () => next.resolve(textReview("live", { pathId: "path-2" })));
  expect(result.current).toEqual(textReview("live", { pathId: "path-2" }));
});

test("a stale reread keeps verified text explicitly outdated until a new observation authorizes recovery", async () => {
  const current = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>()
    .mockResolvedValueOnce(textReview("verified"))
    .mockResolvedValueOnce({ kind: "stale_observation" })
    .mockReturnValueOnce(current.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(result.current).toEqual({ kind: "updating", previous: textReview("verified") });
  await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
  expect(read).toHaveBeenCalledTimes(2);
  rerender({ ...props, observation: readyObservation([{ ...changedFile, pathId: "path-2" }], 2) });
  expect(result.current).toEqual({ kind: "updating", previous: textReview("verified") });
  await act(async () => current.resolve(textReview("current", { pathId: "path-2" })));
  expect(result.current).toEqual(textReview("current", { pathId: "path-2" }));
});

test("unmount stops completion-paced rereads even if the last response arrives afterward", async () => {
  const pending = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(pending.promise);
  const { unmount } = renderHook(readReview, { initialProps: initial(reviewClient(read)) });
  unmount();
  await act(async () => pending.resolve(textReview("abandoned")));
  await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
  expect(read).toHaveBeenCalledTimes(1);
});

test.each(["entry", "generation", "file", "category"] as const)("a late failure cannot replace a new %s reading choice", async (change) => {
  const abandoned = deferred<ReviewResult>();
  const current = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValueOnce(abandoned.promise).mockReturnValueOnce(current.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  const next: Props = change === "entry" ? { ...props, entryId: "two", observation: readyObservation([changedFile], 1, "two") }
    : change === "generation" ? { ...props, generation: 1 }
    : change === "file" ? { ...props, selection: { ...readingSelection, stablePathId: "stable-2" }, observation: readyObservation([{ ...changedFile, stablePathId: "stable-2", pathId: "path-2" }]) }
    : { ...props, selection: { ...readingSelection, category: "staged" } };
  rerender(next);
  expect(result.current.kind).toBe("checking");
  await act(async () => abandoned.reject(new Error("Private transport payload")));
  expect(result.current.kind).toBe("checking");
  const response = textReview("new choice", { entryId: next.entryId, pathId: change === "file" ? "path-2" : "path-1", category: next.selection!.category });
  await act(async () => current.resolve(response));
  expect(result.current).toEqual(response);
});

test("a replacement client starts immediately and the abandoned client's completion cannot publish", async () => {
  const abandoned = deferred<ReviewResult>();
  const current = deferred<ReviewResult>();
  const props = initial(reviewClient(() => abandoned.promise));
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(current.promise);
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  rerender({ ...props, client: reviewClient(read) });
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => current.resolve(textReview("new client")));
  await act(async () => abandoned.resolve(textReview("old client")));
  expect(result.current).toEqual(textReview("new client"));
});

test("a disappeared category remains explicit even when the entire working tree is clean", async () => {
  const pending = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(pending.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  rerender({ ...props, observation: readyObservation([{ ...changedFile, unstaged: null }], 2) });
  expect(result.current.kind).toBe("no_remaining");
  rerender({ ...props, observation: readyObservation([], 3) });
  await act(async () => pending.resolve(textReview("removed category")));
  expect(result.current.kind).toBe("no_remaining");
  expect(read).toHaveBeenCalledTimes(1);
});

test.each([
  { kind: "unsupported", reason: "binary", identity: reviewIdentity },
  { kind: "unavailable", code: "changed_during_read", identity: reviewIdentity },
  { kind: "stale_observation" },
  { kind: "stale_selection" },
] as const)("$kind stays distinct from a successful empty comparison", async (reply) => {
  const { result } = renderHook(readReview, { initialProps: initial(reviewClient(async () => reply)) });
  await act(async () => { await Promise.resolve(); });
  expect(result.current.kind).toBe(reply.kind.startsWith("stale_") ? "checking" : reply.kind);
});

test("native and transport failures replace old source distinctly and retry without needing a category change", async () => {
  const read = vi.fn<RepositoryClient["reviewFile"]>()
    .mockResolvedValueOnce(textReview("initial source"))
    .mockResolvedValueOnce({ kind: "unavailable", code: "changed_during_read", identity: reviewIdentity })
    .mockRejectedValueOnce(new Error("Private transport detail"))
    .mockResolvedValueOnce(textReview("recovered source"));
  const { result } = renderHook(readReview, { initialProps: initial(reviewClient(read)) });
  await act(async () => { await Promise.resolve(); });
  expect(result.current).toEqual(textReview("initial source"));
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(result.current).toEqual({ kind: "unavailable", code: "changed_during_read", identity: reviewIdentity });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(result.current).toEqual({ kind: "transport_unavailable" });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(result.current).toEqual(textReview("recovered source"));
});

test("strict lifecycle replay still completes the initial review without overlapping requests", async () => {
  const reply = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(reply.promise);
  const { result } = renderHook(readReview, {
    initialProps: initial(reviewClient(read)),
    wrapper: ({ children }: { children: ReactNode }) => <StrictMode>{children}</StrictMode>,
  });
  await act(async () => reply.resolve(textReview("strict current")));
  expect(result.current).toEqual(textReview("strict current"));
  expect(read).toHaveBeenCalledTimes(1);
});

test("a prior entry observation never authorizes a file read in the next context", async () => {
  const abandoned = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(abandoned.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  rerender({ ...props, entryId: "two" });
  await act(async () => abandoned.resolve(textReview("old context")));
  expect(result.current.kind).toBe("checking");
  expect(read).toHaveBeenCalledTimes(1);
});

test("returning to a still-pending client coalesces rather than overlapping its abandoned read", async () => {
  const first = deferred<ReviewResult>();
  const last = deferred<ReviewResult>();
  const middle = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValueOnce(first.promise).mockReturnValueOnce(last.promise);
  const originalClient = reviewClient(read);
  const props = initial(originalClient);
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  rerender({ ...props, client: reviewClient(() => middle.promise) });
  rerender({ ...props, generation: 2 });
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => first.resolve(textReview("first client generation")));
  expect(result.current.kind).toBe("checking");
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => last.resolve(textReview("current client generation")));
  await act(async () => middle.reject(new Error("Middle client disconnected")));
  expect(result.current).toEqual(textReview("current client generation"));
});

test.each(["entry", "generation", "file", "category", "client"] as const)("last-verified text is never carried into a new %s scope", async (change) => {
  const nextReply = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockResolvedValueOnce(textReview("verified old scope")).mockReturnValueOnce(nextReply.promise);
  const props = initial(reviewClient(read));
  const { result, rerender } = renderHook(readReview, { initialProps: props });
  await act(async () => { await Promise.resolve(); });
  expect(result.current).toEqual(textReview("verified old scope"));
  const next: Props = change === "entry" ? { ...props, entryId: "two", observation: readyObservation([changedFile], 1, "two") }
    : change === "generation" ? { ...props, generation: 1 }
    : change === "file" ? { ...props, selection: { ...readingSelection, stablePathId: "stable-2" }, observation: readyObservation([{ ...changedFile, stablePathId: "stable-2", pathId: "path-2" }]) }
    : change === "category" ? { ...props, selection: { ...readingSelection, category: "staged" } }
    : { ...props, client: reviewClient(() => nextReply.promise) };
  rerender(next);
  expect(result.current).toEqual({ kind: "checking" });
});
