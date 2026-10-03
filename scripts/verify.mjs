/** Runs fixed local/CI checks and persists only revision facts and allowlisted behavior evidence. */
import { readFile } from 'node:fs/promises';
import { relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { platform, arch, release } from 'node:os';
import { execute, isEntryPoint, parseOptions, sourceFiles, safeIdentifier, writeEvidence } from './evidence.mjs';
import { documentationFailureEvidence, documentationSucceeded } from '../.agents/scripts/check-docs.mjs';

// Keep verification artifacts separate from native development/profile builds.
const cargo = ['--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--target-dir', '.verification/native-target'];
function check(id, boundary, executable, args, reporter = 'none') {
  return { id, boundary, executable, args, invocation: [executable, ...args], reporter };
}
export function selectChecks(suite = 'all', infrastructureFiles = ['tests/infrastructure/policy.test.mjs', 'tests/infrastructure/preview-evidence.test.mjs', 'tests/infrastructure/verify.test.mjs']) {
  if (!['unit', 'integration', 'all'].includes(suite)) throw new Error('invalid_suite');
  const unit = [
    check('frontend_unit', 'unit', 'npm', ['exec', '--', 'vitest', 'run', 'tests/unit', '--reporter=json'], 'vitest'),
    check('native_unit', 'unit', 'cargo', ['test', ...cargo, '--lib', 'unit_tests::'], 'rust'),
    check('infrastructure_unit', 'unit', 'node', ['--test', '--test-reporter=tap', ...infrastructureFiles], 'tap'),
  ];
  const integration = [
    check('frontend_integration', 'integration', 'npm', ['exec', '--', 'vitest', 'run', 'tests/integration', '--reporter=json'], 'vitest'),
    check('native_integration', 'integration', 'cargo', ['test', ...cargo, '--lib', 'integration_tests::'], 'rust'),
    ...['workspace', 'read_only_open', 'observation', 'persistence', 'diagnostics', 'diagnostic_operations'].map(target => check(`native_${target}`, 'integration', 'cargo', ['test', ...cargo, '--test', target], 'rust')),
  ];
  if (suite === 'unit') return unit;
  if (suite === 'integration') return integration;
  return [
    check('frontend_build', 'build', 'npm', ['run', 'build']),
    check('native_build', 'build', 'cargo', ['build', ...cargo]),
    check('agent_documentation', 'documentation', 'node', ['.agents/scripts/check-docs.mjs', '--output', '.verification/doc-examples'], 'documentation'),
    ...unit,
    ...integration,
    check('repository_policy', 'policy', 'node', ['scripts/check-policy.mjs'], 'policy'),
    check('publication_licenses', 'policy', 'node', ['scripts/check-licenses.mjs', '--check']),
  ];
}
export function parseArguments(argv) {
  const options = parseOptions(argv, ['suite', 'output'], { suite: 'all', output: '.verification' });
  if (!['unit', 'integration', 'all'].includes(options.suite)) throw new Error('invalid_suite');
  return options;
}

async function revisionMetadata(root, signal) {
  const revision = await execute('git', ['rev-parse', 'HEAD'], { cwd: root, signal });
  const dirty = await execute('git', ['status', '--porcelain', '--untracked-files=normal'], { cwd: root, signal });
  const rust = await execute('rustc', ['--version'], { cwd: root, signal });
  const git = await execute('git', ['--version'], { cwd: root, signal });
  const hash = revision.output.trim();
  return {
    available: revision.code === 'ok' && /^[a-f0-9]{40,64}$/.test(hash) && dirty.code === 'ok' && rust.code === 'ok' && git.code === 'ok',
    revision: /^[a-f0-9]{40,64}$/.test(hash) ? hash : null,
    dirty: dirty.code === 'ok' ? dirty.output.length > 0 : null,
    rust: rust.output.match(/^rustc ([0-9]+\.[0-9]+\.[0-9]+(?:-[a-z0-9.]+)?)/)?.[1] ?? null,
    git: git.output.match(/^git version ([0-9]+\.[0-9]+\.[0-9]+)/)?.[1] ?? null,
  };
}

async function behaviorIdentifiers(root) {
  const identifiers = new Set();
  const sources = new Map();
  const ts = await import('typescript');
  const files = [...await sourceFiles(root, 'tests'), ...await sourceFiles(root, 'src-tauri/tests')];
  for (const path of files) {
    if (!/\.(?:tsx?|mjs|rs)$/.test(path)) continue;
    const source = await readFile(resolve(root, path), 'utf8');
    sources.set(path, source.split('\n').length);
    if (path.endsWith('.rs')) {
      for (const match of source.matchAll(/\bfn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g)) if (safeIdentifier(match[1])) identifiers.add(match[1]);
      continue;
    }
    const tree = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true);
    function visit(node) {
      if (ts.isCallExpression(node)) {
        const expression = node.expression;
        const name = ts.isIdentifier(expression) ? expression.text : ts.isPropertyAccessExpression(expression) && ts.isIdentifier(expression.expression) ? expression.expression.text : null;
        const title = node.arguments[0];
        if (['test', 'it'].includes(name) && title && ts.isStringLiteralLike(title) && safeIdentifier(title.text)) identifiers.add(title.text);
      }
      ts.forEachChild(node, visit);
    }
    visit(tree);
  }
  return { identifiers, sources };
}

