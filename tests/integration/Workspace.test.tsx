/**
 * Exercises workspace behavior through rendered controls and a replaceable desktop client.
 * Deferred replies expose selection ordering and recovery races without native I/O.
 */

import "@testing-library/jest-dom/vitest";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { act, cleanup, fireEvent, render as renderView, screen, waitFor } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import type { ReactElement } from "react";
import { Workspace } from "../../src/app/Workspace";
import { deferred } from "../support/deferred";
import { changedFile, readyObservation, textReview } from "../support/review";
import type {
  OpenOutcome,
  RepositoryClient,
  RepositoryMutationOutcome,
  SelectOutcome,
  WorkspaceSnapshot,
} from "../../src/contracts/repositories";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});
beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});
beforeEach(() => {
  // jsdom has no layout; stable dimensions let the virtualizer mount the tested rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(
    function (this: HTMLElement) {
      return this.classList.contains("repository-list") ? 480 : 44;
    },
  );
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(296);
});

/** Repository-management scenarios explicitly open and focus the compact disclosure. */
function renderWorkspace(element: ReactElement) {
  const view = renderView(element);
  const selector = screen.getByRole("button", { name: /^Current repository:/ });
  selector.focus();
  fireEvent.click(selector);
  return view;
}

const first: WorkspaceSnapshot["entries"][number] = {
  id: "one",
  kind: "working_tree",
  repositoryLabel: "atlas",
  locationLabel: "/work/atlas",
  head: { kind: "branch", name: "main" },
  availability: "available",
};
const second: WorkspaceSnapshot["entries"][number] = {
  id: "two",
  kind: "bare",
  repositoryLabel: "ledger",
  locationLabel: "/work/ledger.git",
  head: { kind: "unborn", name: "main" },
  availability: "available",
};

/** Starts from host-like state; individual scenarios override transport timing or outcomes. */
function fakeClient(initial: WorkspaceSnapshot): RepositoryClient {
  let snapshot = initial;
  return {
    snapshot: async () => snapshot,
    preferredLanguages: async () => ({ languages: [] }),
    openChosenRepository: async () => ({
      kind: "cancelled",
      snapshot,
    }),
    selectContext: async (entryId) => {
      snapshot = { ...snapshot, revision: snapshot.revision + 1, activeContextId: entryId };
      return { kind: "selected", snapshot };
    },
    renameRepository: async (entryId, displayName) => {
      if (!snapshot.entries.some((entry) => entry.id === entryId))
        return { kind: "not_found", snapshot };
      snapshot = {
        ...snapshot,
        revision: snapshot.revision + 1,
        entries: snapshot.entries.map((entry) =>
          entry.id === entryId ? { ...entry, repositoryLabel: displayName.trim() } : entry,
        ),
      };
      return { kind: "updated", snapshot };
    },
    removeRepository: async (entryId) => {
      if (!snapshot.entries.some((entry) => entry.id === entryId))
        return { kind: "not_found", snapshot };
      const entries = snapshot.entries.filter((entry) => entry.id !== entryId);
      snapshot = {
        ...snapshot,
        revision: snapshot.revision + 1,
        entries,
        activeContextId: snapshot.activeContextId === entryId
          ? entries.find((entry) => entry.availability === "available")?.id ?? null
          : snapshot.activeContextId,
      };
      return { kind: "updated", snapshot };
    },
    refreshEntryAvailability: async () => snapshot,
    observeSelectedContext: async (entryId) => {
      const entry = snapshot.entries.find((candidate) => candidate.id === entryId);
      const base = { entryId, observationRevision: 1 };
      if (entry?.kind === "bare") return { ...base, kind: "bare" };
      if (entry?.availability === "unavailable")
        return { ...base, kind: "unavailable", errorCode: "inaccessible" };
      if (entry?.kind === "unknown") return { ...base, kind: "checking" };
      return { ...base, kind: "ready", files: [] };
    },
    reviewFile: async () => ({ kind: "stale_observation" }),
    reviewCommitFile: async () => ({ kind: "stale_selection" }),
    listRepositoryFiles: async (entryId) => {
      if (snapshot.activeContextId !== entryId) return { kind: "stale_selection" };
      const entry = snapshot.entries.find((candidate) => candidate.id === entryId);
      if (entry?.kind !== "working_tree" || entry.availability !== "available")
        return { kind: "unavailable", code: "inaccessible", message: "Repository files unavailable." };
      return { kind: "files", entryId, listingId: "empty-listing", directoryId: null, cursor: null, directories: [], files: [] };
    },
    reviewRepositoryFile: async () => ({ kind: "stale_selection" }),
    historyPage: async (entryId) => ({
      kind: "page", page: {
        upstream: { state: "unborn", freshness: "unavailable", branch: null, upstream: null, ahead: 0, behind: 0, incoming: null, outgoing: null },
        entryId, cursor: null, commits: [], refs: [], hasMore: false, completeness: "complete",
        head: { scope: "worktree", state: "unborn", branch: null, oid: null },
      },
    }),
    listContexts: async () => ({ kind: "options", branches: [{ name: "main" }], worktrees: [] }),
    selectWorktree: async () => ({ kind: "not_found", snapshot }),
    upstreamFiles: async () => ({ kind: "unavailable", code: "stale_selection", message: "Selection changed." }),
    commitFiles: async () => ({ kind: "error", code: "stale_selection", message: "Selection changed." }),
  };
}


