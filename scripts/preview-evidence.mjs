/** Hashes a real packaged artifact and validates human evidence; it cannot observe native UI. */
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { resolve } from 'node:path';
import { isEntryPoint, parseOptions, readJsonBounded, writeEvidence } from './evidence.mjs';

export const platforms = {
  'macos-arm64': { architecture: 'arm64', os: /^macOS [0-9.]+$/, webview: 'WKWebView' },
  'windows-x64': { architecture: 'x64', os: /^Windows [0-9.]+$/, webview: 'WebView2' },
  'ubuntu-24.04-x64': { architecture: 'x64', os: /^Ubuntu 24\.04(?:\.[0-9]+)?$/, webview: 'WebKitGTK' },
};
export const requiredScenarios = ['install_launch', 'native_picker', 'native_ipc', 'secondary_window_denied', 'repository_open', 'linked_and_bare', 'live_observation', 'read_only_repository', 'diagnostics_correlation', 'diagnostics_privacy', 'diagnostics_health'];
export const requiredLocales = ['pt-BR', 'pt-PT', 'it', 'es', 'en-US', 'en-GB'];
export const requiredLocaleScenarios = ['selection_persistence', 'interface_errors_counts_accessibility', 'native_picker_title', 'repository_data_unchanged', 'linguistic_review'];
const statuses = new Set(['passed', 'failed', 'not_run', 'blocked']);
const validHash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const version = value => typeof value === 'string' && /^[0-9]+\.[0-9]+(?:\.[0-9]+)?$/.test(value);
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);

export function parseArguments(argv) {
  const options = parseOptions(argv, ['artifact', 'platform', 'report', 'output'], { output: '.verification' });
  if (!options.artifact || !Object.hasOwn(platforms, options.platform)) throw new Error('invalid_arguments');
  return options;
}

async function artifactIdentity(path) {
  try {
    const metadata = await stat(path);
    if (!metadata.isFile() || metadata.size === 0) return null;
    const hash = createHash('sha256');
    let bytes = 0;
    for await (const chunk of createReadStream(path)) { hash.update(chunk); bytes += chunk.length; }
    if (bytes !== metadata.size) return null;
    return { sha256: hash.digest('hex'), bytes };
  } catch { return null; }
}

/** A ready result only means a supplied report meets the schema, not independent certification. */
export async function assessPreview({ artifact, platform, report } = {}) {
  const identity = artifact ? await artifactIdentity(artifact) : null;
  const reasons = [];
  const reject = reason => { if (!reasons.includes(reason)) reasons.push(reason); };
  const expected = Object.hasOwn(platforms, platform) ? platforms[platform] : null;
  if (!expected) reject('unsupported_platform');
  if (!identity) reject('artifact_unavailable');
  const supplied = object(report);
  if (!supplied) reject('observation_report_missing');
  const input = supplied ? report : {};
  if (input.schemaVersion !== 1 || input.boundary !== 'packaged-native' || input.platform !== platform) reject('invalid_report_boundary');
  const revision = typeof input.revision === 'string' && /^[a-f0-9]{40,64}$/.test(input.revision) ? input.revision : null;
  if (!revision) reject('revision_missing');
  if (!validHash(input.artifactHash) || input.artifactHash !== identity?.sha256) reject('artifact_identity_mismatch');
  const observation = object(input.observation) ? input.observation : {};
  const observedAt = typeof observation.observedAt === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z$/.test(observation.observedAt) && Number.isFinite(Date.parse(observation.observedAt)) ? observation.observedAt : null;
  const observerValid = typeof observation.observer === 'string' && /^[A-Za-z0-9_-]{1,64}$/.test(observation.observer);
  if (observation.kind !== 'human' || !observedAt || !observerValid) reject('human_observation_missing');
  const runtimeInput = object(input.runtime) ? input.runtime : {};
  const runtime = { os: expected?.os.test(runtimeInput.os) ? runtimeInput.os : null, architecture: runtimeInput.architecture === expected?.architecture ? runtimeInput.architecture : null, webview: runtimeInput.webview === expected?.webview ? runtimeInput.webview : null, gitVersion: version(runtimeInput.gitVersion) ? runtimeInput.gitVersion : null };
  if (Object.values(runtime).some(value => value === null)) reject('runtime_metadata_missing');
  const prerequisites = {};
  for (const name of ['installedGit', 'supportedOs']) {
    prerequisites[name] = statuses.has(input.prerequisites?.[name]) ? input.prerequisites[name] : 'not_run';
    if (prerequisites[name] !== 'passed') reject('prerequisites_unverified');
  }
  const scenarios = {};
  for (const name of requiredScenarios) {
    const scenario = input.scenarios?.[name];
    const valid = object(scenario) && statuses.has(scenario.status) && typeof scenario.observed === 'boolean';
    scenarios[name] = { status: valid ? scenario.status : 'not_run', observed: valid ? scenario.observed : false };
    if (!valid || scenario.status !== 'passed' || scenario.observed !== true) reject('native_scenarios_unverified');
  }
  const localization = {};
  for (const locale of requiredLocales) {
    localization[locale] = {};
    for (const name of requiredLocaleScenarios) {
      const scenario = input.localization?.[locale]?.[name];
      const valid = object(scenario) && statuses.has(scenario.status) && typeof scenario.observed === 'boolean';
      localization[locale][name] = { status: valid ? scenario.status : 'not_run', observed: valid ? scenario.observed : false };
      if (!valid || scenario.status !== 'passed' || scenario.observed !== true) reject('localization_unverified');
    }
  }
  return { schemaVersion: 1, boundary: 'packaged-native', assessedAt: new Date().toISOString(), platform: expected ? platform : null, artifact: identity, revision, observation: { kind: observation.kind === 'human' ? 'human' : 'unverified', observedAt, observerHash: observerValid ? createHash('sha256').update(observation.observer).digest('hex') : null }, runtime, prerequisites, scenarios, localization, ready: reasons.length === 0, status: reasons.length === 0 ? 'report_validated' : 'unverified', reasons, details: 'omitted_private_payloads', assurance: 'human_report_not_independently_certified', signing: 'not_assessed', notarization: 'not_assessed', publicRelease: 'not_assessed' };
}

if (isEntryPoint(import.meta.url)) {
  try {
    const options = parseArguments(process.argv.slice(2));
    let report;
    if (options.report) {
      try { report = await readJsonBounded(options.report); }
      catch { report = null; }
    }
    const evidence = await assessPreview({ ...options, report });
    await writeEvidence(resolve(options.output), 'preview-evidence.json', evidence);
    process.stdout.write(`${JSON.stringify({ status: evidence.status, ready: evidence.ready, reasons: evidence.reasons })}\n`);
    process.exitCode = evidence.ready ? 0 : 1;
  } catch {
    process.stderr.write('preview_evidence_failed: invalid arguments or evidence unavailable\n');
    process.exitCode = 1;
  }
}
