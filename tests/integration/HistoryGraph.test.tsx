/** Exercises inline committed-file hierarchy, parent races and read-only context navigation. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HistoryPage, HistoryPageResult } from "../../src/contracts/history";
import type { CommitFilesResult } from "../../src/contracts/inspection";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { HistoryGraph } from "../../src/features/history";
import { deferred } from "../support/deferred";
import { historyClient, historyCommit, historyOids, historyPage, mergeHistory } from "../support/history";
import { installVirtualLayout } from "../support/virtualLayout";

let restoreVirtualLayout: () => void;
beforeEach(() => {
  restoreVirtualLayout = installVirtualLayout();
});
afterEach(() => {
  cleanup();
  restoreVirtualLayout();
  vi.restoreAllMocks();
});
const { merge, first, second, root, missing } = historyOids;
function committed(commitOid: string, parentOid: string | null, path: string, parents: string[] = parentOid ? [parentOid] : []): CommitFilesResult {
  return { kind: "files", commitOid, parentOid, parents,
    files: [{ id: path, displayPath: path, segments: path.split("/"), kind: "added" }] };
}
function graphClient(): RepositoryClient {
  const client = historyClient(async () => ({ kind: "page", page: historyPage(mergeHistory(), {
    refs: [{ kind: "local_branch", name: "main", commitOid: merge },
      { kind: "remote_tracking", name: "origin/main", commitOid: merge },
      { kind: "tag", name: "release", commitOid: merge }],
  }) }));
  client.commitFiles = async (_entryId, oid, parent) => committed(oid, oid === root ? null : parent ?? first, "src/nested/committed.ts", oid === merge ? [first, second] : oid === root ? [] : [first]);
  return client;
}

test("commit activation expands actual hierarchical file rows directly below it and toggles closed", async () => {
  const user = userEvent.setup();
  render(<HistoryGraph client={graphClient()} entryId="one" selectionGeneration={0} />);
  const commit = await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` });
  expect(screen.getByLabelText("Local branch main")).toBeVisible();
  expect(screen.getByLabelText("Remote-tracking branch origin/main")).toBeVisible();
  expect(screen.getByLabelText("Tag release")).toBeVisible();
  act(() => commit.focus());
  await user.keyboard("{Enter}");
  const expansion = await screen.findByRole("region", { name: `Changed files for commit ${merge}` });
  await user.click(await within(expansion).findByRole("button", { name: "Expand src" }));
  await user.click(within(expansion).getByRole("button", { name: "Expand src/nested" }));
  expect(await within(expansion).findByText("committed.ts")).toBeVisible();
  expect(within(expansion).getByLabelText("Added")).toBeVisible();
  expect(within(expansion).queryByRole("button", { name: /Review/ })).not.toBeInTheDocument();
  await user.click(within(expansion).getByRole("button", { name: "Collapse src/nested" }));
  expect(within(expansion).queryByText("committed.ts")).not.toBeInTheDocument();
  await user.click(commit);
  expect(commit).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("region", { name: `Changed files for commit ${merge}` })).not.toBeInTheDocument();
});

test("merge parent choices use actual parent-specific files and reject superseded replies", async () => {
  const user = userEvent.setup();
  const late = deferred<CommitFilesResult>();
  const client = graphClient();
  client.commitFiles = vi.fn<RepositoryClient["commitFiles"]>()
    .mockResolvedValueOnce(committed(merge, first, "first.ts", [first, second]))
    .mockReturnValueOnce(late.promise)
    .mockResolvedValueOnce(committed(merge, first, "current.ts", [first, second]));
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  expect(await screen.findByText("first.ts")).toBeVisible();
  const parent = screen.getByRole("combobox", { name: "Comparison parent" });
  await user.selectOptions(parent, second);
  expect(screen.queryByText("first.ts")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Loading changed files");
  await user.selectOptions(parent, first);
  expect(await screen.findByText("current.ts")).toBeVisible();
  await act(async () => late.resolve(committed(merge, second, "obsolete.ts", [first, second])));
  expect(screen.queryByText("obsolete.ts")).not.toBeInTheDocument();
  expect(screen.getByText("current.ts")).toBeVisible();
});

test("verified roots show empty-tree comparison and keyboard movement switches expanded commit", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  const commit = await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` });
  commit.focus();
  await user.keyboard("{Enter}{End}{Enter}");
  expect(screen.getByRole("button", { name: `Initial commit, Commit ${root}` })).toHaveFocus();
  expect(await screen.findByText("Compared with the empty tree · root commit")).toBeVisible();
  expect(screen.queryByRole("combobox", { name: "Comparison parent" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: `Changed files for commit ${merge}` })).not.toBeInTheDocument();
});

test("missing ancestry is not treated as an empty-tree root and native file errors stay honest", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  client.historyPage = async () => ({ kind: "page", page: historyPage([
    { ...historyCommit(first, [{ oid: missing, state: "unavailable" }], "Shallow commit"), root: false },
  ], { completeness: "shallow_or_missing" }) });
  client.commitFiles = async () => ({ kind: "error", code: "missing_objects", message: "Parent object unavailable." });
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Shallow commit, Commit ${first}` }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Parent object unavailable.");
  expect(screen.queryByText(/Compared with the empty tree/)).not.toBeInTheDocument();
  await user.click(screen.getByText("Unresolved ancestry (1)"));
  expect(screen.getByText(missing)).toBeVisible();
});

test("refresh and context changes clear expansion and late file errors cannot leak into a new graph", async () => {
  const user = userEvent.setup();
  const late = deferred<CommitFilesResult>();
  const client = graphClient();
  client.commitFiles = () => late.promise;
  const { rerender } = render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(screen.getByRole("button", { name: "Refresh history" }));
  expect(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` })).toHaveAttribute("aria-expanded", "false");
  await user.click(screen.getByRole("button", { name: `Merge topic, Commit ${merge}` }));
  rerender(<HistoryGraph client={client} entryId="one" selectionGeneration={1} />);
  expect(screen.queryByRole("region", { name: `Changed files for commit ${merge}` })).not.toBeInTheDocument();
  await act(async () => late.resolve({ kind: "error", code: "missing_objects", message: "Obsolete file error" }));
  expect(screen.queryByText("Obsolete file error")).not.toBeInTheDocument();
});

test("searchable branch viewing changes history only and a late old branch cannot restore its graph", async () => {
  const user = userEvent.setup();
  const stale = deferred<HistoryPageResult>();
  const client = graphClient();
  const mutate = vi.fn(client.selectContext);
  client.selectContext = mutate;
  client.selectWorktree = vi.fn(client.selectWorktree);
  client.listContexts = async () => ({ kind: "options", branches: [{ name: "main" }, { name: "topic" }, { name: "latest" }], worktrees: [] });
  client.historyPage = (_entryId, _cursor, branch) => branch === "topic" ? stale.promise : Promise.resolve({ kind: "page", page: historyPage(branch === "latest" ? [historyCommit(root, [], "Latest graph")] : mergeHistory()) });
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(screen.getByRole("button", { name: "View branch or worktree: main" }));
  const search = screen.getByRole("searchbox", { name: "Search branches and worktrees" });
  await user.type(search, "topic");
  expect(screen.queryByRole("button", { name: "View branch main" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "View branch topic" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: `Changed files for commit ${merge}` })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "View branch or worktree: topic" }));
  await user.click(await screen.findByRole("button", { name: "View branch latest" }));
  expect(await screen.findByRole("button", { name: `Latest graph, Commit ${root}` })).toBeVisible();
  await act(async () => stale.resolve({ kind: "page", page: historyPage([historyCommit(second, [], "Obsolete graph")]) }));
  expect(screen.queryByText("Obsolete graph")).not.toBeInTheDocument();
  expect(mutate).not.toHaveBeenCalled();
  expect(client.selectWorktree).not.toHaveBeenCalled();
});

test("worktree disclosure supports keyboard selection, Escape and outside dismissal", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  client.listContexts = async () => ({ kind: "options", branches: [], worktrees: [{ id: "native-worktree-token", label: "Existing topic", branch: "topic", current: false }] });
  const select = vi.fn();
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} onSelectWorktree={select} />);
  const trigger = await screen.findByRole("button", { name: "View branch or worktree: main" });
  await user.click(trigger);
  await screen.findByRole("button", { name: "Open worktree Existing topic" });
  expect(screen.getByRole("searchbox", { name: "Search branches and worktrees" })).toHaveFocus();
  await user.keyboard("{End}{Enter}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  await user.click(trigger);
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  await user.click(trigger);
  await user.click(screen.getByRole("button", { name: `Main work, Commit ${first}` }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("pagination resolves boundary lanes while retaining expansion and refresh replaces the pinned graph", async () => {
  const user = userEvent.setup();
  const pending = deferred<HistoryPageResult>();
  const read = vi.fn<RepositoryClient["historyPage"]>()
    .mockResolvedValueOnce({ kind: "page", page: historyPage([historyCommit(first, [{ oid: root, state: "outside_page" }], "Child")], { hasMore: true, cursor: "opaque-cursor", completeness: "paged" }) })
    .mockResolvedValueOnce({ kind: "page", page: historyPage([historyCommit(root, [], "Loaded root")]) })
    .mockReturnValueOnce(pending.promise);
  const client = graphClient();
  client.historyPage = read;
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Child, Commit ${first}` }));
  await user.click(screen.getByRole("button", { name: "Load more" }));
  expect(await screen.findByRole("button", { name: `Loaded root, Commit ${root}` })).toBeVisible();
  expect(screen.queryByText(/^Unresolved ancestry/)).not.toBeInTheDocument();
  expect(screen.getByRole("region", { name: `Changed files for commit ${first}` })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Refresh history" }));
  expect(screen.queryByRole("region", { name: `Changed files for commit ${first}` })).not.toBeInTheDocument();
  await act(async () => pending.resolve({ kind: "page", page: historyPage([historyCommit(root, [], "Refreshed")]) }));
  expect(screen.getByRole("button", { name: `Refreshed, Commit ${root}` })).toHaveAttribute("aria-expanded", "false");
});

test("empty comparisons and transport failures never fabricate file content", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  client.commitFiles = vi.fn<RepositoryClient["commitFiles"]>()
    .mockResolvedValueOnce({ kind: "files", commitOid: merge, parentOid: first, parents: [first, second], files: [] })
    .mockRejectedValueOnce(new Error("private payload"));
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  const commit = await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` });
  await user.click(commit);
  expect(await screen.findByRole("status")).toHaveTextContent("No changed files for this comparison.");
  await user.click(commit);
  await user.click(commit);
  expect(await screen.findByRole("alert")).toHaveTextContent("Desktop connection interrupted");
  expect(screen.queryByText("private payload")).not.toBeInTheDocument();
});

test("refresh updates the graph selector from the observed HEAD, including detached checkout", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  let head: HistoryPage["head"] = { scope: "worktree", state: "attached", branch: "main", oid: first };
  client.historyPage = async () => ({ kind: "page", page: historyPage([], { head }) });
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} workingBranch="main" />);
  await screen.findByRole("button", { name: "View branch or worktree: main" });
  head = { ...head, branch: "topic", oid: second };
  await user.click(screen.getByRole("button", { name: "Refresh history" }));
  expect(await screen.findByRole("button", { name: "View branch or worktree: topic" })).toBeVisible();
  head = { ...head, state: "detached", branch: null };
  await user.click(screen.getByRole("button", { name: "Refresh history" }));
  expect(await screen.findByRole("button", { name: "View branch or worktree: Detached" })).toBeVisible();
});

function largeHistory() {
  const commits = Array.from({ length: 1200 }, (_, index) => {
    const oid = (index + 1).toString(16).padStart(40, "0");
    const parent = (index + 2).toString(16).padStart(40, "0");
    return historyCommit(oid, index === 1199 ? [] : [{ oid: parent, state: "loaded" }], `Commit row ${index}`);
  });
  // This merge edge crosses the whole viewport even when both its endpoints are unmounted.
  commits[1].parents.push({ oid: commits[1198].oid, state: "loaded" });
  return commits;
}

function scrollToHistoryRegion(scroller: Element, index: number, count: number) {
  const height = Number.parseFloat(screen.getByRole("group", { name: "Commits" }).style.height);
  fireEvent.scroll(scroller, { target: { scrollTop: height * index / count } });
}

test("large histories bound complete rows and SVG while retaining lanes crossing unmounted endpoints", async () => {
  const commits = largeHistory();
  const client = historyClient(async () => ({ kind: "page", page: historyPage(commits) }));
  const { container } = render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await screen.findByRole("button", { name: `Commit row 0, Commit ${commits[0].oid}` });
  const scroller = container.querySelector<HTMLElement>(".history-scroll")!;
  expect(container.querySelectorAll(".history-row").length).toBeLessThan(30);
  scrollToHistoryRegion(scroller, 500, commits.length);
  await screen.findByRole("button", { name: `Commit row 500, Commit ${commits[500].oid}` });
  expect(screen.queryByRole("button", { name: `Commit row 1, Commit ${commits[1].oid}` })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: `Commit row 1198, Commit ${commits[1198].oid}` })).not.toBeInTheDocument();
  expect(container.querySelectorAll(".history-row").length).toBeLessThan(30);
  expect(container.querySelectorAll(".history-lanes path").length).toBeLessThan(90);
});

test("Home and End reach every loaded commit without selecting and keep focused rows mounted", async () => {
  const user = userEvent.setup();
  const commits = largeHistory();
  const client = historyClient(async () => ({ kind: "page", page: historyPage(commits) }));
  const { container } = render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  const firstCommit = await screen.findByRole("button", { name: `Commit row 0, Commit ${commits[0].oid}` });
  firstCommit.focus();
  await user.keyboard("{End}");
  const lastCommit = await screen.findByRole("button", { name: `Commit row 1199, Commit ${commits[1199].oid}` });
  expect(lastCommit).toHaveFocus();
  expect(lastCommit).toHaveAttribute("aria-expanded", "false");
  scrollToHistoryRegion(container.querySelector(".history-scroll")!, 500, commits.length);
  await screen.findByRole("button", { name: `Commit row 500, Commit ${commits[500].oid}` });
  expect(lastCommit).toHaveFocus();
  await user.keyboard("{Home}");
  expect(await screen.findByRole("button", { name: `Commit row 0, Commit ${commits[0].oid}` })).toHaveFocus();
  expect(screen.queryByRole("region", { name: /Changed files for commit/ })).not.toBeInTheDocument();
  expect(container.querySelectorAll(".history-row").length).toBeLessThan(30);
});

test("offscreen expansions preserve merge parent and collapsed folder state without another file read", async () => {
  const user = userEvent.setup();
  const commits = largeHistory();
  const client = historyClient(async () => ({ kind: "page", page: historyPage(commits) }));
  const read = vi.fn<RepositoryClient["commitFiles"]>(async (_entryId, oid, parent) =>
    committed(oid, parent ?? commits[1].oid, "src/nested/retained.ts", [commits[1].oid, commits[1199].oid]));
  client.commitFiles = read;
  const { container } = render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Commit row 0, Commit ${commits[0].oid}` }));
  const parent = await screen.findByRole("combobox", { name: "Comparison parent" });
  await user.selectOptions(parent, commits[1199].oid);
  await user.click(await screen.findByRole("button", { name: "Expand src" }));
  await user.click(screen.getByRole("button", { name: "Expand src/nested" }));
  expect(await screen.findByText("retained.ts")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "Collapse src/nested" }));
  const scroller = container.querySelector(".history-scroll")!;
  scrollToHistoryRegion(scroller, 500, commits.length);
  await screen.findByRole("button", { name: `Commit row 500, Commit ${commits[500].oid}` });
  expect(parent).toHaveValue(commits[1199].oid);
  fireEvent.scroll(scroller, { target: { scrollTop: 0 } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Expand src/nested" })).toBeInTheDocument());
  expect(screen.queryByText("retained.ts")).not.toBeInTheDocument();
  expect(read).toHaveBeenCalledTimes(2);
});

test("large inline committed trees window their files against the history scroller", async () => {
  const user = userEvent.setup();
  const client = graphClient();
  client.commitFiles = async () => ({
    kind: "files", commitOid: merge, parentOid: first, parents: [first, second],
    files: Array.from({ length: 1500 }, (_, index) => ({
      id: `file-${index}`, displayPath: `src/file-${String(index).padStart(4, "0")}.ts`,
      segments: ["src", `file-${String(index).padStart(4, "0")}.ts`], kind: "modified",
    })),
  });
  const { container } = render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  const tree = await screen.findByRole("list", { name: "Committed file hierarchy" });
  await user.click(await within(tree).findByRole("button", { name: "Expand src" }));
  expect(await within(tree).findByText("file-0000.ts")).toBeInTheDocument();
  expect(within(tree).getAllByRole("listitem").length).toBeLessThan(40);
  expect(tree).toHaveStyle({ height: "42028px" });
  fireEvent.scroll(container.querySelector(".history-scroll")!, { target: { scrollTop: 21096 } });
  expect(await within(tree).findByText("file-0750.ts")).toBeInTheDocument();
  expect(within(tree).queryByText("file-0001.ts")).not.toBeInTheDocument();
  expect(within(tree).getAllByRole("listitem").length).toBeLessThan(40);
  expect(container.querySelector(".history-expansion .history-continuation path")).not.toBeNull();
});

test("Tab crosses virtual gaps in logical order and exits only at the loaded history boundaries", async () => {
  const user = userEvent.setup();
  const commits = largeHistory();
  const client = historyClient(async () => ({ kind: "page", page: historyPage(commits) }));
  const { container } = render(<><HistoryGraph client={client} entryId="one" selectionGeneration={0} /><button>After history</button></>);
  const firstCommit = await screen.findByRole("button", { name: `Commit row 0, Commit ${commits[0].oid}` });
  firstCommit.focus();
  scrollToHistoryRegion(container.querySelector(".history-scroll")!, 500, commits.length);
  await screen.findByRole("button", { name: `Commit row 500, Commit ${commits[500].oid}` });
  expect(screen.queryByRole("button", { name: `Commit row 1, Commit ${commits[1].oid}` })).not.toBeInTheDocument();
  await user.tab();
  expect(await screen.findByRole("button", { name: `Commit row 1, Commit ${commits[1].oid}` })).toHaveFocus();
  await user.tab({ shift: true });
  expect(firstCommit).toHaveFocus();
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: "Refresh history" })).toHaveFocus();
  await user.tab();
  expect(firstCommit).toHaveFocus();
  await user.keyboard("{End}");
  expect(await screen.findByRole("button", { name: `Commit row 1199, Commit ${commits[1199].oid}` })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("button", { name: "After history" })).toHaveFocus();
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: `Commit row 1199, Commit ${commits[1199].oid}` })).toHaveFocus();
});

test("Tab enters expanded parent and folder controls before moving to the next commit", async () => {
  const user = userEvent.setup();
  render(<HistoryGraph client={graphClient()} entryId="one" selectionGeneration={0} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await screen.findByRole("button", { name: "Expand src" });
  await user.tab();
  expect(screen.getByRole("combobox", { name: "Comparison parent" })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("button", { name: "Expand src" })).toHaveFocus();
  await user.keyboard("{Enter}");
  await user.tab();
  expect(screen.getByRole("button", { name: "Expand src/nested" })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(await screen.findByText("committed.ts")).toBeVisible();
  await user.tab();
  expect(screen.getByRole("button", { name: `Main work, Commit ${first}` })).toHaveFocus();
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: "Collapse src/nested" })).toHaveFocus();
  expect(screen.getByRole("button", { name: `Merge topic, Commit ${merge}` })).toHaveAttribute("aria-expanded", "true");
});
