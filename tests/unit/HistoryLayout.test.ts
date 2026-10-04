/** Verifies persistent edge topology and unresolved ancestry rather than incidental SVG serialization. */

import { expect, test } from "vitest";
import { layoutHistory, type HistoryRow } from "../../src/features/history/graph/layout";
import { historyCommit, historyOids, mergeHistory } from "../support/history";

const { merge, first, second, root, missing } = historyOids;

function outgoing(row: HistoryRow) {
  return row.segments.filter((segment) => segment.from === "node");
}

function incoming(row: HistoryRow) {
  return row.segments.filter((segment) => segment.to === "node");
}

test("ordered merge edges stay separate until their shared ancestor row", () => {
  const layout = layoutHistory(mergeHistory());
  const [mergeRow, firstRow, secondRow, rootRow] = layout.rows;
  const parents = outgoing(mergeRow);
  expect(parents.map(({ oid, parentIndex }) => ({ oid, parentIndex }))).toEqual([
    { oid: first, parentIndex: 0 },
    { oid: second, parentIndex: 1 },
  ]);
  expect(parents[0].fromLane).toBe(mergeRow.lane);
  expect(parents[0].toLane).toBe(mergeRow.lane);
  expect(parents[1].fromLane).toBe(mergeRow.lane);
  expect(parents[1].toLane).not.toBe(parents[0].toLane);
  expect(incoming(firstRow)).toMatchObject([{ edgeKey: parents[0].edgeKey, fromLane: parents[0].toLane, toLane: firstRow.lane }]);
  expect(incoming(secondRow)).toMatchObject([{ edgeKey: parents[1].edgeKey, fromLane: parents[1].toLane, toLane: secondRow.lane }]);
  expect(outgoing(firstRow)[0].toLane).toBe(firstRow.lane);
  expect(outgoing(secondRow)[0].toLane).toBe(secondRow.lane);
  expect(incoming(rootRow)).toMatchObject([
    { oid: root, edgeKey: outgoing(firstRow)[0].edgeKey, fromLane: firstRow.lane, toLane: rootRow.lane, parentIndex: null },
    { oid: root, edgeKey: outgoing(secondRow)[0].edgeKey, fromLane: secondRow.lane, toLane: rootRow.lane, parentIndex: null },
  ]);
  expect(outgoing(rootRow)).toEqual([]);
  expect(layout.stubs).toEqual([]);
});

test("three sibling histories keep distinct active lanes until the actual parent row", () => {
  const parent = { oid: root, state: "loaded" as const };
  const layout = layoutHistory([
    historyCommit(merge, [parent]),
    historyCommit(first, [parent]),
    historyCommit(second, [parent]),
    historyCommit(root),
  ]);
  const [oldestTipRow, middleTipRow, newestTipRow, parentRow] = layout.rows;
  const firstEdge = outgoing(oldestTipRow)[0];
  const secondEdge = outgoing(middleTipRow)[0];
  const thirdEdge = outgoing(newestTipRow)[0];
  expect(new Set([firstEdge.toLane, secondEdge.toLane, thirdEdge.toLane]).size).toBe(3);
  expect(new Set([firstEdge.edgeKey, secondEdge.edgeKey, thirdEdge.edgeKey]).size).toBe(3);
  expect([firstEdge.colorOid, secondEdge.colorOid, thirdEdge.colorOid]).toEqual([merge, first, second]);
  expect(middleTipRow.segments).toContainEqual(expect.objectContaining({
    edgeKey: firstEdge.edgeKey, colorOid: merge, oid: root,
    from: "top", to: "bottom", fromLane: firstEdge.toLane, toLane: firstEdge.toLane, parentIndex: null,
  }));
  expect(newestTipRow.segments.filter((segment) => segment.to === "bottom")).toMatchObject([
    { edgeKey: firstEdge.edgeKey, fromLane: firstEdge.toLane, toLane: firstEdge.toLane },
    { edgeKey: secondEdge.edgeKey, fromLane: secondEdge.toLane, toLane: secondEdge.toLane },
    { edgeKey: thirdEdge.edgeKey, fromLane: newestTipRow.lane, toLane: newestTipRow.lane },
  ]);
  expect(incoming(parentRow)).toMatchObject([
    { edgeKey: firstEdge.edgeKey, colorOid: merge, fromLane: firstEdge.toLane, toLane: parentRow.lane },
    { edgeKey: secondEdge.edgeKey, colorOid: first, fromLane: secondEdge.toLane, toLane: parentRow.lane },
    { edgeKey: thirdEdge.edgeKey, colorOid: second, fromLane: thirdEdge.toLane, toLane: parentRow.lane },
  ]);
  expect(layout.laneCount).toBeGreaterThanOrEqual(3);
  expect(layout.stubs).toEqual([]);
});