test("sidebar labels branches and describes full paths without a visible path row", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first, second],
    activeContextId: "one",
  };
  renderWorkspace(<Workspace client={fakeClient(initial)} />);

  const atlas = await screen.findByRole("button", { name: "atlas main" });
  expect(atlas).toHaveAttribute("aria-current", "true");
  expect(atlas).toHaveAccessibleDescription("/work/atlas");
  const ledger = screen.getByRole("button", { name: /ledger Unborn · main/i });
  expect(ledger).toHaveAccessibleDescription("/work/ledger.git");
  expect(screen.getByRole("button", { name: "Open repository" })).toBeEnabled();
});

test("search filters repositories without changing the active selection and clearing restores them", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first, second],
    activeContextId: "one",
  };
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={fakeClient(initial)} />);

  await screen.findByRole("button", { name: "atlas main" });
  await user.type(
    screen.getByRole("searchbox", { name: "Search repositories" }),
    "LEDGER",
  );
  expect(
    screen.queryByRole("button", { name: "atlas main" }),
  ).not.toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /ledger Unborn · main/i }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Clear search" }));
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveAttribute(
    "aria-current",
    "true",
  );
});

test("arrow navigation focuses the next repository without changing selection until activation", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first, second],
    activeContextId: "one",
  };
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={fakeClient(initial)} />);

  await screen.findByRole("button", { name: "atlas main" });
  await user.tab();
  await user.tab();
  await user.tab();
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveFocus();
  await user.keyboard("{ArrowDown}");
  expect(
    screen.getByRole("button", { name: /ledger Unborn · main/i }),
  ).toHaveFocus();
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await user.keyboard("{Enter}");
  expect(
    await screen.findByRole("button", { name: "Current repository: ledger" }),
  ).toBeInTheDocument();
});

test("failed open preserves existing context and permits another attempt", async () => {
  const original = { revision: 1, entries: [first], activeContextId: null, restoring: false, persistenceError: null };
  const client = fakeClient(original);
  client.openChosenRepository = vi
    .fn()
    .mockResolvedValueOnce({
      kind: "rejected",
      code: "not_repository",
      message: "Choose a Git repository.",
      snapshot: original,
    })
    .mockResolvedValueOnce({
      kind: "opened",
      entryId: "two",
      snapshot: {
        revision: 2,
        restoring: false,
        persistenceError: null,
        entries: [first, second],
        activeContextId: null,
      },
    });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);

  await user.click(
    await screen.findByRole("button", { name: /open repository/i }),
  );
  expect(await screen.findByRole("alert")).toBeVisible();
  expect(screen.getByRole("button", { name: /^atlas main/i })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /open repository/i }));
  expect(
    await screen.findByRole("button", { name: /^ledger Unborn/i }),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^ledger Unborn/i })).not.toHaveAttribute(
    "aria-current",
  );
});

