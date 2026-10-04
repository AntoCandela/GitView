/** Exercises compact opening, shared-context invalidation and hidden work through the real composition. */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { BeginCompanionReviewResult, CompanionClient, ReviewSurfaceClient, ReviewSurfaceSnapshot, SurfaceNotice } from "../../src/contracts/companion";
import { CompanionPanel } from "../../src/app/companion/CompanionPanel";
import { deferred } from "../support/deferred";
import { changedFile, readyObservation, textReview } from "../support/review";
import type { ReviewResult } from "../../src/contracts/diff";

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });
function fixture() {
  let surface: ReviewSurfaceSnapshot = {
    visible: true, openEpoch: "open-1", handoff: { revision: 0, pendingRequestId: null },
    workspace: { revision: 1, contextEpoch: "context-1", activeContextId: "one", restoring: false, persistenceError: null,
      entries: [{ id: "one", kind: "working_tree", repositoryLabel: "Repository", locationLabel: "Admitted worktree", head: { kind: "branch", name: "feature" }, availability: "available" }] },
    observation: readyObservation(),
    presentation: { revision: 1, locale: "en-US", appearanceTheme: "cream", iconTheme: "classic", review: { mode: "changes", theme: "match", lineMode: "scroll" }, persistenceError: false, menuLabels: { openGitView: "Open in GitView", quit: "Quit" } },
  };
  let listener: ((notice: SurfaceNotice) => void) | null = null;
  const transport: ReviewSurfaceClient = { bootstrap: async () => "companion", subscribe: async (next) => { listener = next; return () => { listener = null; }; }, snapshot: async () => surface };
  const client: CompanionClient = {
    begin: async () => ({ kind: "ready", surface }), dismiss: async () => undefined, quit: async () => undefined,
    requestHandoff: async () => ({ kind: "failed", code: "window_unavailable" }),
    selectContext: async () => ({ kind: "selected", snapshot: surface.workspace }),
    observeSelectedContext: async () => surface.observation!, reviewFile: async () => textReview("current source"),
  };
  return { client, transport, get surface() { return surface; }, update(next: ReviewSurfaceSnapshot, notice: SurfaceNotice) { surface = next; listener?.(notice); } };
}
async function settle() { await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); }); }

test("fresh opening stays Checking until its native ticket resolves, and hide rejects the old file response", async () => {
  const host = fixture();
  const begin = deferred<BeginCompanionReviewResult>();
  const review = deferred<ReviewResult>();
  host.client.begin = () => begin.promise;
  const read = vi.fn<CompanionClient["reviewFile"]>().mockReturnValue(review.promise);
  host.client.reviewFile = read;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  expect(screen.getByText("Checking current changes…")).toBeInTheDocument();
  expect(screen.queryByText("example.ts")).not.toBeInTheDocument();
  await act(async () => begin.resolve({ kind: "ready", surface: host.surface }));
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: /example.ts/ }));
  act(() => host.update({ ...host.surface, visible: false, openEpoch: "hidden" }, { kind: "visibility", visible: false, openEpoch: "hidden" }));
  await act(async () => review.resolve(textReview("obsolete source")));
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(screen.queryByText("obsolete source")).not.toBeInTheDocument();
  expect(read).toHaveBeenCalledTimes(1);
});

