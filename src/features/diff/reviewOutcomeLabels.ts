/** Shared explanations for native comparison outcomes, independent of live or historical endpoints. */

import type { ReviewUnavailableCode, ReviewUnsupportedReason } from "../../contracts/diff";

export const unsupportedLabels: Record<ReviewUnsupportedReason, string> = {
  conflict: "Conflicted paths cannot be compared as an ordinary change.",
  rename_or_copy: "Rename and copy comparisons are not supported.",
  type_change: "File type changes are not supported.",
  submodule: "Submodule comparisons are not supported.",
  binary: "Binary content cannot be shown as text.",
  large_or_truncated: "This comparison exceeds the safe preview limit.",
  unborn_head: "Staged comparison requires an existing HEAD commit.",
  unsupported_encoding: "This file is not supported UTF-8 text.",
  other: "This path cannot be safely previewed as text.",
};
export const unavailableLabels: Record<ReviewUnavailableCode, string> = {
  inaccessible: "The file or repository cannot be accessed.",
  git_unavailable: "Git is unavailable.",
  unsafe_repository: "Git requires trusted repository ownership.",
  timeout: "The file comparison timed out.",
  changed_during_read: "The file changed while it was being read.",
  invalid_output: "Git returned a comparison that could not be read safely.",
};
