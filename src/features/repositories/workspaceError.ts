/** Keeps workspace failures as stable facts and translates only when rendered. */
import type { WorkspaceRejectionCode } from "../../contracts/repositories";
import { translate, type Locale, type MessageKey } from "../../i18n";

export type WorkspaceError =
  | { readonly domain: "rejection"; readonly code: WorkspaceRejectionCode }
  | { readonly domain: "workspace"; readonly code: "desktop_unavailable" | "restoration_connection" | "open_connection" | "check_connection" | "not_found" | "switch_connection" | "connection_changed" | "remove_connection" | "rename_connection" };

const rejectionKeys: Record<WorkspaceRejectionCode, MessageKey> = {
  git_unavailable: "repo.error.git_unavailable", not_repository: "repo.error.not_repository", inaccessible: "repo.error.inaccessible",
  unsafe_repository: "repo.error.unsafe_repository", probe_timeout: "repo.error.probe_timeout", repository_changed: "repo.error.repository_changed",
  unsupported_path_encoding: "repo.error.unsupported_path_encoding", repository_unavailable: "repo.error.repository_unavailable",
  invalid_display_name: "repo.error.invalid_display_name", superseded_selection: "repo.error.superseded_selection",
};
const workspaceKeys: Record<Extract<WorkspaceError, { domain: "workspace" }>["code"], MessageKey> = {
  desktop_unavailable: "repo.error.desktop_unavailable", restoration_connection: "repo.error.restoration_connection", open_connection: "repo.error.open_connection",
  check_connection: "repo.error.check_connection", not_found: "repo.error.not_found", switch_connection: "repo.error.switch_connection",
  connection_changed: "repo.error.connection_changed", remove_connection: "repo.error.remove_connection", rename_connection: "repo.error.rename_connection",
};

export function workspaceErrorMessage(error: WorkspaceError, locale: Locale): string {
  return translate(locale, error.domain === "rejection" ? rejectionKeys[error.code] : workspaceKeys[error.code]);
}
