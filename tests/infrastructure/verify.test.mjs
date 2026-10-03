/** Exercises run outcomes and privacy using isolated child processes. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { access, mkdir, mkdtemp, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { execute } from '../../scripts/evidence.mjs';
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

test('parameterized failures retain only source-owned locations and bounded timing facts', async t => {
  const root = await fixture(t);
  await mkdir(join(root, 'tests/unit'), { recursive: true });
  const source = join(root, 'tests/unit/Scenario.test.ts');
  await writeFile(source, "test.each(['one', 'two'])('keeps %s isolated', () => {\n  throw new Error('fixture');\n});\n");
  const report = { numPassedTests: 0, numFailedTests: 2, numPendingTests: 0, numTotalTests: 2, testResults: [{
    name: source,
    assertionResults: [
      { title: 'keeps private_customer isolated', status: 'failed', location: { line: 2 }, duration: 5001.6,
        failureMessages: [`AssertionError: SECRET_TOKEN\n    at scenario (${pathToFileURL(source).href}:2:5)\n    at outside (/private/customer.ts:1:1)\n    at invalid (${source}:99999:1)`, 'Error: Test timed out in 5000ms.\nprivate DOM bytes'] },
      { title: 'private_variant', status: 'failed', location: { line: 0 }, duration: Number.MAX_SAFE_INTEGER },
    ],
  }] };
  const result = await runVerification({ root, checks: [child('frontend', `console.log(JSON.stringify(${JSON.stringify(report)})); process.exitCode = 1`, 'vitest')], metadata });
  const failures = result.manifest.checks[0].failures;
  assert.equal(result.exitCode, 1);
  assert.equal(result.manifest.checks[0].omittedFailures, true);
  assert.deepEqual(failures, ['reported_assertion_failure', 'reported_test_timeout', 'source:tests:unit:Scenario.test.ts', 'source:tests:unit:Scenario.test.ts:2', 'source:tests:unit:Scenario.test.ts:duration_ms:5002']);
  const persisted = await readFile(join(root, '.verification/manifest.json'), 'utf8');
  assert.doesNotMatch(persisted, /SECRET_TOKEN|private_customer|private_variant|private DOM|99999|9007199254740991/);
  assert.equal(persisted.includes(JSON.stringify(root).slice(1, -1)), false);
});

test('Rust failure locations resolve through module paths without retaining outside paths or payloads', async t => {
  const root = await fixture(t);
  await mkdir(join(root, 'src-tauri/tests/integration'), { recursive: true });
  await writeFile(join(root, 'src-tauri/tests/integration/scenario.rs'), 'fn retains_authority() {\n    panic!("fixture");\n}\n');
  const output = "test module::retains_authority ... FAILED\nthread 'case' panicked at src/domain/../../tests/integration/scenario.rs:2:5:\nSECRET_TOKEN\nthread 'private' panicked at /private/customer.rs:2:1:\nprivate payload\ntest result: FAILED. 0 passed; 1 failed; 0 ignored;\n";
  const result = await runVerification({ root, checks: [child('native', `console.log(${JSON.stringify(output)}); process.exitCode = 1`, 'rust')], metadata });
  assert.deepEqual(result.manifest.checks[0].failures, ['retains_authority', 'source:src-tauri:tests:integration:scenario.rs:2']);
  assert.doesNotMatch(await readFile(join(root, '.verification/manifest.json'), 'utf8'), /SECRET_TOKEN|customer\.rs|private payload/);
});

test('documentation failure evidence cannot certify success or publish unknown process data', async t => {
  const root = await fixture(t);
  const failure = { error: 'doc_examples_failed', stage: 'driver_run', processCode: 'check_failed', exitCode: 3221225785 };
  const privateFailure = { ...failure, stage: 'private_customer', output: 'SECRET_TOKEN' };
  const success = { status: 'passed', documents: 1, links: 1, skills: 1, examples: { rust: 2, sql: 3, reader: 'passed', cli: 'passed', privacy: 'passed' } };
  const checks = [child('startup', `console.log(JSON.stringify(${JSON.stringify(failure)}))`, 'documentation'), child('private', `console.log(JSON.stringify(${JSON.stringify(privateFailure)}))`, 'documentation')];
  checks.push(child('contradictory', `console.log(JSON.stringify(${JSON.stringify({ ...success, ...failure })}))`, 'documentation'));
  checks.push(child('complete', `console.log(JSON.stringify(${JSON.stringify(success)}))`, 'documentation'));
  const result = await runVerification({ root, checks, metadata, allowedIdentifiers: [] });
  assert.equal(result.manifest.checks[0].code, 'check_failed');
  assert.deepEqual(result.manifest.checks[0].failures, ['doc_examples_failed', 'documentation:driver_run', 'process:check_failed', 'process_exit:3221225785']);
  assert.equal(result.manifest.checks[1].code, 'missing_required_evidence');
  assert.equal(result.manifest.checks[2].code, 'missing_required_evidence');
  assert.equal(result.manifest.checks[3].status, 'passed');
  assert.doesNotMatch(await readFile(join(root, '.verification/manifest.json'), 'utf8'), /private_customer|SECRET_TOKEN/);
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

test('verification fixtures ignore ambient Git filters without changing the invoking environment', async t => {
  const root = await fixture(t);
  const marker = join(root, 'filter-ran');
  const filter = join(root, 'filter.cjs');
  await writeFile(filter, `require('node:fs').writeFileSync(${JSON.stringify(marker)}, 'ran'); process.stdout.write('altered contents');`);
  const command = `"${process.execPath.replaceAll('\\', '/')}" "${filter.replaceAll('\\', '/')}"`;
  const config = join(root, 'global.gitconfig');
  const configuration = `[filter "fixture"]\nclean = ${JSON.stringify(command)}\n`;
  await writeFile(config, configuration);
  const probe = join(root, 'probe.cjs');
  await writeFile(probe, `
    const {execFileSync} = require('node:child_process');
    const {writeFileSync} = require('node:fs');
    execFileSync('git', ['init', '--quiet']);
    writeFileSync('.gitattributes', '*.txt filter=fixture\\n');
    const options = {input: 'original contents', encoding: 'utf8'};
    const expected = execFileSync('git', ['hash-object', '--no-filters', '--stdin'], options);
    const actual = execFileSync('git', ['hash-object', '--path=example.txt', '--stdin'], options);
    if (actual !== expected) process.exit(1);
  `);
  const driver = join(root, 'driver.mjs');
  await writeFile(driver, `
    import {execFileSync} from 'node:child_process';
    import {runVerification} from ${JSON.stringify(pathToFileURL(resolve('scripts/verify.mjs')).href)};
    const checks = [{id:'git_fixture', boundary:'integration', executable:process.execPath,
      args:[${JSON.stringify(probe)}], invocation:['node','probe.cjs'], reporter:'none'}];
    const result = await runVerification({root:process.cwd(), checks, metadata:${JSON.stringify(metadata)}, allowedIdentifiers:[]});
    const inherited = execFileSync('git', ['config', '--global', '--get', 'filter.fixture.clean'], {encoding:'utf8'}).trim();
    if (inherited !== ${JSON.stringify(command)}) process.exit(2);
    process.exitCode = result.exitCode;
  `);
  const env = { ...process.env };
  for (const key of Object.keys(env)) if (key.toUpperCase().startsWith('GIT_')) delete env[key];
  env.GIT_CONFIG_NOSYSTEM = '1';
  env.GIT_CONFIG_GLOBAL = config;
  // Git also accepts these mixed-case command-scope overrides on Windows.
  env.git_config_count = '1';
  env.git_config_key_0 = 'filter.fixture.clean';
  env.Git_Config_Value_0 = command;
  const result = await execute(process.execPath, [driver], { cwd: root, env });
  assert.equal(result.exitCode, 0);
  await assert.rejects(access(marker), { code: 'ENOENT' });
  assert.equal(await readFile(config, 'utf8'), configuration);
});


test('CLI rejects unknown, duplicate, missing and invalid options', () => {
  assert.throws(() => parseArguments(['--suite', 'unknown']));
  assert.throws(() => parseArguments(['--output']));
  assert.throws(() => parseArguments(['--suite', 'unit', '--suite', 'all']));
  assert.throws(() => parseArguments(['--command', 'unsafe']));
  assert.deepEqual(parseArguments(['--suite', 'integration', '--output', 'evidence']), { suite: 'integration', output: 'evidence' });
});
