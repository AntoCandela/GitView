/** Exercises committed-file activation in the same workbench as live review, including selection races. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useState } from "react";
import type { RepositoryFileSelection } from "../../src/contracts/browsing";
import type { CommitFilesResult, CommitReviewIdentity, CommitReviewResult } from "../../src/contracts/inspection";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { Workbench as AppWorkbench } from "../../src/app/workbench/Workbench";
import type { ObservationView } from "../../src/features/changes";
import { HistoryGraph } from "../../src/features/history";
import { deferred } from "../support/deferred";
import { historyClient, historyOids, historyPage, mergeHistory } from "../support/history";
import { readyObservation, textReview } from "../support/review";
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
const { merge, first, second, root } = historyOids;
const path = "archive/version.ts";
const otherPath = "archive/other.ts";
function identity(commitOid = merge, parentOid: string | null = first, fileId = `${parentOid}:version`, entryId = "one"): CommitReviewIdentity {
  return { entryId, commitOid, parentOid, fileId, displayPath: path, contextLabel: "Pinned repository",
    fromAbsent: false, toAbsent: false };
}
function historicalText(text: string, fields: CommitReviewIdentity = identity()): CommitReviewResult {
  return { ...fields, kind: "text", fromContent: "parent source\n", toContent: `${text}\n`,
    hunks: [{ oldStart: 1, oldCount: 1, newStart: 1, newCount: 1,
    lines: [{ kind: "removal", text: "parent source" }, { kind: "addition", text }] }] };
}
function previewClient(): RepositoryClient {
  const client = historyClient(async (entryId) => ({ kind: "page", page: historyPage(mergeHistory(), { entryId }) }));
  client.commitFiles = async (_entryId, oid, parent) => {
    const parentOid = oid === root ? null : parent ?? first;
    return { kind: "files", commitOid: oid, parentOid, parents: oid === root ? [] : [first, second], files: [
      { id: `${parentOid}:version`, displayPath: path, segments: path.split("/"), kind: "modified" },
      { id: `${parentOid}:other`, displayPath: otherPath, segments: otherPath.split("/"), kind: "modified" },
    ] };
  };
  client.reviewFile = async (entryId, _revision, pathId, category) => textReview("working-only source", {
    entryId, pathId, category, from: category === "staged" ? "HEAD" : category === "untracked" ? "absent" : "index",
    to: category === "staged" ? "index" : "working_files",
  });
  client.reviewCommitFile = async (entryId, oid, parent, fileId) => historicalText("committed-only source", identity(oid, parent, fileId, entryId));
  return client;
}
function Workbench({ client, entryId = "one", generation = 0, observation = readyObservation() }: {
  client: RepositoryClient; entryId?: string; generation?: number; observation?: ObservationView;
}) {
  return <AppWorkbench client={client} entryId={entryId} selectionGeneration={generation} contextLabel="Sample repository" observation={observation}>
    {(comparison) => <HistoryGraph client={client} entryId={entryId} selectionGeneration={generation} comparison={comparison} />}
  </AppWorkbench>;
}

test("clicked committed files use pinned content in the shared preview and sidebar clicks restore live selection", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const liveRead = vi.fn(client.reviewFile);
  client.reviewFile = liveRead;
  render(<Workbench client={client} />);
  const sidebar = screen.getByRole("list", { name: "Changed file hierarchy" });
  await user.click(within(sidebar).getByRole("button", { name: "Expand src" }));
  const liveFile = within(sidebar).getByRole("button", { name: "Review src/example.ts" });
  await user.click(liveFile);
  expect(await screen.findByText("working-only source")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  const historicalFile = await screen.findByRole("button", { name: `Review ${path}` });
  await user.click(historicalFile);
  expect(await screen.findByText("committed-only source")).toBeVisible();
  expect(screen.queryByText("working-only source")).not.toBeInTheDocument();
  expect(screen.getByRole("heading", { name: path })).toBeVisible();
  expect(historicalFile).toHaveAttribute("aria-pressed", "true");
  expect(liveFile).toHaveAttribute("aria-pressed", "false");
  expect(liveRead).toHaveBeenCalledTimes(1);
  await user.click(liveFile);
  expect(await screen.findByText("working-only source")).toBeVisible();
  expect(screen.queryByText("committed-only source")).not.toBeInTheDocument();
  expect(historicalFile).toHaveAttribute("aria-pressed", "false");
  expect(liveFile).toHaveAttribute("aria-pressed", "true");
});

test("late committed-file and parent completions cannot replace newer history or live choices", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const oldest = deferred<CommitReviewResult>();
  const replaced = deferred<CommitReviewResult>();
  const nextParent = deferred<CommitReviewResult>();
  client.reviewCommitFile = vi.fn<RepositoryClient["reviewCommitFile"]>()
    .mockReturnValueOnce(oldest.promise).mockReturnValueOnce(replaced.promise).mockReturnValueOnce(nextParent.promise);
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  await user.click(screen.getByRole("button", { name: `Review ${otherPath}` }));
  await act(async () => oldest.resolve(historicalText("older file")));
  expect(screen.queryByText("older file")).not.toBeInTheDocument();
  await user.selectOptions(screen.getByRole("combobox", { name: "Comparison parent" }), second);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  await act(async () => nextParent.resolve(historicalText("second parent source", identity(merge, second))));
  expect(screen.getByText("second parent source")).toBeVisible();
  await act(async () => replaced.resolve(historicalText("abandoned first parent", identity(merge, first, `${first}:other`))));
  expect(screen.queryByText("abandoned first parent")).not.toBeInTheDocument();
  expect(screen.getByText("second parent source")).toBeVisible();
});

test("switching to live while historical content is pending never displays that completion under a live category", async () => {
  const user = userEvent.setup();
  const pending = deferred<CommitReviewResult>();
  const client = previewClient();
  client.reviewCommitFile = () => pending.promise;
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  await user.click(screen.getByRole("button", { name: "Expand src" }));
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("working-only source")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Staged comparison" }));
  await act(async () => pending.resolve(historicalText("abandoned historical source")));
  expect(screen.queryByText("abandoned historical source")).not.toBeInTheDocument();
});

test.each(["entry", "generation", "client"] as const)("%s changes clear historical selection and isolate old completions even when returning to the same context", async (change) => {
  const user = userEvent.setup();
  const abandoned = deferred<CommitReviewResult>();
  const current = deferred<CommitReviewResult>();
  const client = previewClient();
  client.reviewCommitFile = vi.fn<RepositoryClient["reviewCommitFile"]>()
    .mockReturnValueOnce(abandoned.promise).mockReturnValueOnce(current.promise);
  const { rerender } = render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  rerender(<Workbench client={change === "client" ? previewClient() : client} entryId={change === "entry" ? "two" : "one"}
    generation={change === "generation" ? 1 : 0} />);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
  rerender(<Workbench client={client} />);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  await act(async () => current.resolve(historicalText("current scope")));
  await act(async () => abandoned.resolve(historicalText("abandoned scope")));
  expect(screen.getByText("current scope")).toBeVisible();
  expect(screen.queryByText("abandoned scope")).not.toBeInTheDocument();
});

test.each(["bare", "unavailable"] as const)("historical root files remain reviewable when working status is %s", async (kind) => {
  const user = userEvent.setup();
  const client = previewClient();
  client.commitFiles = async () => ({ kind: "files", commitOid: root, parentOid: null, parents: [], files: [
    { id: "root-authority", displayPath: path, segments: path.split("/"), kind: "added" },
  ] });
  client.reviewCommitFile = async () => ({ ...identity(root, null, "root-authority"), fromAbsent: true, kind: "text",
    fromContent: "", toContent: "root source\n",
    hunks: [{ oldStart: 0, oldCount: 0, newStart: 1, newCount: 1, lines: [{ kind: "addition", text: "root source" }] }] });
  render(<Workbench client={client} observation={kind === "bare" ? { kind: "bare", entryId: "one", observationRevision: 1 }
    : { kind: "unavailable", entryId: "one", observationRevision: 1, errorCode: "invalid_status" }} />);
  await user.click(await screen.findByRole("button", { name: `Initial commit, Commit ${root}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText("root source")).toBeVisible();
  expect(within(screen.getByRole("region", { name: "Old source hunks" })).queryByText("root source")).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Comparison category" })).not.toBeInTheDocument();
});

test("deleted historical files keep removal content and label the selected commit endpoint as absent", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  client.commitFiles = async () => ({ kind: "files", commitOid: merge, parentOid: first, parents: [first], files: [
    { id: "deleted-authority", displayPath: path, segments: path.split("/"), kind: "deleted" },
  ] });
  client.reviewCommitFile = async () => ({ ...identity(merge, first, "deleted-authority"), toAbsent: true, kind: "text",
    fromContent: "deleted source\n", toContent: "",
    hunks: [{ oldStart: 1, oldCount: 1, newStart: 0, newCount: 0, lines: [{ kind: "removal", text: "deleted source" }] }] });
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText("deleted source")).toBeVisible();
  expect(within(screen.getByRole("region", { name: "New source hunks" })).queryByText("deleted source")).not.toBeInTheDocument();
});

test.each(["unsupported", "unavailable", "stale_selection", "mismatch"] as const)("historical %s outcomes never fall back to working bytes", async (kind) => {
  const user = userEvent.setup();
  const client = previewClient();
  const liveRead = vi.fn(client.reviewFile);
  client.reviewFile = liveRead;
  client.reviewCommitFile = async () => kind === "unsupported" ? { kind, reason: "binary", identity: identity() }
    : kind === "unavailable" ? { kind, code: "inaccessible", identity: identity() }
    : kind === "stale_selection" ? { kind } : historicalText("wrong authority", identity(merge, second));
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByRole("heading", { name: kind === "unsupported" ? "Preview unsupported"
    : kind === "stale_selection" ? "Comparison selection expired" : "Comparison unavailable" })).toBeVisible();
  expect(screen.queryByText("wrong authority")).not.toBeInTheDocument();
  expect(screen.queryByText("working-only source")).not.toBeInTheDocument();
  expect(liveRead).not.toHaveBeenCalled();
});

test("an evicted visible file authority is renewed once against the same pinned listing before reading", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const listed = client.commitFiles;
  let listingCount = 0;
  client.commitFiles = async (...args) => {
    const result = await listed(...args);
    listingCount++;
    return result.kind === "files" && listingCount > 1 ? { ...result,
      files: [{ ...result.files[0], id: "renewed-authority" }] } : result;
  };
  client.reviewCommitFile = async (entryId, oid, parent, fileId) => fileId === "renewed-authority"
    ? historicalText("renewed pinned source", identity(oid, parent, fileId, entryId)) : { kind: "stale_selection" };
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  const leaf = await screen.findByRole("button", { name: `Review ${path}` });
  await user.click(leaf);
  expect(await screen.findByText("renewed pinned source")).toBeVisible();
  expect(leaf).toHaveAttribute("aria-pressed", "true");
});

test.each(["commit", "parent", "path", "segments"] as const)("authority renewal rejects a %s mismatch instead of reading an unrelated file", async (mismatch) => {
  const user = userEvent.setup();
  const client = previewClient();
  const listed = client.commitFiles;
  let listingCount = 0;
  client.commitFiles = async (...args) => {
    const result = await listed(...args);
    listingCount++;
    if (result.kind !== "files" || listingCount === 1) return result;
    const file = { ...result.files[0], id: "renewed-authority",
      displayPath: mismatch === "path" ? otherPath : path,
      segments: mismatch === "segments" ? ["archive", "unrelated.ts"] : path.split("/") };
    return { ...result, commitOid: mismatch === "commit" ? root : merge,
      parentOid: mismatch === "parent" ? second : first,
      files: [file] };
  };
  let readCount = 0;
  client.reviewCommitFile = async (entryId, oid, parent, fileId) => ++readCount === 1 ? { kind: "stale_selection" }
    : historicalText("unrelated reopened source", identity(oid, parent, fileId, entryId));
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByRole("heading", { name: "Comparison selection expired" })).toBeVisible();
  expect(screen.queryByText("unrelated reopened source")).not.toBeInTheDocument();
  expect(screen.queryByText("working-only source")).not.toBeInTheDocument();
});

test("changing the raw parent during authority renewal discards the renewed old-parent read", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const renewed = deferred<CommitFilesResult>();
  const listed = client.commitFiles;
  let listingCount = 0;
  client.commitFiles = (...args) => ++listingCount === 2 ? renewed.promise : listed(...args);
  client.reviewCommitFile = async (entryId, oid, parent, fileId) => parent === first
    ? fileId === "renewed-authority" ? historicalText("abandoned renewed source", identity(oid, parent, fileId, entryId))
      : { kind: "stale_selection" }
    : historicalText("current raw-parent source", identity(oid, parent, fileId, entryId));
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  await user.selectOptions(screen.getByRole("combobox", { name: "Comparison parent" }), second);
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText("current raw-parent source")).toBeVisible();
  await act(async () => renewed.resolve({ kind: "files", commitOid: merge, parentOid: first, parents: [first, second],
    files: [{ id: "renewed-authority", displayPath: path, segments: path.split("/"), kind: "modified" }] }));
  expect(screen.queryByText("abandoned renewed source")).not.toBeInTheDocument();
  expect(screen.getByText("current raw-parent source")).toBeVisible();
});

test("changing or collapsing the expanded commit invalidates its pending historical preview", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const abandoned = deferred<CommitReviewResult>();
  client.reviewCommitFile = async (entryId, oid, parent, fileId) => oid === merge ? abandoned.promise
    : { ...identity(oid, parent, fileId, entryId), fromAbsent: true, kind: "text",
      fromContent: "", toContent: "root selected source\n",
      hunks: [{ oldStart: 0, oldCount: 0, newStart: 1, newCount: 1, lines: [{ kind: "addition", text: "root selected source" }] }] };
  render(<Workbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  const rootCommit = screen.getByRole("button", { name: `Initial commit, Commit ${root}` });
  await user.click(rootCommit);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText("root selected source")).toBeVisible();
  await act(async () => abandoned.resolve(historicalText("abandoned commit source")));
  expect(screen.queryByText("abandoned commit source")).not.toBeInTheDocument();
  expect(screen.getByText("root selected source")).toBeVisible();
  await user.click(rootCommit);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
});

test("history invalidation leaves browsed working bytes visible until committed-file activation dismisses them", async () => {
  const user = userEvent.setup();
  const client = previewClient();
  const file: RepositoryFileSelection = { id: "unchanged", listingId: "working-listing",
    displayPath: "unchanged.txt", segments: ["unchanged.txt"] };
  client.reviewRepositoryFile = async () => ({ kind: "text", entryId: "one", listingId: file.listingId,
    fileId: file.id, displayPath: file.displayPath, content: "browsed working bytes\n" });
  function BrowsingWorkbench() {
    const [browse, setBrowse] = useState<RepositoryFileSelection | null>(file);
    return <AppWorkbench client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository"
      observation={readyObservation()} repositoryFile={browse} onRepositoryFileDismiss={() => setBrowse(null)}>
      {(comparison) => <HistoryGraph client={client} entryId="one" selectionGeneration={0} comparison={comparison} />}
    </AppWorkbench>;
  }
  render(<BrowsingWorkbench />);
  expect(await screen.findByText("browsed working bytes")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: `Merge topic, Commit ${merge}` }));
  expect(screen.getByText("browsed working bytes")).toBeVisible();
  await user.click(await screen.findByRole("button", { name: "Expand archive" }));
  await user.click(await screen.findByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText("committed-only source")).toBeVisible();
  expect(screen.queryByText("browsed working bytes")).not.toBeInTheDocument();
});
