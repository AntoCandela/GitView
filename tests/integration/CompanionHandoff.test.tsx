/** Exercises discovery/claim application fences without treating main focus as delivery. */
import "@testing-library/jest-dom/vitest";
import { afterEach, expect, test, vi } from "vitest";
import { act, cleanup, fireEvent, render, renderHook, screen } from "@testing-library/react";
import type { ClaimReviewHandoffResult, PendingReviewHandoffSnapshot, ReviewHandoffClient, ReviewHandoffTarget } from "../../src/contracts/companion";
import { ReviewHandoffReceiver } from "../../src/app/companion/ReviewHandoffReceiver";
import { deferred } from "../support/deferred";
import { Workspace } from "../../src/app/Workspace";
import type { ReviewSurfaceClient, ReviewSurfaceSnapshot } from "../../src/contracts/companion";
import { readyObservation, reviewClient, textReview } from "../support/review";
import type { ReviewResult } from "../../src/contracts/diff";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { ReviewSurfaceConnection } from "../../src/app/companion/useReviewSurface";
import { useMainReviewSurface } from "../../src/app/companion/useMainReviewSurface";
import type { SurfaceNotice } from "../../src/contracts/companion";

afterEach(cleanup);

const target: ReviewHandoffTarget = { kind: "live", entryId: "one", selection: { stablePathId: "file-a", displayPath: "a.ts", category: "staged" }, pathId: "token-a", observationRevision: 3 };
function pending(requestId: string, revision: number): PendingReviewHandoffSnapshot {
  return { revision, pending: { requestId, contextEpoch: "context", entryId: "one", target, sourceOpenEpoch: "open", phase: "pending" } };
}
function claimed(requestId: string, handoffRevision: number, nextTarget = target): ClaimReviewHandoffResult {
  return { kind: "claimed", requestId, contextEpoch: "context", handoffRevision, target: nextTarget, remainingMs: 5000 };
}
async function settle() { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); }

test("same-context reversed claims apply only B's exact native target and acknowledge after application", async () => {
  const first = deferred<ClaimReviewHandoffResult>();
  const second = deferred<ClaimReviewHandoffResult>();
  let discovery = pending("A", 1);
  const applied: ReviewHandoffTarget[] = [];
  const ack = vi.fn<ReviewHandoffClient["ack"]>(async () => { expect(applied).toHaveLength(1); return { kind: "applied" }; });
  const receiver = new ReviewHandoffReceiver({ pending: async () => discovery, claim: (id) => id === "A" ? first.promise : second.promise, ack }, (value) => applied.push(value));
  receiver.reconcile("context", 1, "A");
  await settle();
  discovery = pending("B", 2);
  receiver.reconcile("context", 2, "B");
  await settle();
  const exactTarget = { ...target!, kind: "no_remaining" as const, entryId: "one", selection: { stablePathId: "file-b", displayPath: "b.ts", category: "unstaged" as const } };
  second.resolve(claimed("B", 3, exactTarget));
  await settle();
  first.resolve(claimed("A", 2));
  await settle();
  expect(applied).toEqual([exactTarget]);
  expect(ack).toHaveBeenCalledExactlyOnceWith("B", "context", "changed");
});

test("a claim delayed past invocation-time expiry cannot reset main even when context is unchanged", async () => {
  const claim = deferred<ClaimReviewHandoffResult>();
  let now = 10;
  const apply = vi.fn();
  const ack = vi.fn<ReviewHandoffClient["ack"]>();
  const receiver = new ReviewHandoffReceiver({ pending: async () => pending("A", 1), claim: () => claim.promise, ack }, apply, () => now);
  receiver.reconcile("context", 1, "A");
  await settle();
  now = 5011;
  claim.resolve(claimed("A", 2));
  await settle();
  expect(apply).not.toHaveBeenCalled();
  expect(ack).not.toHaveBeenCalled();
});

