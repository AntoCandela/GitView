/**
 * Adapts the renderer's typed client to fixed Tauri commands.
 * Tracing carries only operation identity and terminal transport facts; Git and path policy stay in Rust.
 */

import { invoke } from "@tauri-apps/api/core";
import type { ObservationSnapshot } from "../contracts/changes";
import type { RepositoryFileResult, RepositoryFilesResult } from "../contracts/browsing";
import type { ReviewResult } from "../contracts/diff";
import type { HistoryPageResult } from "../contracts/history";
import type { CommitFilesResult, CommitReviewResult, ContextOptionsResult } from "../contracts/inspection";
import {
  MAX_RENDERER_DIAGNOSTIC_DURATION_MS,
  type DiagnosticHealth,
  type DiagnosticHealthClient,
  type RendererDiagnostic,
  type RepositoryCommand,
} from "../contracts/Diagnostics";

import type {
  OpenOutcome,
  RepositoryClient,
  RepositoryMutationOutcome,
  SelectOutcome,
  WorkspaceSnapshot,
} from "../contracts/repositories";

export const repositoryClient: RepositoryClient & DiagnosticHealthClient = {
  snapshot: () => invokeRepository<WorkspaceSnapshot>("workspace_snapshot"),
  preferredLanguages: () => invoke<{ languages: string[] }>("preferred_languages"),
  openChosenRepository: (locale) => invokeRepository<OpenOutcome>("open_chosen_repository", { locale }),
  selectContext: (entryId) =>
    invokeRepository<SelectOutcome>("select_context", { entryId }),
  refreshEntryAvailability: (entryId) =>
    invokeRepository<WorkspaceSnapshot>("refresh_entry_availability", { entryId }),
  observeSelectedContext: (entryId) =>
    invokeRepository<ObservationSnapshot>("observe_selected_context", { entryId }),
  reviewFile: (entryId, observationRevision, pathId, category) =>
    invokeRepository<ReviewResult>("review_file", { entryId, observationRevision, pathId, category }),
  historyPage: (entryId, cursor, branch) => invokeRepository<HistoryPageResult>("history_page", { entryId, cursor, branch: branch ?? null }),
  listContexts: (entryId) => invokeRepository<ContextOptionsResult>("list_contexts", { entryId }),
  selectWorktree: (entryId, worktreeId) => invokeRepository<RepositoryMutationOutcome>("select_worktree", { entryId, worktreeId }),
  upstreamFiles: (entryId, token) => invokeRepository<CommitFilesResult>("upstream_files", { entryId, token }),
  commitFiles: (entryId, commitOid, parentOid) => invokeRepository<CommitFilesResult>("commit_files", { entryId, commitOid, parentOid }),
  reviewCommitFile: (entryId, commitOid, parentOid, fileId) =>
    invokeRepository<CommitReviewResult>("review_commit_file", { entryId, commitOid, parentOid, fileId }),
  listRepositoryFiles: (entryId, request) =>
    invokeRepository<RepositoryFilesResult>("list_repository_files", { entryId, request }),
  reviewRepositoryFile: (entryId, listingId, fileId) =>
    invokeRepository<RepositoryFileResult>("review_repository_file", { entryId, listingId, fileId }),
  renameRepository: (entryId, displayName) =>
    invokeRepository<RepositoryMutationOutcome>("rename_repository", { entryId, displayName }),
  removeRepository: (entryId) =>
    invokeRepository<RepositoryMutationOutcome>("remove_repository", { entryId }),
  diagnosticHealth: () => invoke<DiagnosticHealth>("diagnostic_health"),
};

async function invokeRepository<T>(command: RepositoryCommand, args: Record<string, unknown> = {}): Promise<T> {
  const operationId = crypto.randomUUID();
  const startedAt = performance.now();
  let phase: RendererDiagnostic["phase"] = "completed";
  try {
    return await invoke<T>(command, { ...args, operationId });
  } catch (error) {
    phase = "transport_failed";
    throw error;
  } finally {
    const durationMs = Math.min(MAX_RENDERER_DIAGNOSTIC_DURATION_MS, Math.max(0, Math.floor(performance.now() - startedAt)));
    void submitRendererDiagnostic({ operationId, command, phase, durationMs });
  }
}

async function submitRendererDiagnostic(diagnostic: RendererDiagnostic): Promise<void> {
  try {
    await invoke<void>("record_renderer_diagnostic", { diagnostic });
  } catch {
    // Diagnostics are best effort: a failed submission leaves a partial trace, never changes the repository result.
  }
}
