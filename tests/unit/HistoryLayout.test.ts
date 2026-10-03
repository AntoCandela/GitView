/** Verifies merge topology and unresolved ancestry rather than incidental SVG serialization. */

import { expect, test } from "vitest";
import { layoutHistory } from "../../src/features/history/graph/layout";
import { historyCommit, historyOids, mergeHistory } from "../support/history";

const { merge, first, second, root, missing } = historyOids;

test("ordered merge parents split lanes and shared ancestry rejoins the existing lane", () => {
  const layout = layoutHistory(mergeHistory());
  const mergeRow = layout.rows.find((row) => row.commit.oid === merge)!;
  expect(mergeRow.segments.filter((segment) => segment.parentIndex !== null)).toEqual([
    { oid: first, fromLane: 0, toLane: 0, from: "node", to: "bottom", parentIndex: 0 },
    { oid: second, fromLane: 0, toLane: 1, from: "node", to: "bottom", parentIndex: 1 },
  ]);
  const topicRow = layout.rows.find((row) => row.commit.oid === second)!;
  expect(topicRow.lane).toBe(1);
  expect(topicRow.segments).toContainEqual({ oid: root, fromLane: 1, toLane: 0, from: "node", to: "bottom", parentIndex: 0 });
  const rootRow = layout.rows.find((row) => row.commit.oid === root)!;
  expect(rootRow.segments).toContainEqual({ oid: root, fromLane: 0, toLane: 0, from: "top", to: "node", parentIndex: null });
  expect(layout.stubs).toEqual([]);
});

test("later pages resolve a boundary parent without turning omitted parents into roots", () => {
  const child = historyCommit(first, [{ oid: root, state: "outside_page" }], "Child");
  const before = layoutHistory([child]);
  expect(before.stubs).toEqual([{ oid: root, lane: 0, state: "outside_page" }]);
  expect(before.rows[0].commit.root).toBe(false);
  const after = layoutHistory([child, historyCommit(root)]);
  expect(after.stubs).toEqual([]);
  expect(after.rows[1].segments).toContainEqual({ oid: root, fromLane: 0, toLane: 0, from: "top", to: "node", parentIndex: null });
});

test("missing ancestry survives a separate verified root and does not become a false root", () => {
  const child = historyCommit(first, [{ oid: missing, state: "unavailable" }], "Shallow child");
  const layout = layoutHistory([child, historyCommit(root)]);
  expect(layout.stubs).toEqual([{ oid: missing, lane: 0, state: "unavailable" }]);
  expect(layout.rows[0].commit.root).toBe(false);
  expect(layout.rows[1].commit.root).toBe(true);
  expect(layout.rows[1].lane).toBe(1);
  expect(layout.rows[1].segments).toContainEqual({ oid: missing, fromLane: 0, toLane: 0, from: "top", to: "bottom", parentIndex: null });
});

test("an unavailable parent resolves only when its actual commit becomes loaded", () => {
  const parent = { oid: missing, state: "unavailable" as const };
  expect(layoutHistory([historyCommit(first, [parent])]).stubs[0].state).toBe("unavailable");
  expect(layoutHistory([historyCommit(first, [parent]), historyCommit(missing)]).stubs).toEqual([]);
});

test("octopus merges retain every ordered parent and each continuation reaches its own commit", () => {
  const parents = [first, second, root].map((oid) => ({ oid, state: "loaded" as const }));
  const layout = layoutHistory([historyCommit(merge, parents), historyCommit(first), historyCommit(second), historyCommit(root)]);
  expect(layout.rows[0].segments.map((segment) => [segment.oid, segment.parentIndex, segment.toLane])).toEqual([
    [first, 0, 0], [second, 1, 1], [root, 2, 2],
  ]);
  for (const oid of [first, second, root]) {
    const row = layout.rows.find((candidate) => candidate.commit.oid === oid)!;
    expect(row.segments.some((segment) => segment.oid === oid && segment.to === "node")).toBe(true);
  }
  expect(layout.stubs).toEqual([]);
});