test("cached retirement with null pending cancels a claim after a missed retirement notice", async () => {
  const claim = deferred<ClaimReviewHandoffResult>();
  const apply = vi.fn();
  const ack = vi.fn<ReviewHandoffClient["ack"]>();
  const receiver = new ReviewHandoffReceiver({ pending: async () => pending("A", 1), claim: () => claim.promise, ack }, apply);
  receiver.reconcile("context", 1, "A");
  await settle();
  receiver.reconcile("context", 3, null);
  claim.resolve(claimed("A", 2));
  await settle();
  expect(apply).not.toHaveBeenCalled();
  expect(ack).not.toHaveBeenCalled();
});

test("discovery alone and a stale claim never mutate main or acknowledge a focused source", async () => {
  const apply = vi.fn();
  const ack = vi.fn<ReviewHandoffClient["ack"]>();
  const receiver = new ReviewHandoffReceiver({ pending: async () => pending("A", 1), claim: async () => ({ kind: "stale", code: "delivery_timeout" }), ack }, apply);
  receiver.reconcile("context", 1, "A");
  await settle();
  expect(apply).not.toHaveBeenCalled();
  expect(ack).not.toHaveBeenCalled();
});

test("an external context epoch invalidates a delayed claim before any main selection reset", async () => {
  const claim = deferred<ClaimReviewHandoffResult>();
  const apply = vi.fn();
  const receiver = new ReviewHandoffReceiver({ pending: async () => pending("A", 1), claim: () => claim.promise, ack: async () => ({ kind: "applied" }) }, apply);
  receiver.reconcile("context", 1, "A");
  await settle();
  receiver.reconcile("other-context", 1, null);
  claim.resolve(claimed("A", 2));
  await settle();
  expect(apply).not.toHaveBeenCalled();
});

test("the mounted main workbench keeps its own choice until a claim synchronously applies the exact native target", async () => {
  const claim = deferred<ClaimReviewHandoffResult>();
  const client = reviewClient(async () => textReview("main selection"));
  const workspace = { revision: 1, contextEpoch: "context", activeContextId: "one", restoring: false, persistenceError: null,
    entries: [{ id: "one", kind: "working_tree" as const, repositoryLabel: "Repository", locationLabel: "Worktree", head: { kind: "branch" as const, name: "main" }, availability: "available" as const }] };
  client.snapshot = async () => workspace;
  const surface: ReviewSurfaceSnapshot = { workspace, visible: true, openEpoch: "main-open", observation: readyObservation(), presentation: null,
    handoff: { revision: 1, pendingRequestId: "A" } };
  const transport: ReviewSurfaceClient = { bootstrap: async () => "main", snapshot: async () => surface, subscribe: async () => () => undefined };
  const exact: ReviewHandoffTarget = { kind: "no_remaining", entryId: "one", selection: { stablePathId: "native-b", displayPath: "vanished.ts", category: "staged" } };
  const ack = vi.fn<ReviewHandoffClient["ack"]>(async () => {
    expect(screen.getByRole("heading", { name: "vanished.ts" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "No remaining changes" })).toBeInTheDocument();
    return { kind: "applied" };
  });
  render(<Workspace client={client} surfaceClient={transport} handoffClient={{ pending: async () => pending("A", 1), claim: () => claim.promise, ack }} />);
  await act(async () => { await settle(); });
  fireEvent.click(screen.getByRole("button", { name: "Expand all" }));
  fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  await act(async () => { await settle(); });
  expect(screen.getByRole("heading", { name: "src/example.ts" })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "vanished.ts" })).not.toBeInTheDocument();
  await act(async () => claim.resolve(claimed("A", 1, exact)));
  expect(screen.getByRole("heading", { name: "vanished.ts" })).toBeInTheDocument();
  expect(ack).toHaveBeenCalledExactlyOnceWith("A", "context", "changed");
});

test("a lower-revision discovery reply cannot supersede a newer pending handoff", async () => {
  const stale = deferred<PendingReviewHandoffSnapshot>();
  const apply = vi.fn();
  const client: ReviewHandoffClient = {
    pending: vi.fn<ReviewHandoffClient["pending"]>().mockReturnValueOnce(stale.promise).mockResolvedValue(pending("B", 3)),
    claim: async (id) => claimed(id, 3), ack: async () => ({ kind: "applied" }),
  };
  const receiver = new ReviewHandoffReceiver(client, apply);
  receiver.reconcile("context", 1, "A");
  receiver.reconcile("context", 3, "B");
  await settle();
  stale.resolve(pending("A", 1));
  await settle();
  expect(apply).toHaveBeenCalledExactlyOnceWith(target, "context", "B");
});

