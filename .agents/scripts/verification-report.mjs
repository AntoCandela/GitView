/** Summarizes recorded verification without echoing private payloads or trusting supplied commands. */
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isEntryPoint, parseOptions, readJsonBounded, sourceFiles } from '../../scripts/evidence.mjs';
import { selectChecks } from '../../scripts/verify.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const statuses = new Set(['passed', 'failed', 'cancelled', 'not_run']);
const codes = new Set(['ok', 'cancelled', 'start_failed', 'signal_termination', 'output_limit', 'check_failed', 'missing_required_evidence', 'required_tests_skipped']);
const countKeys = ['total', 'passed', 'failed', 'skipped', 'todo', 'ignored', 'numPassedTests', 'numFailedTests', 'numPendingTests', 'numTotalTests'];
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const date = value => typeof value === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(value) && Number.isFinite(Date.parse(value));
const available = value => value === 'available' || value === 'unavailable';

/** A valid record is not proof that its revision or dirty worktree is still current. */
export async function summarizeVerification(manifest) {
  if (!object(manifest) || manifest.schemaVersion !== 1 || manifest.boundary !== 'repository-verification-not-native-preview'
      || !['all', 'unit', 'integration'].includes(manifest.suite) || !['passed', 'failed'].includes(manifest.status)
      || !date(manifest.startedAt) || !date(manifest.finishedAt) || manifest.finishedAt < manifest.startedAt
      || !available(manifest.metadataStatus) || !available(manifest.identifierInventory)
      || !(manifest.revision === null || typeof manifest.revision === 'string' && /^[a-f0-9]{40,64}$/.test(manifest.revision))
      || !(manifest.dirty === null || typeof manifest.dirty === 'boolean') || !Array.isArray(manifest.checks)) {
    throw new Error('invalid_verification_report');
  }
  const infrastructure = (await sourceFiles(root, 'tests/infrastructure')).filter(path => path.endsWith('.test.mjs'));
  const specs = selectChecks(manifest.suite, infrastructure);
  const expected = new Map(specs.map(spec => [spec.id, spec]));
  if (manifest.checks.length !== expected.size) throw new Error('invalid_verification_report');
  const checks = [];
  for (const check of manifest.checks) {
    const spec = object(check) && expected.get(check.id);
    if (!spec || check.required !== true || !statuses.has(check.status) || !codes.has(check.code)
        || (check.status === 'passed') !== (check.code === 'ok')) throw new Error('invalid_verification_report');
    expected.delete(check.id);
    let summary = null;
    if (check.summary !== null) {
      if (!object(check.summary)) throw new Error('invalid_verification_report');
      summary = {};
      for (const key of countKeys) {
        if (!Object.hasOwn(check.summary, key)) continue;
        const count = check.summary[key];
        if (!Number.isSafeInteger(count) || count < 0) throw new Error('invalid_verification_report');
        summary[key] = count;
      }
    }
    if (check.status === 'passed' && spec.reporter !== 'none' && spec.reporter !== 'policy' && spec.reporter !== 'documentation') {
      const required = spec.reporter === 'rust' ? ['passed', 'failed', 'ignored']
        : spec.reporter === 'vitest' ? ['numPassedTests', 'numFailedTests', 'numPendingTests', 'numTotalTests']
        : ['total', 'passed', 'failed', 'skipped', 'todo'];
      if (!summary || required.some(key => !Object.hasOwn(summary, key))
          || !(spec.reporter === 'rust' ? summary.passed > 0 : spec.reporter === 'vitest' ? summary.numTotalTests > 0 : summary.total > 0)
          || ['failed', 'skipped', 'todo', 'ignored', 'numFailedTests', 'numPendingTests'].some(key => (summary[key] ?? 0) > 0)) {
        throw new Error('invalid_verification_report');
      }
    }
    // Regenerate commands from this checkout; a supplied manifest is data, not executable authority.
    checks.push({ id: spec.id, status: check.status, code: check.code, summary, rerun: spec.invocation });
  }
  if (manifest.status === 'passed' && (manifest.metadataStatus !== 'available' || manifest.identifierInventory !== 'available'
      || manifest.revision === null || manifest.dirty === null || checks.some(check => check.status !== 'passed'))) {
    throw new Error('invalid_verification_report');
  }
  return { boundary: manifest.boundary, suite: manifest.suite, recordedStatus: manifest.status,
    startedAt: manifest.startedAt, finishedAt: manifest.finishedAt, revision: manifest.revision, dirty: manifest.dirty,
    metadataStatus: manifest.metadataStatus, identifierInventory: manifest.identifierInventory,
    freshness: 'not_assessed', details: 'private_payloads_omitted', checks };
}

if (isEntryPoint(import.meta.url)) {
  try {
    const options = parseOptions(process.argv.slice(2), ['manifest']);
    if (!options.manifest) throw new Error('invalid_arguments');
    const report = await summarizeVerification(await readJsonBounded(resolve(options.manifest)));
    process.stdout.write(`${JSON.stringify(report)}\n`);
    process.exitCode = report.recordedStatus === 'passed' ? 0 : 1;
  } catch {
    process.stderr.write('{"error":"invalid_verification_report"}\n');
    process.exitCode = 2;
  }
}
