/** Verifies closed PR commands preserve domain outcomes and isolate transport failures. */
import { invoke } from '@tauri-apps/api/core';
import { expect, test, vi } from 'vitest';
import { pullRequestClient } from '../../src/platform/PullRequestClient';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

test('PR status preserves a typed unavailable result instead of reporting transport failure', async () => {
  const failure = { kind: 'unavailable', code: 'integration_unavailable' };
  vi.mocked(invoke).mockResolvedValue(failure);
  await expect(pullRequestClient.status('entry')).resolves.toEqual(failure);
  expect(invoke).toHaveBeenCalledWith('pr_status', expect.objectContaining({ entryId: 'entry', operationId: expect.any(String) }));
});

const operations = [
  ['pr_associations', () => pullRequestClient.associations('entry', null)],
  ['pr_map_head', () => pullRequestClient.mapHead('entry', 'association', 'owner', 'repository', 'topic')],
  ['pr_choose', () => pullRequestClient.choose('entry', 'association', 'candidate')],
  ['pr_open', () => pullRequestClient.open('entry', 'pr')],
  ['pr_page', () => pullRequestClient.page('entry', 'session', 'thread_comments', 'cursor', 'thread')],
  ['pr_refresh', () => pullRequestClient.refresh('entry', 'session')],
  ['pr_compare', () => pullRequestClient.compare('entry', 'session', { kind: 'commit', commitId: 'commit', parentIndex: 1 })],
  ['pr_files_page', () => pullRequestClient.filesPage('entry', 'comparison', 'cursor')],
  ['pr_resolve_anchor', () => pullRequestClient.resolveAnchor('entry', 'session', 'anchor')],
  ['pr_file', () => pullRequestClient.file('entry', 'comparison', 'file')],
  ['pr_release', () => pullRequestClient.release('entry', 'session')],
  ['pr_open_link', () => pullRequestClient.openLink('entry', 'session', 'link')],
] as const;

test.each(operations)('%s preserves typed failure while diagnostic delivery is unavailable', async (command, start) => {
  vi.mocked(invoke).mockReset();
  const outcome = { kind: 'stale', code: 'stale_context' };
  vi.mocked(invoke).mockImplementation(async <T>(name: string) => {
    if (name === 'record_renderer_diagnostic') throw new Error('capture unavailable');
    return outcome as T;
  });
  await expect(start()).resolves.toBe(outcome);
  const call = vi.mocked(invoke).mock.calls[0];
  expect(call?.[0]).toBe(command);
  expect(call?.[1]).toMatchObject({ entryId: 'entry', operationId: expect.any(String) });
  expect(invoke).toHaveBeenLastCalledWith('record_renderer_diagnostic', { diagnostic: expect.objectContaining({ command, phase: 'completed' }) });
});

test.each(operations)('%s keeps transport rejection separate and excludes private error payload', async (command, start) => {
  vi.mocked(invoke).mockReset();
  const error = { credential: 'fixture-private-credential', path: '/fixture/private-path' };
  vi.mocked(invoke).mockImplementation(async <T>(name: string) => {
    if (name === 'record_renderer_diagnostic') return undefined as T;
    throw error;
  });
  await expect(start()).rejects.toBe(error);
  const metadata = vi.mocked(invoke).mock.calls[1]?.[1];
  expect(metadata).toEqual({ diagnostic: {
    operationId: expect.any(String), command, phase: 'transport_failed', durationMs: expect.any(Number),
  } });
});

test('comparison pagination and anchor resolution preserve only opaque native authority', async () => {
  vi.mocked(invoke).mockReset(); vi.mocked(invoke).mockResolvedValue({ kind: 'unavailable', code: 'network' });
  await pullRequestClient.filesPage('entry', 'comparison', 'cursor');
  await pullRequestClient.resolveAnchor('entry', 'session', 'anchor');
  expect(invoke).toHaveBeenCalledWith('pr_files_page', { entryId: 'entry', comparisonId: 'comparison', cursor: 'cursor', operationId: expect.any(String) });
  expect(invoke).toHaveBeenCalledWith('pr_resolve_anchor', { entryId: 'entry', sessionId: 'session', anchorId: 'anchor', operationId: expect.any(String) });
});
