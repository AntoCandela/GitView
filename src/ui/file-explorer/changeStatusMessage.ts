/** Localizes immutable change facts only at the shared explorer presentation boundary. */
import { translate, type Locale, type MessageKey } from "../../i18n";
import type { ChangeStatusFact } from "./changeTreeRows";

const simpleKeys: Record<"untracked" | "conflict" | "unchanged" | "unavailable", MessageKey> = {
  untracked: "tree.status.untracked", conflict: "tree.status.conflict", unchanged: "tree.status.unchanged", unavailable: "tree.status.unavailable",
};
const stagedKeys = { added: "tree.status.staged.added", modified: "tree.status.staged.modified", deleted: "tree.status.staged.deleted" } as const;
const unstagedKeys = { added: "tree.status.unstaged.added", modified: "tree.status.unstaged.modified", deleted: "tree.status.unstaged.deleted" } as const;
const unsupportedKeys = { rename_or_copy: "tree.status.unsupported.rename_or_copy", submodule: "tree.status.unsupported.submodule", type_change: "tree.status.unsupported.type_change" } as const;
const committedKeys = { added: "tree.status.committed.added", modified: "tree.status.committed.modified", deleted: "tree.status.committed.deleted", type_change: "tree.status.committed.type_change" } as const;

export function changeStatusMessage(locale: Locale, fact: ChangeStatusFact): string {
  switch (fact.kind) {
    case "staged": return translate(locale, stagedKeys[fact.change]);
    case "unstaged": return translate(locale, unstagedKeys[fact.change]);
    case "unsupported": return translate(locale, unsupportedKeys[fact.change]);
    case "committed": return translate(locale, committedKeys[fact.change]);
    default: return translate(locale, simpleKeys[fact.kind]);
  }
}