test("transport failure keeps opened entries and the retry action", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first],
    activeContextId: null,
  };
  const client = fakeClient(initial);
  client.openChosenRepository = vi
    .fn()
    .mockRejectedValue(new Error("ipc offline"));
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);

  await user.click(
    await screen.findByRole("button", { name: /open repository/i }),
  );
  expect(await screen.findByRole("alert")).toBeVisible();
  expect(screen.getByRole("button", { name: /^atlas main/i })).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: /open repository/i }),
  ).toBeEnabled();
});

test.each([
  {
    intent: "selection",
    entries: [first, second],
    activeContextId: "two",
    currentLabel: "ledger",
    completeIntent: async (user: UserEvent) => {
      await user.click(screen.getByRole("button", { name: /^ledger Unborn/i }));
      await screen.findByRole("button", { name: "Current repository: ledger" });
      await user.click(screen.getByRole("button", { name: /^Current repository:/ }));
    },
  },
  {
    intent: "rename",
    entries: [{ ...first, repositoryLabel: "Atlas renamed" }, second],
    activeContextId: "one",
    currentLabel: "Atlas renamed",
    completeIntent: async (user: UserEvent) => {
      await openRepositoryActions(user, "atlas");
      await user.click(screen.getByRole("menuitem", { name: "Rename" }));
      await user.clear(screen.getByRole("textbox", { name: "Display name" }));
      await user.type(screen.getByRole("textbox", { name: "Display name" }), "Atlas renamed");
      await user.keyboard("{Enter}");
      await screen.findByRole("button", { name: "Atlas renamed main" });
    },
  },
  {
    intent: "removal",
    entries: [second],
    activeContextId: "two",
    currentLabel: "ledger",
    completeIntent: async (user: UserEvent) => {
      await openRepositoryActions(user, "atlas");
      await user.click(screen.getByRole("menuitem", { name: "Remove from sidebar" }));
      await screen.findByRole("button", { name: "Current repository: ledger" });
    },
  },
])("an admission completed after a newer $intent appears without undoing that intent", async ({
  entries, activeContextId, currentLabel, completeIntent,
}) => {
  const newcomer = { ...first, id: "three", repositoryLabel: "newcomer", locationLabel: "/work/newcomer" };
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  let nativeSnapshot = initial;
  const admission = deferred<OpenOutcome>();
  const client = fakeClient(initial);
  client.snapshot = async () => nativeSnapshot;
  client.openChosenRepository = async () => admission.promise;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await screen.findByRole("button", { name: "atlas main" });
  await user.click(screen.getByRole("button", { name: "Open repository" }));
  await completeIntent(user);

  // The host admits only after the newer intent and its availability reply have finished.
  nativeSnapshot = { ...initial, revision: 3, entries: [...entries, newcomer], activeContextId };
  await act(async () => {
    admission.resolve({ kind: "opened", entryId: "three", snapshot: nativeSnapshot });
  });
  expect(await screen.findByRole("button", { name: "newcomer main" })).not.toHaveAttribute("aria-current");
  expect(screen.getByRole("button", { name: `Current repository: ${currentLabel}` })).toBeVisible();
});