test("context-only claims clear main review only after a valid claim and busy claims preserve its selection", async () => {
  const apply = vi.fn();
  const ack = vi.fn<ReviewHandoffClient["ack"]>().mockResolvedValue({ kind: "applied" });
  const client: ReviewHandoffClient = {
    pending: async () => pending("A", 1),
    claim: async () => ({ kind: "busy", code: "busy" }),
    ack,
  };
  const busy = new ReviewHandoffReceiver(client, apply);
  busy.reconcile("context", 1, "A");
  await settle();
  expect(apply).not.toHaveBeenCalled();
  expect(ack).not.toHaveBeenCalled();
  busy.dispose();
  const contextOnly = new ReviewHandoffReceiver({ ...client, claim: async () => claimed("A", 1, null) }, apply);
  contextOnly.reconcile("context", 1, "A");
  await settle();
  expect(apply).toHaveBeenCalledExactlyOnceWith(null, "context", "A");
  expect(ack).toHaveBeenCalledExactlyOnceWith("A", "context", "applied");
});

test("live claims read their exact native authority even when cached main observation is older and empty", async () => {
  const claim = deferred<ClaimReviewHandoffResult>();
  const file = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValue(file.promise);
  const client = reviewClient(read);
  const workspace = { revision: 1, contextEpoch: "context", activeContextId: "one", restoring: false, persistenceError: null,
    entries: [{ id: "one", kind: "working_tree" as const, repositoryLabel: "Repository", locationLabel: "Worktree", head: { kind: "branch" as const, name: "main" }, availability: "available" as const }] };
  client.snapshot = async () => workspace;
  const surface: ReviewSurfaceSnapshot = { workspace, visible: true, openEpoch: "main-open", observation: readyObservation([], 1), presentation: null,
    handoff: { revision: 1, pendingRequestId: "A" } };
  const transport: ReviewSurfaceClient = { bootstrap: async () => "main", snapshot: async () => surface, subscribe: async () => () => undefined };
  const exact: ReviewHandoffTarget = { kind: "live", entryId: "one", pathId: "claimed-token", observationRevision: 7,
    selection: { stablePathId: "native-new", displayPath: "new.ts", category: "unstaged" } };
  const ack = vi.fn<ReviewHandoffClient["ack"]>(async () => {
    expect(screen.getByRole("heading", { name: "new.ts" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "No remaining changes" })).not.toBeInTheDocument();
    return { kind: "applied" };
  });
  render(<Workspace client={client} surfaceClient={transport} handoffClient={{ pending: async () => pending("A", 1), claim: () => claim.promise, ack }} />);
  await act(async () => { await settle(); });
  await act(async () => claim.resolve(claimed("A", 1, exact)));
  expect(read).toHaveBeenCalledExactlyOnceWith("one", 7, "claimed-token", "unstaged");
  expect(ack).toHaveBeenCalledExactlyOnceWith("A", "context", "applied");
  await act(async () => file.resolve(textReview("exact native target", { entryId: "one", pathId: "claimed-token", displayPath: "new.ts" })));
  expect(screen.getByText("exact native target")).toBeInTheDocument();
});

test("incoherent main workspace invalidates preparation immediately and defers claiming until it is authoritative", async () => {
  const claim = vi.fn<ReviewHandoffClient["claim"]>().mockResolvedValue(claimed("A", 1));
  const apply = vi.fn();
  const receiver = new ReviewHandoffReceiver({ pending: async () => pending("A", 1), claim, ack: async () => ({ kind: "applied" }) }, apply);
  receiver.reconcile("context", 1, "A", false);
  await settle();
  expect(claim).not.toHaveBeenCalled();
  expect(apply).not.toHaveBeenCalled();
  receiver.reconcile("context", 1, "A", true);
  await settle();
  expect(claim).toHaveBeenCalledExactlyOnceWith("A", "context");
  expect(apply).toHaveBeenCalledExactlyOnceWith(target, "context", "A");
});

