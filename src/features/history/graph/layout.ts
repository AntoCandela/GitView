/** Maps ordered raw parents to continuous lanes; unresolved ancestry always ends in a stub. */

import type { HistoryCommit } from "../../../contracts/history";

export interface HistorySegment {
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
  oid: string;
  lane: number;
  state: "outside_page" | "unavailable";
}

/** Recomputing over appended pages resolves old boundary stubs without changing raw parent order. */
export function layoutHistory(commits: HistoryCommit[]): { rows: HistoryRow[]; stubs: HistoryStub[]; laneCount: number } {
  const loaded = new Set(commits.map((commit) => commit.oid));
  const parentStates = new Map<string, HistoryStub["state"]>();
  let lanes: string[] = [];
  let laneCount = 1;
  const rows: HistoryRow[] = [];
  for (const commit of commits) {
    let lane = lanes.indexOf(commit.oid);
    const incoming = lane !== -1;
    if (!incoming) {
      lane = lanes.length;
      lanes.push(commit.oid);
    }
    const next = lanes.filter((oid) => oid !== commit.oid);
    let insertion = lane;
    for (const parent of commit.parents) {
      if (!loaded.has(parent.oid)) {
        const state = parent.state === "unavailable" ? "unavailable" : "outside_page";
        if (parentStates.get(parent.oid) !== "unavailable") parentStates.set(parent.oid, state);
      }
      if (!next.includes(parent.oid)) {
        next.splice(insertion, 0, parent.oid);
        insertion += 1;
      }
    }
    const segments: HistorySegment[] = [];
    lanes.forEach((oid, fromLane) => {
      if (oid === commit.oid) {
        if (incoming) segments.push({ oid, fromLane, toLane: lane, from: "top", to: "node", parentIndex: null });
      } else {
        segments.push({ oid, fromLane, toLane: next.indexOf(oid), from: "top", to: "bottom", parentIndex: null });
      }
    });
    commit.parents.forEach((parent, parentIndex) => {
      segments.push({ oid: parent.oid, fromLane: lane, toLane: next.indexOf(parent.oid), from: "node", to: "bottom", parentIndex });
    });
    laneCount = Math.max(laneCount, lanes.length, next.length);
    rows.push({ commit, lane, segments });
    lanes = next;
  }
  return {
    rows,
    laneCount,
    stubs: lanes.map((oid, lane) => ({ oid, lane, state: parentStates.get(oid) ?? "outside_page" })),
  };
}

