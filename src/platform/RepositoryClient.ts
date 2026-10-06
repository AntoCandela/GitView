/**
 * Adapts the renderer's typed client to fixed Tauri commands.
 * Tracing carries only operation identity and terminal transport facts; Git and path policy stay in Rust.
 */

import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  CompanionClient, CompanionSettingsClient, ReviewHandoffClient, ReviewSurfaceClient, SurfaceNotice,
} from "../contracts/companion";
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

export const reviewSurfaceClient: ReviewSurfaceClient = {
  bootstrap: () => invoke("review_surface_bootstrap"),
  snapshot: () => invoke("review_surface_snapshot"),
  async subscribe(listener) {
    let active = true;
    const channel = new Channel<SurfaceNotice>();
    channel.onmessage = (notice) => { if (active) listener(notice); };
    try {
      await invoke("subscribe_review_surface", { channel });
    } catch (error) {
      active = false;
      throw error;
    }
    // Native owns one registration per caller and clears it on window destruction.
    return () => { active = false; };
  },
};

export const companionClient: CompanionClient = {
  selectContext: repositoryClient.selectContext,
  observeSelectedContext: repositoryClient.observeSelectedContext,
  reviewFile: repositoryClient.reviewFile,
  begin: (openEpoch) => invoke("begin_companion_review", { openEpoch }),
  dismiss: () => invoke("dismiss_companion"),
  requestHandoff: (request) => invoke("request_review_handoff", { request }),
  quit: () => invoke("quit_companion"),
};

export const companionSettingsClient: CompanionSettingsClient = {
  state: () => invoke("companion_state"),
  setEnabled: (enabled) => invoke("set_companion_enabled", { enabled }),
  publishPresentation: (presentation) => invoke("publish_companion_presentation", { presentation }),
};

export const reviewHandoffClient: ReviewHandoffClient = {
  pending: () => invoke("pending_review_handoff"),
  claim: (requestId, contextEpoch) => invoke("claim_review_handoff", { requestId, contextEpoch }),
  ack: (requestId, contextEpoch, outcome) => invoke("ack_review_handoff", { requestId, contextEpoch, outcome }),
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
