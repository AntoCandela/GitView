/** Exercises repository file browsing, selection races and management with the sidebar open. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryFilesResult } from "../../src/contracts/browsing";
import type { WorkspaceSnapshot } from "../../src/contracts/repositories";
import { Workspace } from "../../src/app/Workspace";
import { deferred } from "../support/deferred";
import { changedFile, readyObservation, reviewClient, textReview } from "../support/review";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(360);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(280);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function sidebarClient() {
  const client = reviewClient(async () => textReview("pending working content"));
  let snapshot: WorkspaceSnapshot = {
    revision: 1, restoring: false, persistenceError: null, activeContextId: "one",
    entries: [
      { id: "one", kind: "working_tree", repositoryLabel: "atlas", locationLabel: "/fixture/atlas", head: { kind: "branch", name: "main" }, availability: "available" },
      { id: "two", kind: "working_tree", repositoryLabel: "ledger", locationLabel: "/fixture/ledger", head: { kind: "branch", name: "topic" }, availability: "available" },
    ],
  };
  client.snapshot = async () => snapshot;
  client.refreshEntryAvailability = async () => snapshot;
  client.selectContext = async (entryId) => {
    snapshot = { ...snapshot, revision: snapshot.revision + 1, activeContextId: entryId };
    return { kind: "selected", snapshot };
  };
  client.renameRepository = async (entryId, displayName) => {
    snapshot = { ...snapshot, revision: snapshot.revision + 1,
      entries: snapshot.entries.map((entry) => entry.id === entryId ? { ...entry, repositoryLabel: displayName } : entry) };
    return { kind: "updated", snapshot };
  };
  client.removeRepository = async (entryId) => {
    snapshot = { ...snapshot, revision: snapshot.revision + 1, entries: snapshot.entries.filter((entry) => entry.id !== entryId) };
    return { kind: "updated", snapshot };
  };
  client.openChosenRepository = async () => {
    snapshot = { ...snapshot, revision: snapshot.revision + 1,
      entries: [...snapshot.entries, { ...snapshot.entries[0], id: "three", repositoryLabel: "new-repo" }] };
    return { kind: "opened", entryId: "three", snapshot };
  };
  client.observeSelectedContext = async (entryId) => readyObservation([changedFile], 1, entryId);
  client.listRepositoryFiles = async (entryId, request) => {
    const page = { kind: "files" as const, entryId, listingId: `${entryId}-listing`, directoryId: request.directoryId, cursor: null };
    if (request.directoryId === null) return { ...page, files: [], directories: [
      { id: "docs", displayPath: "docs", segments: ["docs"] },
      { id: "src", displayPath: "src", segments: ["src"] },
    ] };
    if (request.directoryId === "docs") return { ...page, files: [], directories: [
      { id: "deep", displayPath: "docs/deep", segments: ["docs", "deep"] },
    ] };
    if (request.directoryId === "src") return { ...page, directories: [], files: [
      { id: "pending", displayPath: "src/example.ts", segments: ["src", "example.ts"] },
    ] };
    return { ...page, directories: [], files: [
      { id: "unchanged", displayPath: "docs/deep/guide.md", segments: ["docs", "deep", "guide.md"] },
    ] };
  };
  client.reviewRepositoryFile = async (entryId, listingId, fileId) => ({
    kind: "text", entryId, listingId, fileId, displayPath: "docs/deep/guide.md", content: "unchanged repository contents\n",
  });
  return client;
}

async function openFiles(user: UserEvent) {
  await user.click(await screen.findByRole("button", { name: "Expand sidebar" }));
  const sidebar = screen.getByRole("complementary", { name: "Workspace sidebar" });
  return sidebar;
}

async function expandFiles(user: UserEvent, sidebar: HTMLElement) {
  await within(sidebar).findByRole("button", { name: "Expand docs" });
  await user.click(within(sidebar).getByRole("button", { name: "Expand all" }));
  await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" });
}

test("reset reopens the files sidebar without clearing its selected preview", async () => {
  const user = userEvent.setup();
  render(<Workspace client={sidebarClient()} />);
  const sidebar = await openFiles(user);
  await expandFiles(user, sidebar);
  await user.click(within(sidebar).getByRole("button", { name: "Review docs/deep/guide.md" }));
  const source = await screen.findByText("unchanged repository contents");
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  expect(sidebar).not.toBeVisible();
  await user.click(screen.getByRole("button", { name: "Reset layout" }));
  expect(sidebar).toBeVisible();
  expect(screen.getByRole("button", { name: "Collapse sidebar" })).toHaveAttribute("aria-expanded", "true");
  expect(within(sidebar).getByRole("button", { name: "Review docs/deep/guide.md" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("unchanged repository contents")).toBe(source);
});

test("unchanged files open in shared preview and sidebar collapse preserves the selected file", async () => {
  const user = userEvent.setup();
  render(<Workspace client={sidebarClient()} />);
  const sidebar = await openFiles(user);
  await expandFiles(user, sidebar);
  await user.click(await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" }));
  expect(await screen.findByText("unchanged repository contents")).toBeVisible();
  const pending = within(sidebar).getByRole("button", { name: "Review src/example.ts" });
  expect(within(pending).getByLabelText("Staged Modified, Unstaged Modified")).toHaveTextContent("MM");
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  expect(sidebar).not.toBeVisible();
  expect(screen.getByText("unchanged repository contents")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
  expect(await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" })).toHaveAttribute("aria-pressed", "true");
  const changed = screen.getByRole("complementary", { name: "Files" });
  await user.click(within(changed).getByRole("button", { name: "Expand all" }));
  await user.click(within(changed).getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("pending working content")).toBeVisible();
  expect(screen.queryByText("unchanged repository contents")).not.toBeInTheDocument();
});

test("folder controls and sidebar collapse preserve nested directory expansion", async () => {
  const user = userEvent.setup();
  render(<Workspace client={sidebarClient()} />);
  const sidebar = await openFiles(user);
  expect(await within(sidebar).findByRole("button", { name: "Expand docs" })).toHaveAttribute("aria-expanded", "false");
  expect(within(sidebar).queryByRole("button", { name: "Expand docs/deep" })).not.toBeInTheDocument();
  await expandFiles(user, sidebar);
  await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" });
  await user.click(within(sidebar).getByRole("button", { name: "Collapse all" }));
  expect(within(sidebar).queryByRole("button", { name: "Review docs/deep/guide.md" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
  expect(within(sidebar).getByRole("button", { name: "Expand docs" })).toHaveAttribute("aria-expanded", "false");
  await user.click(within(sidebar).getByRole("button", { name: "Expand all" }));
  expect(within(sidebar).getByRole("button", { name: "Review docs/deep/guide.md" })).toBeVisible();
});

test("late repository listings cannot replace the newly selected repository tree", async () => {
  const user = userEvent.setup();
  const delayed = deferred<RepositoryFilesResult>();
  const client = sidebarClient();
  const listing = client.listRepositoryFiles;
  client.listRepositoryFiles = (entryId, request) => entryId === "one" ? delayed.promise : listing(entryId, request);
  render(<Workspace client={client} />);
  const sidebar = await openFiles(user);
  await user.click(await screen.findByRole("button", { name: "Current repository: atlas" }));
  await user.click(await screen.findByRole("button", { name: "ledger topic" }));
  await screen.findByRole("button", { name: "Current repository: ledger" });
  await expandFiles(user, sidebar);
  expect(await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" })).toBeVisible();
  await act(async () => delayed.resolve({ kind: "files", entryId: "one", listingId: "old",
    directoryId: null, cursor: null, directories: [], files: [
    { id: "old", displayPath: "old-only.txt", segments: ["old-only.txt"] },
  ] }));
  expect(within(sidebar).queryByText("old-only.txt")).not.toBeInTheDocument();
});

test("repository disclosure rename preserves focus and the file preview while the sidebar is open", async () => {
  const user = userEvent.setup();
  render(<Workspace client={sidebarClient()} />);
  const sidebar = await openFiles(user);
  await expandFiles(user, sidebar);
  await user.click(await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" }));
  await screen.findByText("unchanged repository contents");
  await user.click(screen.getByRole("button", { name: "Current repository: atlas" }));
  const trigger = await screen.findByRole("button", { name: "Actions for atlas" });
  await waitFor(() => expect(trigger).toBeEnabled());
  await user.click(trigger);
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  const input = await screen.findByRole("textbox", { name: "Display name" });
  await waitFor(() => expect(input).toHaveFocus());
  await user.clear(input);
  await user.type(input, "Atlas renamed{Enter}");
  expect(await screen.findByRole("button", { name: "Current repository: Atlas renamed" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Atlas renamed main" })).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "Atlas renamed main" })).toHaveAccessibleDescription("/fixture/atlas");
  expect(sidebar).toBeVisible();
  expect(screen.getByText("unchanged repository contents")).toBeVisible();
});

test("failed repository listing reports unavailable status and refresh restores real leaves", async () => {
  const user = userEvent.setup();
  const client = sidebarClient();
  const listing = client.listRepositoryFiles;
  client.listRepositoryFiles = async () => ({ kind: "unavailable", code: "inaccessible", message: "Repository files unavailable." });
  render(<Workspace client={client} />);
  const sidebar = await openFiles(user);
  expect(await within(sidebar).findByRole("alert")).toHaveTextContent("Repository files unavailable.");
  expect(within(sidebar).queryByText("0 files")).not.toBeInTheDocument();
  client.listRepositoryFiles = listing;
  await user.click(within(sidebar).getByRole("button", { name: "Refresh files" }));
  await expandFiles(user, sidebar);
  expect(await within(sidebar).findByRole("button", { name: "Review docs/deep/guide.md" })).toBeVisible();
  expect(within(sidebar).queryByRole("alert")).not.toBeInTheDocument();
});