test.each(["opened", "lost"] as const)(
  "an %s open reply reconciles after a pending removal without restoring the removed selection",
  async (reply) => {
    const newcomer = { ...first, id: "three", repositoryLabel: "newcomer", locationLabel: "/work/newcomer" };
    const initial: WorkspaceSnapshot = {
      revision: 1, entries: [first, second], activeContextId: "one",
      restoring: false, persistenceError: null,
    };
    const removed: WorkspaceSnapshot = { ...initial, revision: 2, entries: [second], activeContextId: "two" };
    let nativeSnapshot = initial;
    const admission = deferred<OpenOutcome>();
    const removal = deferred<RepositoryMutationOutcome>();
    const client = fakeClient(initial);
    client.snapshot = async () => nativeSnapshot;
    client.openChosenRepository = async () => admission.promise;
    client.removeRepository = async () => removal.promise;
    const user = userEvent.setup();
    renderWorkspace(<Workspace client={client} />);
    await screen.findByRole("button", { name: "atlas main" });
    await user.click(screen.getByRole("button", { name: "Open repository" }));
    await openRepositoryActions(user, "atlas");
    await user.click(screen.getByRole("menuitem", { name: "Remove from sidebar" }));

    // Removal committed first but its reply is held; admission is absent from that older reply.
    nativeSnapshot = { ...removed, revision: 3, entries: [second, newcomer] };
    await act(async () => {
      if (reply === "opened")
        admission.resolve({ kind: "opened", entryId: "three", snapshot: nativeSnapshot });
      else
        admission.reject(new Error("lost reply"));
    });
    expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "newcomer main" })).not.toBeInTheDocument();

    await act(async () => { removal.resolve({ kind: "updated", snapshot: removed }); });
    expect(await screen.findByRole("button", { name: "newcomer main" })).not.toHaveAttribute("aria-current");
    expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "atlas main" })).not.toBeInTheDocument();
  },
);

test("selection requests execute in intent order and stale A cannot replace B", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first, second],
    activeContextId: null,
  };
  const firstSelection = deferred<SelectOutcome>();
  const select = vi.fn().mockImplementation((id: string) =>
    id === "one"
      ? firstSelection.promise
      : Promise.resolve({
          kind: "selected",
          snapshot: { ...initial, revision: 3, activeContextId: "two" },
        }),
  );
  const client = fakeClient(initial);
  client.selectContext = select;
  client.refreshEntryAvailability = async () => ({
    ...initial,
    revision: 4,
    activeContextId: "two",
  });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);

  await user.click(await screen.findByRole("button", { name: /^atlas main/i }));
  await user.click(screen.getByRole("button", { name: /^Current repository:/ }));
  await user.click(screen.getByRole("button", { name: /^ledger Unborn/i }));
  expect(screen.getByRole("status")).toBeInTheDocument();
  expect(select).toHaveBeenCalledTimes(1);
  // The older host selection completes only after both clicks have entered the queue.
  firstSelection.resolve({
    kind: "selected",
    snapshot: { ...initial, revision: 2, activeContextId: "one" },
  });
  await waitFor(() => expect(select).toHaveBeenCalledTimes(2));
  expect(
    await screen.findByRole("button", { name: "Current repository: ledger" }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole("heading", { name: "No working tree" }),
  ).toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: "Current repository: atlas" }),
  ).not.toBeInTheDocument();
});

test("late recovery from a failed selection cannot restore superseded context", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1,
    restoring: false,
    persistenceError: null,
    entries: [first, second],
    activeContextId: null,
  };
  const recovery = deferred<WorkspaceSnapshot>();
  const client = fakeClient(initial);
  client.snapshot = vi
    .fn()
    .mockResolvedValueOnce(initial)
    .mockReturnValueOnce(recovery.promise)
    .mockRejectedValue(new Error("ipc offline"));
  client.selectContext = vi.fn().mockRejectedValue(new Error("ipc offline"));
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);

  await user.click(await screen.findByRole("button", { name: /^atlas main/i }));
  await screen.findByRole("alert");
  await user.click(screen.getByRole("button", { name: /^Current repository:/ }));
  await user.click(screen.getByRole("button", { name: /^ledger Unborn/i }));
  expect(screen.getByRole("status")).toBeInTheDocument();

  // Recovery belongs to the failed first intent even though its snapshot revision is newer.
  await act(async () => {
    recovery.resolve({ ...initial, revision: 2, activeContextId: "one" });
  });
  await user.click(screen.getByRole("button", { name: /^Current repository:/ }));

  expect(screen.getByRole("button", { name: /^atlas main/i })).not.toHaveAttribute(
    "aria-current",
  );
  expect(screen.getByRole("button", { name: /^ledger Unborn/i })).not.toHaveAttribute(
    "aria-current",
  );
  expect(
    screen.queryByRole("button", { name: "Current repository: atlas" }),
  ).not.toBeInTheDocument();
});