test("later pages resolve only actual parents and preserve every prefix segment", () => {
  const commits = [
    historyCommit(first, [{ oid: root, state: "outside_page" }, { oid: missing, state: "unavailable" }]),
    historyCommit(second, [{ oid: root, state: "outside_page" }]),
  ];
  const before = layoutHistory(commits);
  const after = layoutHistory([...commits, historyCommit(merge), historyCommit(root)]);
  expect(before.rows[0].commit.root).toBe(false);
  expect(before.rows[1].commit.root).toBe(false);
  expect(before.stubs.filter((stub) => stub.oid === root)).toHaveLength(2);
  expect(before.stubs.filter((stub) => stub.oid === root).map((stub) => stub.state)).toEqual(["outside_page", "outside_page"]);
  expect(after.rows.slice(0, commits.length)).toEqual(before.rows);
  expect(after.stubs).toEqual(before.stubs.filter((stub) => stub.oid === missing));
  expect(incoming(after.rows[2])).toEqual([]);
  expect(incoming(after.rows[3]).map((segment) => segment.edgeKey)).toEqual(
    before.stubs.filter((stub) => stub.oid === root).map((stub) => stub.edgeKey),
  );
});

test("missing ancestry passes through independent roots without creating a false connection", () => {
  const child = historyCommit(first, [{ oid: missing, state: "unavailable" }], "Shallow child");
  const layout = layoutHistory([child, historyCommit(root), historyCommit(second)]);
  const [childRow, rootRow, otherRootRow] = layout.rows;
  const edge = outgoing(childRow)[0];
  expect(layout.stubs).toMatchObject([
    { oid: missing, edgeKey: edge.edgeKey, colorOid: first, lane: edge.toLane, state: "unavailable" },
  ]);
  expect(childRow.commit.root).toBe(false);
  expect(rootRow.commit.root).toBe(true);
  expect(otherRootRow.commit.root).toBe(true);
  expect(rootRow.lane).not.toBe(edge.toLane);
  expect(otherRootRow.lane).toBe(rootRow.lane);
  expect(rootRow.segments).toMatchObject([
    { edgeKey: edge.edgeKey, fromLane: edge.toLane, toLane: edge.toLane, from: "top", to: "bottom", parentIndex: null },
  ]);
  expect(otherRootRow.segments).toEqual(rootRow.segments);
  expect(incoming(rootRow)).toEqual([]);
  expect(outgoing(rootRow)).toEqual([]);
});

test("an unavailable parent resolves only when its actual commit becomes loaded", () => {
  const child = historyCommit(first, [{ oid: missing, state: "unavailable" }]);
  const before = layoutHistory([child, historyCommit(root)]);
  const after = layoutHistory([child, historyCommit(root), historyCommit(missing)]);
  expect(before.stubs).toMatchObject([{ oid: missing, state: "unavailable" }]);
  expect(after.rows.slice(0, before.rows.length)).toEqual(before.rows);
  expect(incoming(after.rows[2])).toMatchObject([
    { oid: missing, edgeKey: before.stubs[0].edgeKey, colorOid: first, fromLane: before.stubs[0].lane, toLane: after.rows[2].lane },
  ]);
  expect(after.stubs).toEqual([]);
});

test.each([
  ["unavailable", "outside_page"],
  ["outside_page", "unavailable"],
] as const)("shared missing stubs retain separate edges with unavailable priority: %s then %s", (firstState, secondState) => {
  const layout = layoutHistory([
    historyCommit(first, [{ oid: missing, state: firstState }]),
    historyCommit(second, [{ oid: missing, state: secondState }]),
  ]);
  expect(layout.stubs).toMatchObject([
    { oid: missing, edgeKey: outgoing(layout.rows[0])[0].edgeKey, colorOid: first, state: "unavailable" },
    { oid: missing, edgeKey: outgoing(layout.rows[1])[0].edgeKey, colorOid: second, state: "unavailable" },
  ]);
  expect(layout.stubs[0].lane).not.toBe(layout.stubs[1].lane);
  expect(layout.stubs[0].edgeKey).not.toBe(layout.stubs[1].edgeKey);
});

