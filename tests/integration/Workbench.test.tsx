/** Exercises the actual file-sidebar, ancestry graph and compact repository controls as one workbench. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryMutationOutcome, SelectOutcome, WorkspaceSnapshot } from "../../src/contracts/repositories";
import { Workspace } from "../../src/app/Workspace";
import { deferred } from "../support/deferred";
import { historyOids, historyPage, mergeHistory } from "../support/history";
import { reviewClient, textReview } from "../support/review";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(360);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(300);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

const snapshot: WorkspaceSnapshot = {
  revision: 1, restoring: false, persistenceError: null, activeContextId: "one",
  entries: [{ id: "one", kind: "working_tree", repositoryLabel: "atlas", locationLabel: "/fixture/atlas", head: { kind: "branch", name: "main" }, availability: "available" }],
};
function workbenchClient() {
  const client = reviewClient(async (entryId, _revision, pathId, category) => textReview(category === "staged" ? "indexed content" : "working content", {
    entryId, pathId, category, from: category === "staged" ? "HEAD" : "index", to: category === "staged" ? "index" : "working_files",
  }));
  client.snapshot = async () => snapshot;
  client.historyPage = async () => ({ kind: "page", page: historyPage(mergeHistory(), { refs: [{ kind: "local_branch", name: "main", commitOid: historyOids.merge }] }) });
  client.commitFiles = async (_entryId, commitOid, parentOid) => ({
    kind: "files", commitOid, parentOid: parentOid ?? historyOids.first, parents: [historyOids.first, historyOids.second],
    files: [{ id: "commit-file", displayPath: "src/committed.ts", segments: ["src", "committed.ts"], kind: "added" }],
  });
  return client;
}

test("filename selection opens the lower comparison without replacing ancestry or expanded commit files", async () => {
  const user = userEvent.setup();
  render(<Workspace client={workbenchClient()} />);
  const merge = await screen.findByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` });
  await user.click(merge);
  await user.click(await within(screen.getByRole("list", { name: "Committed file hierarchy" })).findByRole("button", { name: "Expand src" }));
  await user.click(within(screen.getByRole("complementary", { name: "Files" })).getByRole("button", { name: "Expand all" }));
  expect(await screen.findByText("committed.ts")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("working content")).toBeVisible();
  expect(merge).toBeVisible();
  expect(merge).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("committed.ts")).toBeVisible();
  const files = screen.getByRole("complementary", { name: "Files" });
  expect(within(files).getByRole("button", { name: "Review src/example.ts" })).toHaveAttribute("aria-pressed", "true");
  expect(within(files).queryByRole("button", { name: "Staged comparison" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Staged comparison" }));
  expect(await screen.findByText("indexed content")).toBeVisible();
  expect(screen.queryByText("working content")).not.toBeInTheDocument();
  expect(merge).toBeVisible();
});

test("repository disclosure exposes actual rows and closes when graph navigation resumes", async () => {
  const user = userEvent.setup();
  render(<Workspace client={workbenchClient()} />);
  const selector = await screen.findByRole("button", { name: "Current repository: atlas" });
  expect(selector).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("navigation", { name: "Repositories" })).not.toBeInTheDocument();
  await user.click(selector);
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "Open repository" })).toBeEnabled();
  await user.keyboard("{Escape}");
  expect(selector).toHaveAttribute("aria-expanded", "false");
  expect(selector).toHaveFocus();
  await user.click(selector);
  await user.click(screen.getByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` }));
  expect(selector).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("navigation", { name: "Repositories" })).not.toBeInTheDocument();
  await user.click(await within(screen.getByRole("list", { name: "Committed file hierarchy" })).findByRole("button", { name: "Expand src" }));
  expect(await screen.findByText("committed.ts")).toBeVisible();
});

test("same-entry reselection immediately clears both graph and comparison before confirmation", async () => {
  const user = userEvent.setup();
  const selection = deferred<SelectOutcome>();
  const client = workbenchClient();
  client.selectContext = () => selection.promise;
  render(<Workspace client={client} />);
  expect(await screen.findByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("working content")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Current repository: atlas" }));
  await user.click(screen.getByRole("button", { name: "atlas main" }));
  expect(screen.queryByRole("navigation", { name: "Repositories" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Commit ancestry" })).not.toBeInTheDocument();
  expect(screen.queryByText("working content")).not.toBeInTheDocument();
  await act(async () => selection.resolve({ kind: "selected", snapshot: { ...snapshot, revision: 2 } }));
  expect(await screen.findByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` })).toBeVisible();
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
});

test("choosing an existing unadmitted worktree clears old views until its native snapshot confirms navigation", async () => {
  const user = userEvent.setup();
  const switchResult = deferred<RepositoryMutationOutcome>();
  const client = workbenchClient();
  client.listContexts = async () => ({ kind: "options", branches: [{ name: "main" }], worktrees: [
    { id: "opaque-worktree", label: "Topic worktree", branch: "topic", current: false },
  ] });
  let confirmed = snapshot;
  client.selectWorktree = () => switchResult.promise;
  client.refreshEntryAvailability = async () => confirmed;
  client.historyPage = async (entryId) => ({ kind: "page", page: historyPage(mergeHistory(), {
    entryId, head: { scope: "worktree", state: "attached", branch: entryId === "topic-entry" ? "topic" : "main", oid: historyOids.merge },
  }) });
  render(<Workspace client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` }));
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("working content")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "View branch or worktree: main" }));
  await user.click(await screen.findByRole("button", { name: "Open worktree Topic worktree" }));
  expect(screen.queryByRole("region", { name: "Commit ancestry" })).not.toBeInTheDocument();
  expect(screen.queryByText("working content")).not.toBeInTheDocument();
  confirmed = {
    ...snapshot, revision: 2, activeContextId: "topic-entry",
    entries: [...snapshot.entries, { ...snapshot.entries[0], id: "topic-entry", repositoryLabel: "atlas topic", head: { kind: "branch", name: "topic" } }],
  };
  await act(async () => switchResult.resolve({ kind: "updated", snapshot: confirmed }));
  expect(await screen.findByRole("button", { name: "View branch or worktree: topic" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Current repository: atlas topic" })).toBeVisible();
  expect(screen.queryByRole("region", { name: `Changed files for commit ${historyOids.merge}` })).not.toBeInTheDocument();
  expect(screen.queryByText("working content")).not.toBeInTheDocument();
});

test("viewing a branch replaces ancestry but leaves the actual working-file selection and content untouched", async () => {
  const user = userEvent.setup();
  const client = workbenchClient();
  client.listContexts = async () => ({ kind: "options", branches: [{ name: "main" }, { name: "topic" }], worktrees: [] });
  client.historyPage = async (entryId, _cursor, branch) => ({ kind: "page", page: historyPage(
    branch === "topic" ? [{ oid: historyOids.root, subject: "Topic-only history", parents: [], root: true }] : mergeHistory(),
    { entryId },
  ) });
  render(<Workspace client={client} />);
  await user.click(await screen.findByRole("button", { name: "Expand src" }));
  await user.click(await screen.findByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("working content")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "View branch or worktree: main" }));
  await user.click(await screen.findByRole("button", { name: "View branch topic" }));
  expect(await screen.findByRole("button", { name: `Topic-only history, Commit ${historyOids.root}` })).toBeVisible();
  expect(screen.getByRole("button", { name: "Review src/example.ts" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("working content")).toBeVisible();
  expect(screen.getByRole("button", { name: "Current repository: atlas" })).toBeVisible();
  expect(screen.getByRole("button", { name: "View branch or worktree: topic" }).getAttribute("aria-description")).toMatch(/topic.*main/);
});
