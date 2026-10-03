/** Maps observed changes to reusable explorer presentation; segments never authorize reads. */
import type { ChangedPath, ChangeKind, UnsupportedKind } from "../../contracts/changes";
import type { ChangeTreeFile } from "../../ui/file-explorer/ChangeTree";

const kindLabels: Record<ChangeKind, string> = { added: "Added", modified: "Modified", deleted: "Deleted" };
const unsupportedLabels: Record<UnsupportedKind, string> = {
  rename_or_copy: "Rename or copy · unsupported",
  submodule: "Submodule · unsupported",
  type_change: "Type change · unsupported",
};

/** Shares pending-change presentation with explorers; segments are labels, never read authorization. */
export function changedPathTreeFile(file: ChangedPath): ChangeTreeFile {
  const status = file.conflict ? "Conflict" : file.unsupportedKind ? unsupportedLabels[file.unsupportedKind]
    : file.untracked ? "Untracked" : `${file.staged ? `Staged ${kindLabels[file.staged]}` : ""}${file.staged && file.unstaged ? ", " : ""}${file.unstaged ? `Unstaged ${kindLabels[file.unstaged]}` : ""}`;
  return {
    id: file.stablePathId, displayPath: file.displayPath, segments: file.segments, status,
    unsupported: Boolean(file.conflict || file.unsupportedKind),
    marker: file.conflict ? "!" : file.unsupportedKind ? "?" : file.untracked ? "U" : file.staged && file.unstaged ? "MM" : (file.unstaged ?? file.staged) === "added" ? "A" : (file.unstaged ?? file.staged) === "deleted" ? "D" : "M",
  };
}
