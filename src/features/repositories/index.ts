/** Exposes repository lifecycle and catalog/file sections to app composition. */
export { useWorkspace } from "./useWorkspace";
export { RepositoryBrowser } from "./catalog/RepositoryBrowser";
export type { RepositoryRenameState } from "./catalog/RepositoryNameEditor";
export { headLabel } from "./catalog/headLabel";
export { RepositoryFiles } from "./files/RepositoryFiles";
export { workspaceErrorMessage } from "./workspaceError";
export type { WorkspaceError } from "./workspaceError";