test("octopus merges retain every ordered parent without shifting surviving lanes", () => {
  const parents = [first, second, root].map((oid) => ({ oid, state: "loaded" as const }));
  const layout = layoutHistory([historyCommit(merge, parents), historyCommit(first), historyCommit(second), historyCommit(root)]);
  const [mergeRow, firstRow, secondRow, rootRow] = layout.rows;
  const edges = outgoing(mergeRow);
  expect(edges.map(({ oid, parentIndex }) => ({ oid, parentIndex }))).toEqual([
    { oid: first, parentIndex: 0 }, { oid: second, parentIndex: 1 }, { oid: root, parentIndex: 2 },
  ]);
  expect(new Set(edges.map((edge) => edge.toLane)).size).toBe(3);
  expect(new Set(edges.map((edge) => edge.edgeKey)).size).toBe(3);
  expect(incoming(firstRow)).toMatchObject([{ oid: first, edgeKey: edges[0].edgeKey, fromLane: edges[0].toLane, toLane: firstRow.lane }]);
  expect(incoming(secondRow)).toMatchObject([{ oid: second, edgeKey: edges[1].edgeKey, fromLane: edges[1].toLane, toLane: secondRow.lane }]);
  expect(incoming(rootRow)).toMatchObject([{ oid: root, edgeKey: edges[2].edgeKey, fromLane: edges[2].toLane, toLane: rootRow.lane }]);
  expect(firstRow.segments).toContainEqual(expect.objectContaining({
    edgeKey: edges[2].edgeKey, fromLane: edges[2].toLane, toLane: edges[2].toLane, from: "top", to: "bottom",
  }));
  expect(secondRow.segments).toContainEqual(expect.objectContaining({
    edgeKey: edges[2].edgeKey, fromLane: edges[2].toLane, toLane: edges[2].toLane, from: "top", to: "bottom",
  }));
  expect(outgoing(firstRow)).toEqual([]);
  expect(outgoing(secondRow)).toEqual([]);
  expect(outgoing(rootRow)).toEqual([]);
  expect(layout.stubs).toEqual([]);
});

test("new histories reuse the lowest vacant lane without moving a live edge", () => {
  const layout = layoutHistory([
    historyCommit(merge, [{ oid: first, state: "loaded" }, { oid: missing, state: "outside_page" }]),
    historyCommit(first),
    historyCommit(second, [{ oid: root, state: "outside_page" }]),
  ]);
  const [mergeRow, firstRow, secondRow] = layout.rows;
  const pendingEdge = outgoing(mergeRow)[1];
  expect(secondRow.lane).toBe(firstRow.lane);
  expect(outgoing(secondRow)[0].toLane).toBe(secondRow.lane);
  expect(secondRow.segments).toContainEqual(expect.objectContaining({
    edgeKey: pendingEdge.edgeKey, fromLane: pendingEdge.toLane, toLane: pendingEdge.toLane, from: "top", to: "bottom",
  }));
  expect(layout.stubs).toEqual(expect.arrayContaining([
    expect.objectContaining({ oid: root, lane: secondRow.lane }),
    expect.objectContaining({ oid: missing, edgeKey: pendingEdge.edgeKey, lane: pendingEdge.toLane }),
  ]));
});

test("first-parent continuation keeps the node lane while additional parents reuse vacant slots", () => {
  const layout = layoutHistory([
    historyCommit(merge, [{ oid: first, state: "loaded" }, { oid: second, state: "loaded" }]),
    historyCommit(first),
    historyCommit(second, [{ oid: root, state: "outside_page" }, { oid: missing, state: "outside_page" }]),
  ]);
  const [, firstRow, secondRow] = layout.rows;
  const edges = outgoing(secondRow);
  expect(edges).toMatchObject([
    { oid: root, parentIndex: 0, fromLane: secondRow.lane, toLane: secondRow.lane },
    { oid: missing, parentIndex: 1, fromLane: secondRow.lane, toLane: firstRow.lane },
  ]);
  expect(incoming(secondRow)[0].fromLane).toBe(secondRow.lane);
  expect(layout.stubs).toHaveLength(2);
});

test("empty history has no ancestry and retains a usable minimum gutter", () => {
  expect(layoutHistory([])).toEqual({ rows: [], stubs: [], laneCount: 1 });
});
