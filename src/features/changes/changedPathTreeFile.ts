/** Maps observed changes to reusable explorer presentation; segments never authorize reads. */
import type { ChangedPath } from "../../contracts/changes";
import type { ChangeTreeFile, ChangeStatusFact } from "../../ui/file-explorer/ChangeTree";

/** Shares pending-change facts with explorers; segments are labels, never read authorization. */
export function changedPathTreeFile(file: ChangedPath): ChangeTreeFile {
  const statuses: ChangeStatusFact[] = [];
  if (file.conflict) statuses.push({ kind: "conflict" });
  else if (file.unsupportedKind) statuses.push({ kind: "unsupported", change: file.unsupportedKind });
  else if (file.untracked) statuses.push({ kind: "untracked" });
  else {
    if (file.staged) statuses.push({ kind: "staged", change: file.staged });
    if (file.unstaged) statuses.push({ kind: "unstaged", change: file.unstaged });
  }
  return {
    id: file.stablePathId, displayPath: file.displayPath, segments: file.segments, statuses,
    unsupported: Boolean(file.conflict || file.unsupportedKind),
    marker: file.conflict ? "!" : file.unsupportedKind ? "?" : file.untracked ? "U" : file.staged && file.unstaged ? "MM" : (file.unstaged ?? file.staged) === "added" ? "A" : (file.unstaged ?? file.staged) === "deleted" ? "D" : "M",
  };
}
