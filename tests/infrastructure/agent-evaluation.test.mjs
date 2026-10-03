/** Deterministic grader regression tests, not model runs or simulated agent evaluations. */
import assert from 'node:assert/strict';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import test, { before, after } from 'node:test';
import { diagnosticFacts, gradeWorkflow, prepareWorkflow, validateDiagnosis, validateReporterCapture } from '../../.agents/scripts/evaluate-workflow.mjs';

const root = resolve('.');
let directory;
let run;
let challenge;
let facts;
let snippets;

before(async () => {
  await mkdir(join(root, '.verification'), { recursive: true });
  directory = await mkdtemp(join(root, '.verification', 'grader-'));
  run = join(directory, 'run');
  await prepareWorkflow({ root, output: run });
  challenge = JSON.parse(await readFile(join(run, 'challenge.json'), 'utf8'));
  facts = await diagnosticFacts(join(run, challenge.database), { root });
  const skill = await readFile(join(root, '.agents/skills/gitview-sql-diagnostics/SKILL.md'), 'utf8');
  const rust = [...skill.matchAll(/```rust\n([\s\S]*?)```/g)].map(match => match[1]);
  snippets = [rust.find(source => source.includes('fn record_save_failure(')), rust.find(source => source.includes('fn spawn_scan<'))];
  assert.ok(snippets.every(source => typeof source === 'string'));
});
after(async () => { if (directory) await rm(directory, { recursive: true, force: true }); });

async function submit(candidates = snippets, answerOverrides = {}) {
  await writeFile(join(run, 'record-save-failure.rs'), candidates[0]);
  await writeFile(join(run, 'spawn-scan.rs'), candidates[1]);
  const answer = { schemaVersion: 1, runId: challenge.runId, revision: challenge.revision,
    scenarios: [{ id: 'diagnose-failure', facts }, { id: 'instrument-correlated-failure' },
      { id: 'reject-invalid-evidence', capture: { exitCode: 2, response: { error: 'invalid_verification_report' } } }], ...answerOverrides };
  await writeFile(join(run, 'answer.json'), JSON.stringify(answer));
  return gradeWorkflow({ root, run });
}

async function isolatedBindingFixture(context) {
  const fixture = await mkdtemp(join(directory, 'binding-'));
  const fixtureRoot = join(fixture, 'repository');
  await mkdir(fixtureRoot);
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: join(fixture, 'empty-config') };
  for (const key of Object.keys(env)) if (key.startsWith('GIT_') && !['GIT_CONFIG_NOSYSTEM', 'GIT_CONFIG_GLOBAL'].includes(key)) delete env[key];
  const git = args => {
    const result = spawnSync('git', args, { cwd: fixtureRoot, env, encoding: 'utf8' });
    assert.equal(result.status, 0);
    return result.stdout.trim();
  };
  git(['init', '--quiet']);
  git(['config', 'user.name', 'Binding fixture']);
  git(['config', 'user.email', 'fixture@example.invalid']);
  await writeFile(join(fixtureRoot, 'baseline'), 'isolated binding');
  git(['add', 'baseline']);
  git(['commit', '--quiet', '-m', 'isolated baseline']);
  for (const path of ['scripts', '.agents', 'src-tauri/src', 'src-tauri/capabilities', 'src-tauri/icons', 'src-tauri/permissions', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'src-tauri/build.rs', 'src-tauri/tauri.conf.json']) {
    await mkdir(join(fixtureRoot, path, '..'), { recursive: true });
    await cp(join(root, path), join(fixtureRoot, path), { recursive: true });
  }
  const fixtureRun = join(fixtureRoot, '.verification', 'run');
  await cp(run, fixtureRun, { recursive: true });
  await writeFile(join(fixtureRun, 'challenge.json'), JSON.stringify({ ...challenge, revision: git(['rev-parse', 'HEAD']) }));
  await writeFile(join(fixtureRun, 'answer.json'), '{}');
  context.after(() => rm(fixture, { recursive: true, force: true }));
  return { fixtureRoot, fixtureRun };
}

