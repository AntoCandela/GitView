/** Maps stable native history failure codes to presentation keys without retaining native prose. */
import type { HistoryErrorCode } from "../../contracts/history";
import type { MessageKey } from "../../i18n";

export const historyErrorKeys = {
  inaccessible: "history.error.inaccessible",
  git_unavailable: "history.error.git_unavailable",
  unsafe_repository: "history.error.unsafe_repository",
  timeout: "history.error.timeout",
  invalid_output: "history.error.invalid_output",
  resource_limit: "history.error.resource_limit",
  stale_selection: "history.error.stale_selection",
  stale_cursor: "history.error.stale_cursor",
  missing_objects: "history.error.missing_objects",
} as const satisfies Record<HistoryErrorCode, MessageKey>;

export type HistoryReadFailure = { kind: "unavailable" | "error"; code: HistoryErrorCode };
