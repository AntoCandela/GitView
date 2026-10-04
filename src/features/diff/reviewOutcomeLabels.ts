/** Maps native comparison facts to messages; callers translate with the current interface locale. */

import type { ReviewUnavailableCode, ReviewUnsupportedReason } from "../../contracts/diff";
import type { MessageKey } from "../../i18n";

export const unsupportedMessageKeys: Record<ReviewUnsupportedReason, MessageKey> = {
  conflict: "diff.unsupported.conflict",
  rename_or_copy: "diff.unsupported.renameOrCopy",
  type_change: "diff.unsupported.typeChange",
  submodule: "diff.unsupported.submodule",
  binary: "diff.unsupported.binary",
  large_or_truncated: "diff.unsupported.largeOrTruncated",
  unborn_head: "diff.unsupported.unbornHead",
  unsupported_encoding: "diff.unsupported.encoding",
  other: "diff.unsupported.other",
};
export const unavailableMessageKeys: Record<ReviewUnavailableCode, MessageKey> = {
  inaccessible: "diff.unavailable.inaccessible",
  git_unavailable: "diff.unavailable.git",
  unsafe_repository: "diff.unavailable.unsafeRepository",
  timeout: "diff.unavailable.timeout",
  changed_during_read: "diff.unavailable.changedDuringRead",
  invalid_output: "diff.unavailable.invalidOutput",
};
