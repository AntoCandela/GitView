/** Transports the closed PR commands, preserving domain failures separately from transport rejection. */
import { invoke } from '@tauri-apps/api/core';
import type { PullRequestClient, PullRequestCommand } from '../contracts/pullRequests';
import { MAX_RENDERER_DIAGNOSTIC_DURATION_MS, type RendererDiagnostic } from '../contracts/Diagnostics';

export const pullRequestClient: PullRequestClient = {
  status: entryId => request('pr_status', { entryId }),
  associations: (entryId, branch) => request('pr_associations', { entryId, branch }),
  mapHead: (entryId, associationId, owner, repository, headRef) => request('pr_map_head', { entryId, associationId, owner, repository, headRef }),
  choose: (entryId, associationId, candidateId) => request('pr_choose', { entryId, associationId, candidateId }),
  open: (entryId, prId) => request('pr_open', { entryId, prId }),
  page: (entryId, sessionId, collection, cursor, threadId) => request('pr_page', { entryId, sessionId, collection, cursor, threadId: threadId ?? null }),
  refresh: (entryId, sessionId) => request('pr_refresh', { entryId, sessionId }),
  compare: (entryId, sessionId, selection) => request('pr_compare', { entryId, sessionId, selection }),
  filesPage: (entryId, comparisonId, cursor) => request('pr_files_page', { entryId, comparisonId, cursor }),
  resolveAnchor: (entryId, sessionId, anchorId) => request('pr_resolve_anchor', { entryId, sessionId, anchorId }),
  file: (entryId, comparisonId, fileId) => request('pr_file', { entryId, comparisonId, fileId }),
  release: (entryId, sessionId) => request('pr_release', { entryId, sessionId }),
  openLink: (entryId, sessionId, linkId) => request('pr_open_link', { entryId, sessionId, linkId }),
};

async function request<T>(command: PullRequestCommand, args: Record<string, unknown>): Promise<T> {
  const operationId = crypto.randomUUID();
  const started = performance.now();
  let phase: RendererDiagnostic['phase'] = 'completed';
  try {
    return await invoke<T>(command, { ...args, operationId });
  } catch (error) {
    phase = 'transport_failed';
    throw error;
  } finally {
    const durationMs = Math.min(MAX_RENDERER_DIAGNOSTIC_DURATION_MS, Math.max(0, Math.floor(performance.now() - started)));
    void invoke('record_renderer_diagnostic', { diagnostic: { operationId, command, phase, durationMs } }).catch(() => {
      // Capture is best effort and must not alter the domain or transport result.
    });
  }
}
