/** Assigns branch identity colors independently of shifting graph lanes; shared ancestry stays shared. */

import type { HistoryPage } from "../../../contracts/history";

const paletteSize = 8;

function colorIndex(identity: string): number {
  let hash = 0;
  for (let index = 0; index < identity.length; index += 1) hash = (Math.imul(hash, 31) + identity.charCodeAt(index)) >>> 0;
  return hash % paletteSize;
}

function colorToken(index: number): string {
  return `var(--history-color-${index})`;
}

/** The same ref set keeps its colors across refresh/pagination; up to eight refs receive distinct colors. */
export function colorHistory(page: HistoryPage | null, preferredBranch: string | null) {
  const refs = new Map<string, string>();
  const commits = new Map<string, string>();
  const occupied = new Set<number>();
  const branches = (page?.refs ?? []).filter((ref) => ref.kind !== "tag")
    .sort((left, right) => `${left.kind}:${left.name}`.localeCompare(`${right.kind}:${right.name}`));
  for (const ref of branches) {
    const identity = `${ref.kind}:${ref.name}`;
    let index = colorIndex(identity);
    for (let offset = 0; occupied.has(index) && offset < paletteSize; offset += 1) index = (index + 1) % paletteSize;
    occupied.add(index);
    const color = colorToken(index);
    refs.set(identity, color);
    if (!commits.has(ref.commitOid) || (ref.kind === "local_branch" && ref.name === preferredBranch)) commits.set(ref.commitOid, color);
  }
  for (const commit of page?.commits ?? []) {
    const color = commits.get(commit.oid) ?? colorToken(colorIndex(commit.oid));
    commits.set(commit.oid, color);
    commit.parents.forEach((parent, parentIndex) => {
      if (commits.has(parent.oid)) return;
      let parentColor = parentIndex === 0 ? color : colorToken(colorIndex(parent.oid));
      if (parentIndex > 0 && parentColor === color) parentColor = colorToken((colorIndex(parent.oid) + 1) % paletteSize);
      commits.set(parent.oid, parentColor);
    });
  }
  return { refs, commits };
}