test("vanished selected category remains No remaining changes without selecting the staged alternative", async () => {
  const host = fixture();
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: /example.ts/ }));
  await settle();
  act(() => host.update({ ...host.surface, observation: readyObservation([{ ...changedFile, unstaged: null }], 2) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  expect(screen.getByText("No remaining changes")).toBeInTheDocument();
});

test("empty workspace keeps Open and Quit reachable without admission or management", async () => {
  const host = fixture();
  host.update({ ...host.surface, workspace: { ...host.surface.workspace, entries: [], activeContextId: null }, observation: null }, { kind: "presentation", revision: 1 });
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  expect(screen.getByRole("button", { name: "Open in GitView" })).toBeEnabled();
  expect(screen.getByRole("button", { name: "Quit" })).toBeEnabled();
  expect(screen.queryByRole("button", { name: /Open repository/i })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open in GitView" }));
  await settle();
  expect(screen.getByRole("alert")).toBeInTheDocument();
});

test("Escape dismisses through native authority rather than resetting the admitted context", async () => {
  const host = fixture();
  const dismiss = vi.fn<CompanionClient["dismiss"]>().mockResolvedValue(undefined);
  host.client.dismiss = dismiss;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  fireEvent.keyDown(screen.getByRole("button", { name: "Open in GitView" }), { key: "Escape" });
  expect(dismiss).toHaveBeenCalledTimes(1);
  expect(host.surface.workspace.activeContextId).toBe("one");
});

test("bare context is not clean and hands off without manufacturing a file identity", async () => {
  const host = fixture();
  host.update({ ...host.surface, observation: { kind: "bare", entryId: "one", observationRevision: 2 } }, { kind: "presentation", revision: 1 });
  const request = vi.fn<CompanionClient["requestHandoff"]>().mockResolvedValue({ kind: "applied", requestId: "context-only" });
  host.client.requestHandoff = request;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  expect(screen.getByText("No working tree")).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open in GitView" }));
  await settle();
  expect(request).toHaveBeenCalledExactlyOnceWith({ openEpoch: "open-1", contextEpoch: "context-1", selection: null });
  expect(screen.getByRole("button", { name: "Quit" })).toBeInTheDocument();
});

test("reopening fences a delayed opening ticket and retains no hidden periodic snapshot reads", async () => {
  const host = fixture();
  const first = deferred<BeginCompanionReviewResult>();
  const second = deferred<BeginCompanionReviewResult>();
  host.client.begin = vi.fn<CompanionClient["begin"]>().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const readSnapshot = vi.fn<ReviewSurfaceClient["snapshot"]>(async () => host.surface);
  host.transport.snapshot = readSnapshot;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  const old = host.surface;
  act(() => host.update({ ...old, visible: false, openEpoch: "hidden" }, { kind: "visibility", visible: false, openEpoch: "hidden" }));
  await settle();
  const hiddenReads = readSnapshot.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
  expect(readSnapshot).toHaveBeenCalledTimes(hiddenReads);
  act(() => host.update({ ...old, openEpoch: "open-2" }, { kind: "visibility", visible: true, openEpoch: "open-2" }));
  await settle();
  await act(async () => first.resolve({ kind: "ready", surface: old }));
  expect(screen.getByText("Checking current changes…")).toBeInTheDocument();
  await act(async () => second.resolve({ kind: "ready", surface: host.surface }));
  expect(screen.queryByText("Checking current changes…")).not.toBeInTheDocument();
});

test("admitted-context selection invalidates old review immediately and cannot discover another worktree", async () => {
  const host = fixture();
  const oldReview = deferred<ReviewResult>();
  host.client.reviewFile = () => oldReview.promise;
  const nextEntry = { ...host.surface.workspace.entries[0], id: "admitted-linked", locationLabel: "Linked worktree" };
  host.update({ ...host.surface, workspace: { ...host.surface.workspace, entries: [...host.surface.workspace.entries, nextEntry] } }, { kind: "presentation", revision: 1 });
  const select = vi.fn<CompanionClient["selectContext"]>(async (entryId) => {
    host.update({ ...host.surface, workspace: { ...host.surface.workspace, revision: 2, contextEpoch: "context-2", activeContextId: entryId },
      observation: readyObservation([], 2, entryId) }, { kind: "invalidate", workspaceRevision: 2, contextEpoch: "context-2", openEpoch: "open-1" });
    return { kind: "selected", snapshot: host.surface.workspace };
  });
  host.client.selectContext = select;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  fireEvent.change(screen.getByRole("combobox"), { target: { value: "admitted-linked" } });
  await settle();
  await act(async () => oldReview.resolve(textReview("wrong context")));
  expect(select).toHaveBeenCalledExactlyOnceWith("admitted-linked");
  expect(screen.queryByText("wrong context")).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Clean" })).toBeInTheDocument();
});

test("binary comparisons keep their unsupported outcome and compact controls remain read-only", async () => {
  const host = fixture();
  host.client.reviewFile = async () => ({ kind: "unsupported", reason: "binary", identity: {
    entryId: "one", pathId: "path-1", category: "unstaged", displayPath: "src/example.ts", contextLabel: "Repository",
    from: "index", to: "working_files", fromAbsent: false, toAbsent: false,
  } });
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  expect(screen.getByRole("combobox")).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  await settle();
  expect(screen.getByRole("heading", { name: "Preview unsupported" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Read-only file comparison" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Appearance" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stage" })).not.toBeInTheDocument();
});

test("a selected disappeared category hands off its issued identity rather than another category or context only", async () => {
  const host = fixture();
  host.client.reviewFile = async (entryId, _revision, pathId, category) => textReview("current source", { entryId, pathId, category });
  const request = vi.fn<CompanionClient["requestHandoff"]>().mockResolvedValue({ kind: "changed", requestId: "vanished" });
  host.client.requestHandoff = request;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  await settle();
  act(() => host.update({ ...host.surface, observation: readyObservation([{ ...changedFile, pathId: "path-2" }], 2) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  act(() => host.update({ ...host.surface, observation: readyObservation([], 3) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Open in GitView" }));
  await settle();
  expect(request).toHaveBeenCalledExactlyOnceWith({ openEpoch: "open-1", contextEpoch: "context-1",
    selection: { entryId: "one", stablePathId: "stable-1", pathId: "path-2", observationRevision: 2, category: "unstaged" } });
  expect(screen.getByRole("heading", { name: "No remaining changes" })).toBeInTheDocument();
});

test("reactivating a vanished unstaged category preserves its last issued authority while staged changes remain", async () => {
  const host = fixture();
  const request = vi.fn<CompanionClient["requestHandoff"]>().mockResolvedValue({ kind: "changed", requestId: "ceased-category" });
  host.client.requestHandoff = request;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  await settle();
  act(() => host.update({ ...host.surface, observation: readyObservation([{ ...changedFile, pathId: "new-staged-token", unstaged: null }], 2) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  fireEvent.click(screen.getByRole("button", { name: "Unstaged comparison" }));
  fireEvent.click(screen.getByRole("button", { name: "Open in GitView" }));
  await settle();
  expect(request).toHaveBeenCalledExactlyOnceWith({ openEpoch: "open-1", contextEpoch: "context-1",
    selection: { entryId: "one", stablePathId: "stable-1", pathId: "path-1", observationRevision: 1, category: "unstaged" } });
});

test("a same-scope ready ticket grants freshness after newer workspace and handoff revisions without regressing them", async () => {
  const host = fixture();
  const opening = deferred<BeginCompanionReviewResult>();
  const original = host.surface;
  const begin = vi.fn<CompanionClient["begin"]>().mockReturnValue(opening.promise);
  host.client.begin = begin;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  act(() => host.update({ ...original, workspace: { ...original.workspace, revision: 2,
    entries: [{ ...original.workspace.entries[0], repositoryLabel: "Renamed repository" }] },
    handoff: { revision: 2, pendingRequestId: null } }, { kind: "handoff", revision: 2, requestId: null }));
  await settle();
  await act(async () => opening.resolve({ kind: "ready", surface: original }));
  expect(screen.queryByText("Checking current changes…")).not.toBeInTheDocument();
  expect(screen.getByRole("option", { name: /Renamed repository/ })).toBeInTheDocument();
  expect(begin).toHaveBeenCalledTimes(1);
});

test("an initial cached read failure recovers with bounded bootstrap reads", async () => {
  const host = fixture();
  const snapshots = vi.fn<ReviewSurfaceClient["snapshot"]>().mockRejectedValueOnce(new Error("transport unavailable")).mockImplementation(async () => host.surface);
  host.transport.snapshot = snapshots;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByRole("button", { name: "Open in GitView" })).toBeEnabled();
  expect(screen.queryByText("Checking current changes…")).not.toBeInTheDocument();
});

test("missed reveal notices recover on window focus without any hidden periodic reads", async () => {
  const host = fixture();
  let authoritative = { ...host.surface, visible: false, openEpoch: "hidden" };
  const snapshots = vi.fn<ReviewSurfaceClient["snapshot"]>(async () => authoritative);
  host.transport.snapshot = snapshots;
  host.client.begin = async () => ({ kind: "ready", surface: authoritative });
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  const hiddenReads = snapshots.mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(snapshots).toHaveBeenCalledTimes(hiddenReads);
  authoritative = { ...host.surface, openEpoch: "revealed" };
  act(() => window.dispatchEvent(new Event("focus")));
  await settle();
  expect(screen.getByRole("button", { name: "Open in GitView" })).toBeEnabled();
});

test("initial cached read failures stop after the bounded bootstrap attempts", async () => {
  const host = fixture();
  const snapshots = vi.fn<ReviewSurfaceClient["snapshot"]>().mockRejectedValue(new Error("unavailable"));
  host.transport.snapshot = snapshots;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
  expect(snapshots).toHaveBeenCalledTimes(3);
});

test("an unavailable opening retries only after native recovery publishes a newer usable observation", async () => {
  const host = fixture();
  host.update({ ...host.surface, observation: { kind: "unavailable", entryId: "one", observationRevision: 2, errorCode: "inaccessible" } },
    { kind: "presentation", revision: 1 });
  const begin = vi.fn<CompanionClient["begin"]>().mockResolvedValueOnce({ kind: "unavailable", code: "unavailable" })
    .mockImplementation(async () => ({ kind: "ready", surface: host.surface }));
  host.client.begin = begin;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(begin).toHaveBeenCalledTimes(1);
  act(() => host.update({ ...host.surface, observation: readyObservation([], 3) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  expect(begin).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("heading", { name: "Clean" })).toBeInTheDocument();
});

test("recovery arriving before a failed opening reply still completes the same opening", async () => {
  const host = fixture();
  host.update({ ...host.surface, observation: { kind: "unavailable", entryId: "one", observationRevision: 2, errorCode: "inaccessible" } },
    { kind: "presentation", revision: 1 });
  const failed = deferred<BeginCompanionReviewResult>();
  const begin = vi.fn<CompanionClient["begin"]>().mockReturnValueOnce(failed.promise)
    .mockImplementation(async () => ({ kind: "ready", surface: host.surface }));
  host.client.begin = begin;
  render(<CompanionPanel client={host.client} surfaceClient={host.transport} />);
  await settle();
  act(() => host.update({ ...host.surface, observation: readyObservation([], 3) },
    { kind: "invalidate", workspaceRevision: 1, contextEpoch: "context-1", openEpoch: "open-1" }));
  await settle();
  expect(begin).toHaveBeenCalledTimes(1);
  await act(async () => failed.resolve({ kind: "unavailable", code: "unavailable" }));
  await settle();
  expect(screen.getByRole("heading", { name: "Clean" })).toBeInTheDocument();
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(begin).toHaveBeenCalledTimes(2);
});
