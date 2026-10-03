/** Exercises CLI failure classification and omission of untrusted manifest payloads. */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { selectChecks } from '../../scripts/verify.mjs';

function manifest() {
  return {
    schemaVersion: 1, boundary: 'repository-verification-not-native-preview', suite: 'unit',
    status: 'passed', metadataStatus: 'available', identifierInventory: 'available',
    startedAt: '2026-01-01T00:00:00.000Z', finishedAt: '2026-01-01T00:00:01.000Z',
    revision: 'a'.repeat(40), dirty: true,
    checks: selectChecks('unit').map(spec => ({ id: spec.id, required: true, status: 'passed', code: 'ok',
      summary: spec.reporter === 'rust' ? { passed: 1, failed: 0, ignored: 0 }
        : spec.reporter === 'vitest' ? { numPassedTests: 1, numFailedTests: 0, numPendingTests: 0, numTotalTests: 1 }
        : { total: 1, passed: 1, failed: 0, skipped: 0, todo: 0 } })),
  };
}
async function invoke(context, report) {
  const directory = await mkdtemp(join(tmpdir(), 'gitview-agent-report-'));
  context.after(() => rm(directory, { recursive: true, force: true }));
  const path = join(directory, 'manifest.json');
  await writeFile(path, JSON.stringify(report));
  return spawnSync(process.execPath, [resolve('.agents/scripts/verification-report.mjs'), '--manifest', path], { encoding: 'utf8' });
}

test('recorded failures stay nonzero without leaking supplied commands or private payloads', async context => {
  const report = manifest();
  report.status = 'failed';
  report.environment = { token: 'private-agent-payload' };
  Object.assign(report.checks[0], { status: 'failed', code: 'check_failed',
    rerun: ['node', 'private-agent-payload'], failures: ['private-agent-payload'], summary: null });
  report.checks[1].summary.private_field = 'private-agent-payload';
  const result = await invoke(context, report);
  assert.equal(result.status, 1);
  const output = JSON.parse(result.stdout);
  assert.equal(output.recordedStatus, 'failed');
  assert.equal(output.checks[0].code, 'check_failed');
  assert.equal(output.freshness, 'not_assessed');
  assert.equal(result.stdout.includes('private-agent-payload'), false);
  assert.equal(result.stdout.includes('private_field'), false);
});

test('missing required checks cannot be presented as a recorded pass', async context => {
  const report = manifest();
  report.checks.pop();
  const result = await invoke(context, report);
  assert.equal(result.status, 2);
  assert.equal(JSON.parse(result.stderr).error, 'invalid_verification_report');
  assert.equal(result.stdout, '');
});

test('duplicate check rows cannot replace a missing required boundary', async context => {
  const report = manifest();
  report.checks[2] = structuredClone(report.checks[0]);
  const result = await invoke(context, report);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
});

test('TODO evidence cannot accompany a passing required check', async context => {
  const report = manifest();
  report.checks[2].summary.todo = 1;
  const result = await invoke(context, report);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, '');
});