test("unavailable selected location is not shown as clean and can be rechecked", async () => {
  const unavailable = { ...first, availability: "unavailable" as const };
  const initial: WorkspaceSnapshot = {
    revision: 2,
    restoring: false,
    persistenceError: null,
    entries: [unavailable],
    activeContextId: "one",
  };
  const client = fakeClient(initial);
  const refresh = vi.fn(async () => ({
    ...initial,
    revision: 3,
    entries: [first],
  }));
  client.refreshEntryAvailability = refresh;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await user.click(await screen.findByRole("button", { name: "Check again" }));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Check again" })).not.toBeInTheDocument(),
  );
  expect(screen.queryByText(/clean/i)).not.toBeInTheDocument();
});

test("same repository reselection removes the old tree while the host selection is pending", async () => {
  const initial: WorkspaceSnapshot = { revision: 1, entries: [first], activeContextId: "one", restoring: false, persistenceError: null };
  const selection = deferred<SelectOutcome>();
  const client = fakeClient(initial);
  let restored = false;
  client.observeSelectedContext = async (entryId) => ({
    entryId,
    observationRevision: restored ? 2 : 1,
    kind: "ready",
    files: restored ? [] : [{
      pathId: "path-1", stablePathId: "stable-1", displayPath: "old.txt", segments: ["old.txt"],
      staged: null, unstaged: "modified", untracked: false, conflict: false, unsupportedKind: null,
    }],
  });
  client.selectContext = async () => selection.promise;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  expect(await screen.findByText("old.txt")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "atlas main" }));
  expect(screen.queryByText("old.txt")).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
  restored = true;
  await act(async () => {
    selection.resolve({ kind: "selected", snapshot: { ...initial, revision: 2 } });
  });
  expect(await screen.findByRole("heading", { name: "Clean" })).toBeVisible();
});