function instrumentation(result) {
  return result.scenarios.find(scenario => scenario.id === 'instrument-correlated-failure');
}

test('queried diagnosis rejects a wrong causal parent and arbitrary private explanation', () => {
  assert.throws(() => validateDiagnosis({ ...facts, childParentOperationId: facts.childOperationId }, facts), /facts_mismatch/);
  assert.throws(() => validateDiagnosis({ ...facts, message: 'private-path-payload' }, facts), /facts_mismatch/);
});

test('captured reporter response rejects private fields and misleading readiness', () => {
  const expected = { exitCode: 2, response: { error: 'invalid_verification_report' } };
  assert.throws(() => validateReporterCapture({ exitCode: 0, response: { ready: true } }, expected), /invalid_evidence_response/);
  assert.throws(() => validateReporterCapture({ exitCode: 2, response: { error: 'invalid_verification_report', data: 'private-token' } }, expected), /invalid_evidence_response/);
});

test('a submission from another run never certifies even otherwise valid answers', async () => {
  const result = await submit(snippets, { runId: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa' });
  assert.equal(result.status, 'failed');
  assert.deepEqual(result.scenarios.map(scenario => scenario.code), ['invalid_answer', 'invalid_answer', 'invalid_answer']);
});

test('malformed answer bytes fail every scenario without returning the untrusted input', async () => {
  await writeFile(join(run, 'answer.json'), '{"private-token":');
  const result = await gradeWorkflow({ root, run });
  assert.equal(result.status, 'failed');
  assert.deepEqual(result.scenarios.map(scenario => scenario.code), ['invalid_answer', 'invalid_answer', 'invalid_answer']);
  assert.equal(JSON.stringify(result).includes('private-token'), false);
});

test('real SQLite and real reporter grade valid deterministic fixture submissions', async () => {
  const result = await submit();
  assert.equal(result.status, 'passed');
  assert.deepEqual(result.scenarios.map(scenario => scenario.outcome), ['passed', 'passed', 'passed']);
  assert.equal(result.boundary, 'workflow-evaluation-not-release-readiness');
});

test('compiled candidate that omits the actual persistence failure is rejected', async () => {
  const baseline = await readFile(join(root, '.agents/evaluations/record-save-failure.rs'), 'utf8');
  const result = await submit([baseline, snippets[1]]);
  assert.equal(result.status, 'failed');
  assert.equal(instrumentation(result).code, 'candidate_rejected');
});

test('compiled child work without scoped parent correlation is rejected', async () => {
  const baseline = await readFile(join(root, '.agents/evaluations/spawn-scan.rs'), 'utf8');
  const result = await submit([snippets[0], baseline]);
  assert.equal(instrumentation(result).outcome, 'failed');
  assert.equal(instrumentation(result).code, 'candidate_rejected');
});

test('arbitrary payload metadata is rejected by the real typed contract compiler without publication', async () => {
  const privateCandidate = snippets[0].replace('DiagnosticDetails {', 'DiagnosticDetails { message: "private-token", data: "private-path",');
  const result = await submit([privateCandidate, snippets[1]]);
  assert.equal(instrumentation(result).code, 'candidate_rejected');
  const persistedGrade = await readFile(join(run, 'grade.json'), 'utf8');
  assert.equal(persistedGrade.includes('private-token'), false);
  assert.equal(persistedGrade.includes('private-path'), false);
});

test('changing native build-time code invalidates prepared evidence before candidate execution', async context => {
  const { fixtureRoot, fixtureRun } = await isolatedBindingFixture(context);
  const baseline = await gradeWorkflow({ root: fixtureRoot, run: fixtureRun });
  assert.deepEqual(baseline.scenarios.map(scenario => scenario.code), ['invalid_answer', 'invalid_answer', 'invalid_answer']);
  const build = join(fixtureRoot, 'src-tauri/build.rs');
  await writeFile(build, `${await readFile(build, 'utf8')}\n// Changed isolated build input.\n`);
  await assert.rejects(gradeWorkflow({ root: fixtureRoot, run: fixtureRun }), { message: 'stale_run' });
});