test("main subscription waits for the authoritative workspace read after a context notice before consuming its claim", async ({ onTestFinished }) => {
  const delayed = deferred<ReviewSurfaceSnapshot>();
  const workspace = { revision: 1, contextEpoch: "previous", activeContextId: "one", restoring: false, persistenceError: null, entries: [] };
  const previous: ReviewSurfaceSnapshot = { workspace, visible: true, openEpoch: "main", observation: null, presentation: null,
    handoff: { revision: 0, pendingRequestId: null } };
  let listener: ((notice: SurfaceNotice) => void) | undefined;
  const transport: ReviewSurfaceClient = { bootstrap: async () => "main",
    subscribe: async (receive) => { listener = receive; return () => { listener = undefined; }; },
    snapshot: vi.fn<ReviewSurfaceClient["snapshot"]>().mockResolvedValueOnce(previous).mockReturnValue(delayed.promise) };
  const connection = new ReviewSurfaceConnection(transport);
  const claim = vi.fn<ReviewHandoffClient["claim"]>().mockResolvedValue(claimed("A", 1));
  const apply = vi.fn();
  renderHook(() => useMainReviewSurface(connection, {
    pending: async () => pending("A", 1), claim, ack: async () => ({ kind: "applied" }),
  }, () => undefined, apply));
  const stop = connection.start();
  onTestFinished(stop);
  await act(async () => { await settle(); });
  act(() => {
    listener?.({ kind: "invalidate", contextEpoch: "context", workspaceRevision: 2, openEpoch: "main" });
    listener?.({ kind: "handoff", revision: 1, requestId: "A" });
  });
  await act(async () => { await settle(); });
  expect(claim).not.toHaveBeenCalled();
  expect(apply).not.toHaveBeenCalled();
  await act(async () => delayed.resolve({ ...previous, workspace: { ...workspace, revision: 2, contextEpoch: "context" },
    handoff: { revision: 1, pendingRequestId: "A" } }));
  expect(claim).toHaveBeenCalledExactlyOnceWith("A", "context");
  expect(apply).toHaveBeenCalledExactlyOnceWith({ requestId: "A", contextEpoch: "context", target, observationRevision: -1 });
});

test("a retired surface read cannot release a replacement lifetime's single-flight read", async ({ onTestFinished }) => {
  const retired = deferred<ReviewSurfaceSnapshot>();
  const replacement = deferred<ReviewSurfaceSnapshot>();
  const workspace = { revision: 2, contextEpoch: "current", activeContextId: null, restoring: false, persistenceError: null, entries: [] };
  const current: ReviewSurfaceSnapshot = { workspace, visible: false, openEpoch: "hidden", observation: null, presentation: null,
    handoff: { revision: 0, pendingRequestId: null } };
  const snapshots = vi.fn<ReviewSurfaceClient["snapshot"]>().mockReturnValueOnce(retired.promise)
    .mockReturnValueOnce(replacement.promise).mockResolvedValue(current);
  const connection = new ReviewSurfaceConnection({ bootstrap: async () => "main",
    subscribe: async () => () => undefined, snapshot: snapshots });
  const stopRetired = connection.start();
  await settle();
  stopRetired();
  const stopCurrent = connection.start();
  onTestFinished(stopCurrent);
  await settle();

  retired.resolve({ ...current, workspace: { ...workspace, revision: 1, contextEpoch: "retired" } });
  await settle();
  connection.refresh();
  connection.refresh();
  expect(snapshots).toHaveBeenCalledTimes(2);
  expect(connection.getSnapshot().snapshot).toBeNull();

  replacement.resolve(current);
  await settle();
  expect(connection.getSnapshot().snapshot).toEqual(current);
  expect(connection.getSnapshot().unavailable).toBe(false);
  expect(snapshots).toHaveBeenCalledTimes(3);
});
