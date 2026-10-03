/** Native readiness requires actual artifact identity and explicit human observations. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { assessPreview, requiredScenarios, parseArguments } from '../../scripts/preview-evidence.mjs';

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'gitview-native-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const artifact = join(root, 'preview.dmg');
  await writeFile(artifact, 'real artifact bytes');
  return artifact;
}
function observedReport(artifactHash) {
  return { schemaVersion: 1, boundary: 'packaged-native', platform: 'macos-arm64', revision: 'a'.repeat(40), artifactHash,
    observation: { kind: 'human', observedAt: '2026-10-01T12:00:00Z', observer: 'maintainer-1' },
    prerequisites: { installedGit: 'passed', supportedOs: 'passed' },
    runtime: { os: 'macOS 15', architecture: 'arm64', webview: 'WKWebView', gitVersion: '2.50.0' },
    scenarios: Object.fromEntries(requiredScenarios.map(name => [name, { status: 'passed', observed: true }])) };
}

test('missing report keeps all scenarios not run and hashes actual file', async t => {
  const artifact = await fixture(t);
  const result = await assessPreview({ artifact, platform: 'macos-arm64' });
  assert.equal(result.ready, false);
  assert.equal(result.artifact.sha256, createHash('sha256').update('real artifact bytes').digest('hex'));
  assert.ok(Object.values(result.scenarios).every(item => item.status === 'not_run'));
});

test('browser observations cannot certify native readiness and payloads are omitted', async t => {
  const artifact = await fixture(t);
  const hash = createHash('sha256').update('real artifact bytes').digest('hex');
  const report = observedReport(hash);
  report.boundary = 'browser';
  report.privateNotes = '<html>/private/customer SECRET</html>';
  const result = await assessPreview({ artifact, platform: 'macos-arm64', report });
  assert.equal(result.ready, false);
  assert.doesNotMatch(JSON.stringify(result), /SECRET|private\/customer|<html>/);
});

test('explicit native observations pass validation only for the matching artifact', async t => {
  const artifact = await fixture(t);
  const report = observedReport(createHash('sha256').update('real artifact bytes').digest('hex'));
  assert.equal((await assessPreview({ artifact, platform: 'macos-arm64', report })).ready, true);
  report.artifactHash = 'b'.repeat(64);
  assert.equal((await assessPreview({ artifact, platform: 'macos-arm64', report })).ready, false);
});

test('missing, failed, malformed or unobserved scenarios never return ready', async t => {
  const artifact = await fixture(t);
  const report = observedReport(createHash('sha256').update('real artifact bytes').digest('hex'));
  delete report.scenarios.native_picker;
  assert.equal((await assessPreview({ artifact, platform: 'macos-arm64', report })).ready, false);
  report.scenarios.native_picker = { status: 'passed', observed: false };
  assert.equal((await assessPreview({ artifact, platform: 'macos-arm64', report })).ready, false);
  report.scenarios.native_picker = { status: 'failed', observed: true };
  assert.equal((await assessPreview({ artifact, platform: 'macos-arm64', report })).ready, false);
  assert.equal((await assessPreview({ artifact, platform: 'unsupported', report: {} })).ready, false);
});

test('native CLI requires artifact and supported platform and rejects unknown or duplicate flags', () => {
  assert.throws(() => parseArguments([]));
  assert.throws(() => parseArguments(['--artifact', 'x', '--platform', 'browser']));
  assert.throws(() => parseArguments(['--artifact', 'x', '--platform', 'macos-arm64', '--pass-all']));
  assert.throws(() => parseArguments(['--artifact', 'x', '--artifact', 'y', '--platform', 'macos-arm64']));
});
