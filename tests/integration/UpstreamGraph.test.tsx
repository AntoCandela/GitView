/** Exercises independent upstream summaries through the shared committed comparison UI. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { HistoryGraph } from "../../src/features/history";
import { historyClient, historyPage, mergeHistory, historyOids } from "../support/history";
import { installVirtualLayout } from "../support/virtualLayout";
import { deferred } from "../support/deferred";
import type { CommitFilesResult } from "../../src/contracts/inspection";

let restore: () => void;
beforeEach(() => { restore = installVirtualLayout(); });
afterEach(() => { cleanup(); restore(); vi.restoreAllMocks(); });
const incoming = { token: "incoming-token", baseOid: historyOids.root, tipOid: historyOids.second };
const outgoing = { token: "outgoing-token", baseOid: historyOids.root, tipOid: historyOids.merge };
function fixture(ahead = 2, behind = 3) {
  const page = historyPage(mergeHistory(), { upstream: {
    state: "ready", freshness: "fresh", branch: "main", upstream: "team/main", ahead, behind,
    incoming: behind ? incoming : null, outgoing: ahead ? outgoing : null,
  } });
  const client = historyClient(async () => ({ kind: "page", page }));
  client.upstreamFiles = vi.fn(async (_entryId, token) => {
    const range = token === incoming.token ? incoming : outgoing;
    const name = token === incoming.token ? "incoming.txt" : "outgoing.txt";
    return { kind: "files", commitOid: range.tipOid, parentOid: range.baseOid, parents: [range.baseOid],
      files: [{ id: name, displayPath: name, segments: [name], kind: "modified" }] } as CommitFilesResult;
  });
  return { client, page };
}

test("diverged summaries independently open aggregate files and select pinned comparisons", async () => {
  const user = userEvent.setup();
  const { client } = fixture();
  const onSelect = vi.fn();
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0}
    comparison={{ selection: null, onSelect, captureAutoSelection: () => onSelect, onInvalidate: vi.fn() }} />);
  const incomingNode = await screen.findByRole("button", { name: "Incoming Changes · 3 commits" });
  const outgoingNode = screen.getByRole("button", { name: "Outgoing Changes · 2 commits" });
  await user.click(incomingNode);
  expect(await screen.findByText("incoming.txt")).toBeVisible();
  expect(onSelect).toHaveBeenLastCalledWith(expect.objectContaining({ commitOid: incoming.tipOid,
    parentOid: incoming.baseOid, upstream: { token: incoming.token, direction: "incoming" } }));
  await user.click(outgoingNode);
  expect(await screen.findByText("outgoing.txt")).toBeVisible();
  expect(screen.queryByText("incoming.txt")).not.toBeInTheDocument();
  expect(onSelect).toHaveBeenLastCalledWith(expect.objectContaining({ commitOid: outgoing.tipOid, parentOid: outgoing.baseOid }));
  expect(client.upstreamFiles).toHaveBeenNthCalledWith(1, "one", incoming.token);
  expect(client.upstreamFiles).toHaveBeenNthCalledWith(2, "one", outgoing.token);
});

test.each([[0, 0], [0, 3], [2, 0]])("zero sides are omitted for ahead %i and behind %i", async (ahead, behind) => {
  const { client } = fixture(ahead, behind);
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  await screen.findByRole("button", { name: /Merge topic, Commit/ });
  expect(screen.queryByRole("button", { name: /Incoming Changes ·/ }) !== null).toBe(behind > 0);
  expect(screen.queryByRole("button", { name: /Outgoing Changes ·/ }) !== null).toBe(ahead > 0);
  if (!ahead && !behind) expect(screen.getByText("Up to date with team/main")).toBeVisible();
});

test("failed fetch labels cached comparisons stale without erasing the graph", async () => {
  const { client, page } = fixture();
  page.upstream.freshness = "stale";
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0} />);
  expect(await screen.findByText(/Fetch failed/)).toHaveTextContent("last-known upstream state");
  expect(screen.getByRole("button", { name: "Incoming Changes · 3 commits" })).toBeVisible();
  expect(screen.getByRole("button", { name: /Merge topic, Commit/ })).toBeVisible();
});

test("late incoming results cannot replace a selected outgoing comparison", async () => {
  const user = userEvent.setup();
  const { client } = fixture();
  const late = deferred<CommitFilesResult>();
  const original = client.upstreamFiles;
  client.upstreamFiles = (entry, token) => token === incoming.token ? late.promise : original(entry, token);
  const onSelect = vi.fn();
  render(<HistoryGraph client={client} entryId="one" selectionGeneration={0}
    comparison={{ selection: null, onSelect, captureAutoSelection: () => onSelect, onInvalidate: vi.fn() }} />);
  await user.click(await screen.findByRole("button", { name: "Incoming Changes · 3 commits" }));
  await user.click(screen.getByRole("button", { name: "Outgoing Changes · 2 commits" }));
  await screen.findByText("outgoing.txt");
  await act(async () => late.resolve(await original("one", incoming.token)));
  expect(screen.queryByText("incoming.txt")).not.toBeInTheDocument();
  expect(onSelect).toHaveBeenCalledTimes(1);
});
