/** Keeps each raw parent edge in a persistent lane until its actual parent row or an ancestry stub. */

import type { HistoryCommit } from "../../../contracts/history";

export interface HistorySegment {
  /** Stable child OID + raw parent index identity across every piece of this edge. */
  edgeKey: string;
  /** Source child OID used to look up its lineage color, not the shared target's color. */
  colorOid: string;
  oid: string;
  fromLane: number;
  toLane: number;
  from: "top" | "node";
  to: "node" | "bottom";
  parentIndex: number | null;
}

export interface HistoryRow {
  commit: HistoryCommit;
  lane: number;
  segments: HistorySegment[];
}

export interface HistoryStub {
  edgeKey: string;
  colorOid: string;
  oid: string;
  lane: number;
  state: "outside_page" | "unavailable";
}

type PendingEdge = Pick<HistorySegment, "oid" | "edgeKey" | "colorOid">;

/** Appended pages preserve prefix geometry; shared targets join only at their actual commit row. */
export function layoutHistory(commits: HistoryCommit[]): { rows: HistoryRow[]; stubs: HistoryStub[]; laneCount: number } {
  const parentStates = new Map<string, HistoryStub["state"]>();
  const lanes: (PendingEdge | null)[] = [];
  let laneCount = 1;
  const rows: HistoryRow[] = [];
  for (const commit of commits) {
    const incomingLane = lanes.findIndex((edge) => edge?.oid === commit.oid);
    const lane = incomingLane === -1 ? freeLane(lanes) : incomingLane;
    const segments: HistorySegment[] = [];
    lanes.forEach((edge, fromLane) => {
      if (edge === null) return;
      if (edge.oid === commit.oid) {
        segments.push({ ...edge, fromLane, toLane: lane, from: "top", to: "node", parentIndex: null });
        lanes[fromLane] = null;
      } else {
        segments.push({ ...edge, fromLane, toLane: fromLane, from: "top", to: "bottom", parentIndex: null });
      }
    });
    commit.parents.forEach((parent, parentIndex) => {
      const state = parent.state === "unavailable" ? "unavailable" : "outside_page";
      if (parentStates.get(parent.oid) !== "unavailable") parentStates.set(parent.oid, state);
      const edge: PendingEdge = { oid: parent.oid, edgeKey: `${commit.oid}:${parentIndex}`, colorOid: commit.oid };
      // Never share or compact live slots, even when two edges have the same target.
      const toLane = parentIndex === 0 ? lane : freeLane(lanes);
      lanes[toLane] = edge;
      segments.push({ ...edge, fromLane: lane, toLane, from: "node", to: "bottom", parentIndex });
    });
    laneCount = Math.max(laneCount, lane + 1, lanes.length);
    rows.push({ commit, lane, segments });
  }
  const stubs: HistoryStub[] = [];
  lanes.forEach((edge, lane) => {
    if (edge !== null) stubs.push({ ...edge, lane, state: parentStates.get(edge.oid) ?? "outside_page" });
  });
  return { rows, stubs, laneCount };
}

function freeLane(lanes: (PendingEdge | null)[]): number {
  const lane = lanes.indexOf(null);
  return lane === -1 ? lanes.length : lane;
}

