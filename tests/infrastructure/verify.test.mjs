/** Exercises run outcomes and privacy using isolated child processes. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { access, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { runVerification, parseArguments } from '../../scripts/verify.mjs';

const metadata = { revision: 'a'.repeat(40), dirty: false, rust: '1.90.0', git: '2.50.0', available: true };
function child(id, code, reporter = 'none') {
  return { id, boundary: 'unit', executable: process.execPath, args: ['-e', code], invocation: ['node', 'fixture.mjs'], reporter };
}
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'gitview-verification-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

test('named failure remains actionable without leaking private stdout or environment', async t => {
  const root = await fixture(t);
  const checks = [child('first', "console.log('test fixture::preserves_selection ... FAILED'); console.error('<html>/private/customer SECRET_TOKEN</html>'); process.exit(1)", 'rust'), child('second', 'process.exit(0)')];
  const result = await runVerification({ root, output: join(root, 'evidence'), checks, metadata, allowedIdentifiers: ['preserves_selection'] });
  const persisted = await readFile(join(root, 'evidence', 'manifest.json'), 'utf8');
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[1].status, 'passed');
  assert.deepEqual(result.manifest.checks[0].failures, ['preserves_selection']);
  assert.deepEqual(result.manifest.checks[0].rerun, ['node', 'fixture.mjs']);
  assert.equal(result.manifest.checks[0].details, 'omitted_private_payloads');
  assert.doesNotMatch(persisted, /SECRET_TOKEN|private\/customer|<html>|console\.log/);
});

test('start failure is failed and does not prevent other required checks', async t => {
  const root = await fixture(t);
  const missing = { ...child('missing', ''), executable: join(root, 'not-an-executable') };
  const result = await runVerification({ root, output: join(root, 'out'), checks: [missing, child('next', 'process.exit(0)')], metadata, allowedIdentifiers: [] });
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].code, 'start_failed');
  assert.equal(result.manifest.checks[0].status, 'failed');
  assert.equal(result.manifest.checks[1].status, 'passed');
});

test('cancellation stops the running child and leaves remaining required checks not run', async t => {
  const root = await fixture(t);
  const controller = new AbortController();
  t.after(() => controller.abort());
  const marker = join(root, 'started');
  const startChild = `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'started'); setTimeout(() => {}, 60000)`;
  const running = runVerification({ root, output: join(root, 'out'), checks: [child('slow', startChild), child('next', 'process.exit(0)')], metadata, allowedIdentifiers: [], signal: controller.signal });
  const deadline = Date.now() + 5000;
  while (true) {
    try { await access(marker); break; }
    catch {
      if (Date.now() >= deadline) { controller.abort(); await running; assert.fail('fixture child did not start'); }
      await delay(10);
    }
  }
  controller.abort();
  const result = await running;
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].status, 'cancelled');
  assert.equal(result.manifest.checks[1].status, 'not_run');
});

test('bounded output cannot hide incomplete evidence behind a passing exit', async t => {
  const root = await fixture(t);
  const result = await runVerification({ root, output: join(root, 'out'), checks: [child('large', "console.log('x'.repeat(10000))")], metadata, outputLimit: 128 });
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].code, 'output_limit');
});

test('missing revision metadata cannot summarize otherwise successful required checks as verified', async t => {
  const root = await fixture(t);
  const result = await runVerification({ root, output: join(root, 'out'), checks: [child('success', 'process.exit(0)')], metadata: { ...metadata, available: false, revision: null }, allowedIdentifiers: [] });
  assert.equal(result.manifest.checks[0].status, 'passed');
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.metadataStatus, 'unavailable');
});

test('signal termination is failed rather than skipped even without an exit code', async t => {
  const root = await fixture(t);
  const result = await runVerification({ root, output: join(root, 'out'), checks: [child('terminated', "process.kill(process.pid, 'SIGTERM')")], metadata, allowedIdentifiers: [] });
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].status, 'failed');
  assert.ok((process.platform === 'win32' ? ['signal_termination', 'check_failed'] : ['signal_termination']).includes(result.manifest.checks[0].code));
});

test('unrecognized output identifiers and incomplete success evidence do not establish passed behavior', async t => {
  const root = await fixture(t);
  const result = await runVerification({ root, output: join(root, 'out'), checks: [child('private', "console.log('test private_customer ... FAILED'); process.exitCode = 1", 'rust'), child('empty', 'process.exit(0)', 'vitest')], metadata, allowedIdentifiers: ['preserves_selection'] });
  assert.deepEqual(result.manifest.checks[0].failures, []);
  assert.equal(result.manifest.checks[0].omittedFailures, true);
  assert.equal(result.manifest.checks[1].code, 'missing_required_evidence');
  assert.doesNotMatch(JSON.stringify(result.manifest), /private_customer/);
});

test('framework skipped tests cannot claim required behavior coverage', async t => {
  const root = await fixture(t);
  const result = await runVerification({ root, output: join(root, 'out'), checks: [child('ignored', "console.log('test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;')", 'rust')], metadata, allowedIdentifiers: [] });
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].code, 'required_tests_skipped');
});

test('real Node TODO scenarios cannot certify required infrastructure behavior', async t => {
  const root = await fixture(t);
  await writeFile(join(root, 'scenario.mjs'), "import test from 'node:test'; test('completed scenario', () => {}); test.todo('required scenario');\n");
  // Nested Node tests otherwise inherit the parent runner's private child protocol instead of TAP.
  await writeFile(join(root, 'launch.mjs'), "import {spawnSync} from 'node:child_process'; const env={...process.env}; delete env.NODE_TEST_CONTEXT; const result=spawnSync(process.execPath,['--test','--test-reporter=tap','scenario.mjs'],{env,encoding:'utf8'}); process.stdout.write(result.stdout); process.stderr.write(result.stderr); process.exit(result.status);\n");
  const checks = [{ id: 'pending_infrastructure', boundary: 'unit', executable: process.execPath, args: ['launch.mjs'], invocation: ['node', 'launch.mjs'], reporter: 'tap' }];
  const result = await runVerification({ root, output: join(root, 'out'), checks, metadata, allowedIdentifiers: ['required scenario'] });
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].code, 'required_tests_skipped');
  assert.equal(result.manifest.checks[0].summary.todo, 1);
});


test('CLI rejects unknown, duplicate, missing and invalid options', () => {
  assert.throws(() => parseArguments(['--suite', 'unknown']));
  assert.throws(() => parseArguments(['--output']));
  assert.throws(() => parseArguments(['--suite', 'unit', '--suite', 'all']));
  assert.throws(() => parseArguments(['--command', 'unsafe']));
  assert.deepEqual(parseArguments(['--suite', 'integration', '--output', 'evidence']), { suite: 'integration', output: 'evidence' });
});