test("periodic observation recovers a missing context even when workspace availability is stale", async () => {
  vi.useFakeTimers();
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [{ ...first, availability: "unavailable" }], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const client = fakeClient(initial);
  let available = false;
  client.observeSelectedContext = async (entryId) => available
    ? { entryId, observationRevision: 2, kind: "ready", files: [] }
    : { entryId, observationRevision: 1, kind: "unavailable", errorCode: "inaccessible" };
  renderWorkspace(<Workspace client={client} />);
  await act(async () => { await Promise.resolve(); });
  expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
  available = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByRole("heading", { name: "Clean" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
});

test("late restoration polling cannot replace a newer repository selection", async () => {
  vi.useFakeTimers();
  const initial: WorkspaceSnapshot = {
    revision: 1,
    entries: [first, second],
    activeContextId: "one",
    restoring: true,
    persistenceError: null,
  };
  const lateRestoration = deferred<WorkspaceSnapshot>();
  const client = fakeClient(initial);
  let selected = false;
  client.snapshot = vi.fn()
    .mockResolvedValueOnce(initial)
    .mockReturnValueOnce(lateRestoration.promise)
    .mockImplementation(async () => ({
      ...initial, revision: 21, activeContextId: "two", restoring: false,
    }));
  client.selectContext = async () => {
    selected = true;
    return { kind: "selected", snapshot: { ...initial, revision: 2, activeContextId: "two" } };
  };
  client.refreshEntryAvailability = async () => ({
    ...initial, revision: 3, activeContextId: selected ? "two" : "one",
  });
  renderWorkspace(<Workspace client={client} />);
  await act(async () => { await Promise.resolve(); });
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  await act(async () => { screen.getByRole("button", { name: /^ledger Unborn/i }).click(); });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();

  // A later revision does not override the intent that superseded the read.
  await act(async () => {
    lateRestoration.resolve({ ...initial, revision: 20, restoring: false });
  });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Current repository: atlas" })).not.toBeInTheDocument();
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();
});

test("replacing the desktop client discards an outstanding restoration reply and resets revision ordering", async () => {
  vi.useFakeTimers();
  const initial: WorkspaceSnapshot = {
    revision: 50,
    entries: [first],
    activeContextId: "one",
    restoring: true,
    persistenceError: null,
  };
  const lateRestoration = deferred<WorkspaceSnapshot>();
  const oldClient = fakeClient(initial);
  oldClient.snapshot = vi.fn()
    .mockResolvedValueOnce(initial)
    .mockReturnValueOnce(lateRestoration.promise);
  const replacement = fakeClient({
    revision: 1,
    entries: [second],
    activeContextId: "two",
    restoring: false,
    persistenceError: null,
  });
  const { rerender } = renderWorkspace(<Workspace client={oldClient} />);
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  rerender(<Workspace client={replacement} />);
  expect(screen.queryByRole("button", { name: "Current repository: atlas" })).not.toBeInTheDocument();
  await act(async () => { await Promise.resolve(); });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();

  await act(async () => {
    lateRestoration.resolve({ ...initial, revision: 100, restoring: false });
  });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Current repository: atlas" })).not.toBeInTheDocument();
  await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
  expect(screen.getByRole("button", { name: "Current repository: ledger" })).toBeVisible();
});

/** Opens the real menu only after the initial authoritative snapshot is interactive. */
async function openRepositoryActions(user: UserEvent, repositoryLabel: string) {
  const trigger = await screen.findByRole("button", { name: `Actions for ${repositoryLabel}` });
  await waitFor(() => expect(trigger).toBeEnabled());
  await user.click(trigger);
  await screen.findByRole("menu");
}

test("keyboard menu dismissal and rename cancellation leave selection and display names unchanged", async () => {
  const client = fakeClient({
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await screen.findByRole("button", { name: "Current repository: atlas" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Actions for atlas" })).toBeEnabled());
  await user.tab();
  await user.tab();
  await user.tab();
  await user.tab();
  const trigger = screen.getByRole("button", { name: "Actions for atlas" });
  expect(trigger).toHaveFocus();
  await user.keyboard("{Enter}");
  const rename = await screen.findByRole("menuitem", { name: "Rename" });
  await waitFor(() => expect(rename).toHaveFocus());
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("menuitem", { name: "Remove from sidebar" })).toHaveFocus();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();

  await user.keyboard("{Enter}");
  await screen.findByRole("menuitem", { name: "Rename" });
  await user.keyboard("{Enter}");
  const input = await screen.findByRole("textbox", { name: "Display name" });
  await waitFor(() => expect(input).toHaveFocus());
  expect(input).toHaveValue("atlas");
  await user.clear(input);
  await user.type(input, "   ");
  await user.keyboard("{Enter}");
  expect(input).toHaveFocus();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  await user.type(input, "discarded");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "Display name" })).not.toBeInTheDocument());
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
});

test("renaming an inactive row waits for native state and survives reopening the workspace", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const rename = deferred<RepositoryMutationOutcome>();
  let nativeSnapshot = initial;
  const client = fakeClient(initial);
  client.snapshot = async () => nativeSnapshot;
  client.renameRepository = async () => rename.promise;
  const user = userEvent.setup();
  const view = renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "ledger");
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  const input = await screen.findByRole("textbox", { name: "Display name" });
  expect(input).toHaveValue("ledger");
  expect(input).toHaveAccessibleDescription("/work/ledger.git");
  expect(screen.getByRole("navigation", { name: "Repositories" })).toContainElement(input);
  await user.clear(input);
  await user.type(input, "  Shared ledger  ");
  await user.keyboard("{Enter}");
  expect(input).toHaveAttribute("readonly");
  expect(screen.getByRole("button", { name: "Actions for ledger" })).toBeDisabled();
  nativeSnapshot = {
    ...initial, revision: 2,
    entries: [first, { ...second, repositoryLabel: "Shared ledger" }],
  };
  await act(async () => { rename.resolve({ kind: "updated", snapshot: nativeSnapshot }); });
  expect(await screen.findByRole("button", { name: /^Shared ledger Unborn/i })).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Display name" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  view.unmount();
  renderWorkspace(<Workspace client={client} />);
  expect(await screen.findByRole("button", { name: /^Shared ledger Unborn/i })).toBeVisible();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
});