function sourceFailureIdentifier(root, sources, candidate, line) {
  if (typeof candidate !== 'string') return null;
  if (candidate.startsWith('file:')) {
    try { candidate = fileURLToPath(candidate); } catch { return null; }
  }
  for (const directory of [root, resolve(root, 'src-tauri')]) {
    const path = relative(root, resolve(directory, candidate)).replaceAll('\\', '/');
    const lineCount = sources.get(path);
    if (!lineCount) continue;
    if (line !== undefined && (!Number.isSafeInteger(Number(line)) || Number(line) < 1 || Number(line) > lineCount)) continue;
    const identifier = `source:${path.replaceAll('/', ':')}${line === undefined ? '' : `:${Number(line)}`}`;
    if (safeIdentifier(identifier)) return identifier;
  }
  return null;
}

function extractEvidence(output, reporter, allowed, root, sources) {
  const failures = new Set();
  let summary = null;
  let omittedFailures = false;
  let reporterValid = reporter === 'none';
  const retain = name => {
    if (allowed.has(name) && safeIdentifier(name)) failures.add(name);
    else omittedFailures = true;
  };
  const retainSource = (path, line) => {
    const identifier = sourceFailureIdentifier(root, sources, path, line);
    if (identifier) failures.add(identifier);
    return identifier;
  };
  if (['vitest', 'policy', 'documentation'].includes(reporter)) {
    try {
      const data = JSON.parse(output.slice(output.indexOf('{'), output.lastIndexOf('}') + 1));
      if (reporter === 'vitest' && Array.isArray(data.testResults)) {
        reporterValid = true;
        for (const result of data.testResults) for (const assertion of result.assertionResults ?? []) if (assertion.status === 'failed') {
          retain(assertion.title);
          const source = retainSource(result.name);
          if (assertion.location?.line !== undefined) retainSource(result.name, assertion.location.line);
          if (source && Number.isFinite(assertion.duration) && assertion.duration >= 0 && assertion.duration <= 86_400_000) {
            const duration = `${source}:duration_ms:${Math.round(assertion.duration)}`;
            if (safeIdentifier(duration)) failures.add(duration);
          }
          for (const message of Array.isArray(assertion.failureMessages) ? assertion.failureMessages : []) {
            if (typeof message !== 'string') continue;
            if (/(?:^|\n)(?:Error: )?(?:Test|Hook) timed out in \d+ms/.test(message)) failures.add('reported_test_timeout');
            if (/(?:^|\n)AssertionError:/.test(message)) failures.add('reported_assertion_failure');
            for (const match of message.matchAll(/(?:\(|\bat\s+)([^()\r\n]+):(\d+):\d+\)?/g)) retainSource(match[1], match[2]);
          }
        }
        summary = Object.fromEntries(['numPassedTests', 'numFailedTests', 'numPendingTests', 'numTotalTests'].filter(key => Number.isSafeInteger(data[key]) && data[key] >= 0).map(key => [key, data[key]]));
      }
      if (reporter === 'documentation') {
        const failure = documentationFailureEvidence(data);
        reporterValid = documentationSucceeded(data) || failure !== null;
        if (failure) {
          failures.add(failure.error);
          if (failure.stage) failures.add(`documentation:${failure.stage}`);
          if (failure.processCode) failures.add(`process:${failure.processCode}`);
          if (failure.exitCode !== null) failures.add(`process_exit:${failure.exitCode}`);
          for (const code of failure.compilerCodes) failures.add(`compiler:${code}`);
        }
      }
      if (reporter === 'policy' && typeof data.passed === 'boolean' && Array.isArray(data.violations)) {
        reporterValid = true;
        for (const violation of data.violations) if (['test_placement', 'private_artifact', 'contract_dependency', 'ui_dependency', 'platform_dependency', 'feature_dependency', 'native_dependency', 'dynamic_dependency', 'inventory_unavailable', 'source_unavailable'].includes(violation.rule)) failures.add(violation.rule);
      }
    } catch { /* Incomplete reporter evidence must not become a successful check. */ }
  }
  if (reporter === 'rust') {
    reporterValid = /test result: (?:ok|FAILED)\./.test(output);
    for (const match of output.matchAll(/^test ([A-Za-z0-9_:]+) \.\.\. FAILED\s*$/gm)) retain(match[1].split('::').at(-1));
    for (const match of output.matchAll(/panicked at ([^\r\n]+):(\d+):\d+:/g)) retainSource(match[1], match[2]);
    // Enclosing counts, not a self-spawned child's counts, govern required coverage.
    for (const counts of output.matchAll(/^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored/gm)) {
      summary = { passed: Number(counts[1]), failed: Number(counts[2]), ignored: Number(counts[3]) };
    }
  }
  if (reporter === 'tap') {
    reporterValid = /^# tests \d+$/m.test(output);
    for (const match of output.matchAll(/^\s*not ok \d+ - (.+)$/gm)) retain(match[1].trim());
    for (const match of output.matchAll(/^\s+location:\s+['"](.+):(\d+):\d+['"]\r?$/gm)) retainSource(match[1], match[2]);
    const counts = output.match(/^# tests (\d+)[\s\S]*?^# pass (\d+)[\s\S]*?^# fail (\d+)/m);
    if (counts) summary = { total: Number(counts[1]), passed: Number(counts[2]), failed: Number(counts[3]), skipped: Number(output.match(/^# skipped (\d+)$/m)?.[1] ?? 0), todo: Number(output.match(/^# todo (\d+)$/m)?.[1] ?? 0) };
  }
  return { failures: [...failures].sort(), summary, omittedFailures, reporterValid };
}

function testEnvironment() {
  const env = { ...process.env };
  for (const key of Object.keys(env)) if (key.toUpperCase().startsWith('GIT_')) delete env[key];
  const empty = platform() === 'win32' ? 'NUL' : '/dev/null';
  return { ...env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_SYSTEM: empty, GIT_CONFIG_GLOBAL: empty };
}

/** Injection is a test-only module seam; the CLI never accepts executable specifications. */
export async function runVerification({ root = process.cwd(), suite = 'all', output = '.verification', checks, metadata, allowedIdentifiers, signal, outputLimit } = {}) {
  const startedAt = new Date().toISOString();
  const revision = metadata ?? await revisionMetadata(root, signal);
  let inventoryAvailable = true;
  let allowed;
  let sources = new Map();
  try {
    if (allowedIdentifiers !== undefined) allowed = new Set(allowedIdentifiers);
    else {
      const inventory = await behaviorIdentifiers(root);
      allowed = inventory.identifiers;
      sources = inventory.sources;
    }
  }
  catch { allowed = new Set(); inventoryAvailable = false; }
  const infrastructureFiles = (await sourceFiles(root, 'tests/infrastructure')).filter(path => path.endsWith('.test.mjs'));
  const requiredChecks = checks ?? selectChecks(suite, infrastructureFiles);
  // Fixture subprocesses must not inherit runner/user filters, hooks or repository redirects.
  const isolatedTests = testEnvironment();
  const outcomes = [];
  for (const spec of requiredChecks) {
    const base = { id: spec.id, boundary: spec.boundary, required: true, invocation: spec.invocation, rerun: spec.invocation, details: 'omitted_private_payloads' };
    if (signal?.aborted) {
      outcomes.push({ ...base, status: 'not_run', code: 'cancelled', startedAt: null, finishedAt: null, exitCode: null, signal: null, failures: [], summary: null, omittedFailures: true });
      continue;
    }
    const checkStart = new Date().toISOString();
    // npm.cmd cannot be started shell-free on Windows; launch its JS entry through Node.
    const npmCli = process.env.npm_execpath;
    let executable = spec.executable;
    let args = spec.args;
    if (platform() === 'win32' && executable === 'npm') {
      executable = process.execPath;
      const npmPath = npmCli ?? resolve(process.execPath, '..', 'node_modules/npm/bin/npm-cli.js');
      args = [npmPath, ...args];
    }
    const env = ['unit', 'integration'].includes(spec.boundary) ? isolatedTests : undefined;
    const result = await execute(executable, args, { cwd: root, env, signal, outputLimit });
    const evidence = extractEvidence(result.output, spec.reporter, allowed, root, sources);
    const emptySuite = spec.reporter === 'rust' ? evidence.summary?.passed + evidence.summary?.failed === 0 : spec.reporter === 'vitest' ? evidence.summary?.numTotalTests === 0 : spec.reporter === 'tap' ? evidence.summary?.total === 0 : false;
    // TODO scenarios are missing proof too, even when the framework exits successfully.
    const skippedTests = (evidence.summary?.numPendingTests ?? evidence.summary?.ignored ?? evidence.summary?.skipped ?? 0) > 0 || (evidence.summary?.todo ?? 0) > 0;
    const code = result.code !== 'ok' ? result.code : spec.reporter === 'documentation' && evidence.failures.length ? 'check_failed' : !evidence.reporterValid || emptySuite ? 'missing_required_evidence' : skippedTests ? 'required_tests_skipped' : 'ok';
    outcomes.push({ ...base, status: code === 'ok' ? 'passed' : code === 'cancelled' ? 'cancelled' : 'failed', code, startedAt: checkStart, finishedAt: new Date().toISOString(), exitCode: result.exitCode, signal: ['SIGTERM', 'SIGKILL', 'SIGINT', 'SIGHUP', 'SIGABRT', 'SIGSEGV'].includes(result.signal) ? result.signal : result.signal ? 'other' : null, ...evidence });
  }
  const passed = revision.available && inventoryAvailable && outcomes.length > 0 && outcomes.every(item => item.status === 'passed');
  const manifest = { schemaVersion: 1, boundary: 'repository-verification-not-native-preview', suite, startedAt, finishedAt: new Date().toISOString(), revision: revision.revision, dirty: revision.dirty, environment: { os: platform(), architecture: arch(), node: process.versions.node, rust: revision.rust, git: revision.git }, metadataStatus: revision.available ? 'available' : 'unavailable', identifierInventory: inventoryAvailable ? 'available' : 'unavailable', status: passed ? 'passed' : 'failed', checks: outcomes, humanReview: ['correctness', 'privacy_allowlist', 'trace_causality', 'native_observations'], platformExclusions: [{ behavior: 'real_git_non_utf8_filenames', excludedPlatform: 'darwin', reason: 'filesystem_does_not_accept_fixture' }, { behavior: 'unix_process_lifecycle_fixtures', excludedPlatform: 'win32', reason: 'unix_only_fixture' }], nativePreview: 'unverified' };
  manifest.environment.osVersion = release().match(/^[0-9]+(?:\.[0-9]+){1,3}/)?.[0] ?? null;
  await writeEvidence(resolve(root, output), 'manifest.json', manifest);
  return { manifest, exitCode: passed ? 0 : 1 };
}

if (isEntryPoint(import.meta.url)) {
  const controller = new AbortController();
  const cancel = () => controller.abort();
  process.once('SIGINT', cancel);
  process.once('SIGTERM', cancel);
  try {
    const options = parseArguments(process.argv.slice(2));
    const result = await runVerification({ ...options, signal: controller.signal });
    process.stdout.write(`${JSON.stringify({ status: result.manifest.status, checks: result.manifest.checks.map(({ id, status, code, failures, rerun }) => ({ id, status, code, failures, rerun })) })}\n`);
    process.exitCode = result.exitCode;
  } catch {
    process.stderr.write('verification_failed: invalid arguments or evidence unavailable\n');
    process.exitCode = 1;
  } finally {
    process.removeListener('SIGINT', cancel);
    process.removeListener('SIGTERM', cancel);
  }
}