test("active removal follows the native available replacement and eventually clears the context", async () => {
  const unavailable = { ...first, id: "offline", repositoryLabel: "offline", availability: "unavailable" as const };
  const client = fakeClient({
    revision: 1, entries: [first, unavailable, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "atlas");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  expect(await screen.findByRole("button", { name: "Current repository: ledger" })).toBeVisible();
  expect(await screen.findByRole("heading", { name: "No working tree" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "atlas main" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^offline main/i })).not.toHaveAttribute("aria-current");
  await openRepositoryActions(user, "ledger");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  expect(await screen.findByRole("heading", { name: "Choose a repository" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: "No working tree" })).not.toBeInTheDocument();
  await openRepositoryActions(user, "offline");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  expect(await screen.findByRole("heading", { name: "No repository open" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /^offline main/i })).not.toBeInTheDocument();
});

test("rejected rename stays editable and preserves the independent persistence warning", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first], activeContextId: "one",
    restoring: false,
    persistenceError: { code: "save_failed", message: "Workspace could not be saved." },
  };
  const client = fakeClient(initial);
  client.renameRepository = async () => ({
    kind: "rejected", code: "invalid_display_name", snapshot: initial,
  });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "atlas");
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  await user.type(screen.getByRole("textbox", { name: "Display name" }), " extended");
  await user.keyboard("{Enter}");
  await waitFor(() => expect(screen.getAllByRole("alert")).toHaveLength(2));
  expect(screen.getByRole("textbox", { name: "Display name" })).toBeEnabled();
  await user.keyboard("{Escape}");
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  expect(screen.getByRole("alert")).toBeVisible();
});

test("a lost removal reply reconciles committed native state without clearing its save warning", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const recovered: WorkspaceSnapshot = {
    ...initial, revision: 2, entries: [second], activeContextId: "two",
    persistenceError: { code: "save_failed", message: "Workspace could not be saved." },
  };
  const client = fakeClient(initial);
  client.snapshot = vi.fn().mockResolvedValueOnce(initial).mockResolvedValue(recovered);
  client.removeRepository = async () => { throw new Error("lost reply"); };
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "atlas");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  expect(await screen.findByRole("button", { name: "Current repository: ledger" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "atlas main" })).not.toBeInTheDocument();
  expect(screen.getAllByRole("alert")).toHaveLength(2);
  await user.click(screen.getByRole("button", { name: "Dismiss error" }));
  expect(screen.getByRole("alert")).toBeVisible();
});

test("a newer selection waits for removal and cannot be reverted by the old removal reply", async () => {
  const third = { ...first, id: "three", repositoryLabel: "third" };
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second, third], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const removal = deferred<RepositoryMutationOutcome>();
  const client = fakeClient(initial);
  client.removeRepository = async () => removal.promise;
  const final: WorkspaceSnapshot = {
    ...initial, revision: 3, entries: [second, third], activeContextId: "three",
  };
  client.selectContext = async () => ({ kind: "selected", snapshot: final });
  client.refreshEntryAvailability = async () => final;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "atlas");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  await user.click(screen.getByRole("button", { name: "third main" }));
  await act(async () => {
    removal.resolve({
      kind: "updated",
      snapshot: { ...initial, revision: 2, entries: [second, third], activeContextId: "two" },
    });
  });
  expect(await screen.findByRole("button", { name: "Current repository: third" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Current repository: ledger" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "atlas main" })).not.toBeInTheDocument();
});

test("missing removal reconciles the authoritative list without discarding the surviving selection", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const client = fakeClient(initial);
  client.removeRepository = async () => ({
    kind: "not_found",
    snapshot: { ...initial, revision: 2, entries: [first] },
  });
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "ledger");
  await user.click(await screen.findByRole("menuitem", { name: "Remove from sidebar" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: /^ledger Unborn/i })).not.toBeInTheDocument());
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("alert")).toHaveTextContent(/no longer exists/i);
});

test("a rename finishing after inline editor cancellation cannot undo a newer selection", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const rename = deferred<RepositoryMutationOutcome>();
  const client = fakeClient(initial);
  client.renameRepository = async () => rename.promise;
  const entries = [{ ...first, repositoryLabel: "Atlas renamed" }, second];
  const selected = { ...initial, revision: 3, entries, activeContextId: "two" };
  client.selectContext = async () => ({ kind: "selected", snapshot: selected });
  client.refreshEntryAvailability = async () => selected;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "atlas");
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  const input = screen.getByRole("textbox", { name: "Display name" });
  await user.clear(input);
  await user.type(input, "Atlas renamed");
  await user.keyboard("{Enter}");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "Display name" })).not.toBeInTheDocument());
  await waitFor(() => expect(screen.getByRole("button", { name: "atlas main" })).toHaveFocus());
  await user.click(screen.getByRole("button", { name: /^ledger Unborn/i }));
  await act(async () => {
    rename.resolve({ kind: "updated", snapshot: { ...initial, revision: 2, entries } });
  });
  expect(await screen.findByRole("button", { name: "Current repository: ledger" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Current repository:/ }));
  expect(screen.getByRole("button", { name: "Atlas renamed main" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Current repository: Atlas renamed" })).not.toBeInTheDocument();
});

test("filtering a pending inline rename does not lose the committed name or active context", async () => {
  const initial: WorkspaceSnapshot = {
    revision: 1, entries: [first, second], activeContextId: "one",
    restoring: false, persistenceError: null,
  };
  const rename = deferred<RepositoryMutationOutcome>();
  const client = fakeClient(initial);
  client.renameRepository = async () => rename.promise;
  const user = userEvent.setup();
  renderWorkspace(<Workspace client={client} />);
  await openRepositoryActions(user, "ledger");
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  const input = await screen.findByRole("textbox", { name: "Display name" });
  await waitFor(() => expect(input).toHaveFocus());
  await user.clear(input);
  await user.type(input, "Bookkeeping");
  await user.keyboard("{Enter}");
  const search = screen.getByRole("searchbox", { name: "Search repositories" });
  await user.type(search, "atlas");
  expect(screen.queryByRole("textbox", { name: "Display name" })).not.toBeInTheDocument();
  await act(async () => {
    rename.resolve({
      kind: "updated",
      snapshot: {
        ...initial, revision: 2,
        entries: [first, { ...second, repositoryLabel: "Bookkeeping" }],
      },
    });
  });
  expect(search).toHaveFocus();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear search" }));
  expect(await screen.findByRole("button", { name: /^Bookkeeping Unborn/i })).toBeVisible();
});

test("selected file content refreshes automatically even while its cached status revision is unchanged", async () => {
  vi.useFakeTimers();
  const initial: WorkspaceSnapshot = { revision: 1, entries: [first], activeContextId: "one", restoring: false, persistenceError: null };
  const client = fakeClient(initial);
  client.observeSelectedContext = async () => readyObservation([changedFile]);
  let text = "initial working content";
  client.reviewFile = async () => textReview(text);
  renderWorkspace(<Workspace client={client} />);
  await act(async () => { await Promise.resolve(); });
  await act(async () => { screen.getByRole("button", { name: "Expand all" }).click(); });
  await act(async () => { screen.getByRole("button", { name: "Review src/example.ts" }).click(); });
  expect(screen.getByText("initial working content")).toBeVisible();
  text = "edited working content";
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByText("edited working content")).toBeVisible();
  expect(screen.queryByText("initial working content")).not.toBeInTheDocument();
});
