/** Keeps failed measurements, incompatible runs and private payloads out of passing evidence. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { assessPerformance, comparePerformance } from '../../scripts/performance-evidence.mjs';

function report() {
  return {
    schemaVersion: 1,
    runId: '00000000-0000-4000-8000-000000000001',
    observedAt: '2026-10-04T00:00:00.000Z',
    scenario: { id: 'F-01', version: 1, stories: [{ key: 'US-4', version: 1 }], eligibility: 'implemented' },
    source: { revision: 'a'.repeat(40), dirty: false, artifactHash: 'b'.repeat(64), build: 'optimized' },
    fixture: { generatorHash: 'c'.repeat(64), seed: 1, files: 1000, commits: 1000, tags: 500, variant: 'few_changes' },
    runtime: { platform: 'macos-arm64', osVersion: '15.0', engine: 'Chromium', engineVersion: '140.0.0', gitVersion: '2.50.0', harnessVersion: '24.21.0', hardwareHash: 'd'.repeat(64), cpuCount: 8, memoryBytes: 17179869184, power: 'ac', viewportWidth: 1440, viewportHeight: 900, scale: 2, refreshHz: 60 },
    protocol: { boundary: 'browser-real-service', process: 'warm', cache: 'uncontrolled', profile: 'fresh', clock: 'single-origin-monotonic', presentation: 'dom-raf-proxy', collectorHash: 'e'.repeat(64), warmups: 0, attempts: 1, independent: true, budget: 'provisional' },
    samples: [{ index: 1, phase: 'measurement', behavior: 'pass', timing: 'measured', threshold: 'not_evaluated', reason: 'none', elapsedMs: 40, firstUsefulMs: 20, completedMs: 30, styledMs: 35, checks: { exactResult: 'pass', readOnly: 'pass', continuity: 'pass' }, metrics: [{ name: 'rss_bytes', scope: 'host', collector: 'native_sampler', atMs: 40, value: null, reason: 'unsupported' }] }],
    cleanup: { processes: 'pass', fixtures: 'pass', appState: 'pass' },
  };
}

test('measured failure remains a valid incomplete report with its elapsed duration', () => {
  const input = report();
  input.samples[0] = { ...input.samples[0], behavior: 'fail', reason: 'timeout', firstUsefulMs: null, completedMs: null, styledMs: null, checks: { exactResult: 'not_run', readOnly: 'pass', continuity: 'not_run' } };
  const result = assessPerformance(input);
  assert.equal(result.status, 'incomplete');
  assert.equal(result.summary.outcomes.fail, 1);
  assert.equal(result.summary.completed.count, 0);
  assert.equal(result.report.samples[0].elapsedMs, 40);
});

test('missing attempts cannot disappear from a passing sample total', () => {
  const input = report();
  input.protocol.attempts = 2;
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('unavailable collectors cannot claim zero resource usage', () => {
  const input = report();
  input.samples[0].metrics[0].value = 0;
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('unknown nested payloads are rejected rather than persisted or echoed', () => {
  const input = report();
  input.samples[0].metrics[0].path = '/private/SECRET';
  const result = assessPerformance(input);
  assert.equal(result.status, 'invalid');
  assert.equal(result.report, null);
  assert.doesNotMatch(JSON.stringify(result), /SECRET|private/);
});

test('free-form strings cannot replace allowlisted runtime values', () => {
  const input = report();
  input.runtime.engineVersion = '140.0 /private/SECRET';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('provisional budgets cannot claim a threshold verdict', () => {
  const input = report();
  input.samples[0].threshold = 'met';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('completed content cannot precede the first useful result or the final elapsed time', () => {
  const input = report();
  input.samples[0].completedMs = 10;
  assert.equal(assessPerformance(input).status, 'invalid');
  input.samples[0].completedMs = 50;
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('unavailable timing preserves behavioral success without inventing latency', () => {
  const input = report();
  Object.assign(input.samples[0], { timing: 'unavailable', reason: 'collector_unavailable', elapsedMs: null, firstUsefulMs: null, completedMs: null, styledMs: null });
  const result = assessPerformance(input);
  assert.equal(result.status, 'incomplete');
  assert.equal(result.summary.outcomes.pass, 1);
  assert.equal(result.summary.completed.medianMs, null);
});

test('cleanup failure prevents a successful assessment', () => {
  const input = report();
  input.cleanup.processes = 'fail';
  assert.equal(assessPerformance(input).status, 'incomplete');
});

test('a proposed feature cannot report a successful journey', () => {
  const input = report();
  input.scenario.eligibility = 'proposed';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('native claims cannot use Chromium or a DOM scheduling proxy', () => {
  const input = report();
  input.protocol.boundary = 'packaged-native';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('failed correctness cannot be hidden behind fast completed content', () => {
  const input = report();
  input.samples[0].checks.exactResult = 'fail';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('warmups are retained but excluded from the measured distribution', () => {
  const input = report();
  input.protocol.warmups = 1;
  input.samples.unshift({ ...structuredClone(input.samples[0]), phase: 'warmup', elapsedMs: 500, firstUsefulMs: 400, completedMs: 450, styledMs: null });
  input.samples[1].index = 2;
  const result = assessPerformance(input);
  assert.equal(result.summary.completed.medianMs, 30);
  assert.equal(result.summary.completed.count, 1);
  assert.equal(result.report.samples.length, 2);
});

test('small samples expose uncertainty rather than publishing a p95 claim', () => {
  const result = assessPerformance(report());
  assert.equal(result.status, 'recorded');
  assert.equal(result.summary.completed.medianMs, 30);
  assert.equal(result.summary.completed.p95Ms, null);
  assert.equal(result.summary.completed.tailReason, 'insufficient_samples');
});

test('like-for-like comparisons permit changed artifacts but retain identity and raw outcomes', () => {
  const baseline = report();
  const candidate = report();
  candidate.runId = '00000000-0000-4000-8000-000000000002';
  candidate.source.revision = 'f'.repeat(40);
  candidate.source.artifactHash = 'f'.repeat(64);
  candidate.samples[0].completedMs = 25;
  const result = comparePerformance(baseline, candidate);
  assert.equal(result.status, 'comparable');
  assert.equal(result.delta.completedMedianMs, -5);
  assert.equal(result.threshold, 'not_evaluated');
});

test('different fixtures cannot produce a performance delta', () => {
  const baseline = report();
  const candidate = report();
  candidate.fixture.files = 10000;
  const result = comparePerformance(baseline, candidate);
  assert.equal(result.status, 'incomparable');
  assert.equal(result.delta, null);
  assert.deepEqual(result.reasons, ['fixture_mismatch']);
});

test('profiling and ordinary optimized builds cannot be compared', () => {
  const baseline = report();
  const candidate = report();
  candidate.source.build = 'profiling';
  assert.equal(comparePerformance(baseline, candidate).status, 'incomparable');
});

test('changed warmup policy or hardware prevents comparison', () => {
  const baseline = report();
  const candidate = report();
  candidate.runtime.hardwareHash = 'f'.repeat(64);
  assert.deepEqual(comparePerformance(baseline, candidate).reasons, ['runtime_mismatch']);
  candidate.runtime.hardwareHash = baseline.runtime.hardwareHash;
  candidate.protocol.cache = 'verified_cold';
  assert.deepEqual(comparePerformance(baseline, candidate).reasons, ['protocol_mismatch']);
});

test('a failure alongside successful samples prevents a passing comparison', () => {
  const baseline = report();
  const candidate = report();
  baseline.protocol.attempts = candidate.protocol.attempts = 2;
  baseline.samples.push({ ...structuredClone(baseline.samples[0]), index: 2 });
  candidate.samples.push({ ...structuredClone(candidate.samples[0]), index: 2, behavior: 'fail', reason: 'incorrect_result', checks: { exactResult: 'fail', readOnly: 'pass', continuity: 'pass' } });
  const result = comparePerformance(baseline, candidate);
  assert.equal(result.status, 'incomplete');
  assert.equal(result.candidate.summary.outcomes.fail, 1);
  assert.equal(result.delta, null);
});

test('a missing warmup slot cannot become recorded evidence', () => {
  const input = report();
  input.protocol.warmups = 1;
  input.samples[1] = { ...input.samples[0], index: 2 };
  delete input.samples[0];
  assert.equal(assessPerformance(input).status, 'invalid');
  assert.equal(comparePerformance(input, input).status, 'incomparable');
});

test('a missing measurement slot returns invalid evidence instead of throwing', () => {
  const input = report();
  delete input.samples[0];
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('the returned snapshot cannot change after its values were validated', () => {
  const input = report();
  let reads = 0;
  Object.defineProperty(input.fixture, 'variant', { enumerable: true, get: () => ++reads <= 2 ? 'few_changes' : '/private/SECRET' });
  const result = assessPerformance(input);
  assert.equal(result.status, 'recorded');
  assert.equal(result.report.fixture.variant, 'few_changes');
  assert.doesNotMatch(JSON.stringify(result), /SECRET/);
});

test('metric arrays reject private extra properties', () => {
  const input = report();
  input.samples[0].metrics.privatePath = '/private/SECRET';
  const result = assessPerformance(input);
  assert.equal(result.status, 'invalid');
  assert.equal(result.report, null);
});

test('sample arrays reject private extra properties', () => {
  const input = report();
  input.samples.privatePath = '/private/SECRET';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('story arrays reject private extra properties', () => {
  const input = report();
  input.scenario.stories.privatePath = '/private/SECRET';
  assert.equal(assessPerformance(input).status, 'invalid');
});

test('an unclonable caller payload stays inside the safe invalid-result boundary', () => {
  const input = report();
  input.samples[0].metrics.callback = () => 'SECRET';
  const result = assessPerformance(input);
  assert.equal(result.status, 'invalid');
  assert.doesNotMatch(JSON.stringify(result), /SECRET|callback/);
});

function repeatedReport(count) {
  const input = report();
  input.protocol.attempts = count;
  input.samples = Array.from({ length: count }, (_, index) => ({
    ...structuredClone(input.samples[0]), index: index + 1,
    elapsedMs: 100, firstUsefulMs: 0, completedMs: index + 1, styledMs: null,
  }));
  return input;
}

test('independent tails use nearest-rank p95 only once enough samples exist', () => {
  assert.equal(assessPerformance(repeatedReport(49)).summary.completed.p95Ms, null);
  const result = assessPerformance(repeatedReport(50));
  assert.equal(result.summary.completed.medianMs, 25.5);
  assert.equal(result.summary.completed.p95Ms, 48);
  assert.equal(result.summary.completed.maxMs, 50);
  assert.equal(result.summary.styled.count, 0);
});

test('correlated observations never gain a tail claim from sample count alone', () => {
  const input = repeatedReport(100);
  input.protocol.independent = false;
  assert.equal(assessPerformance(input).summary.completed.p95Ms, null);
  assert.equal(assessPerformance(input).summary.completed.tailReason, 'correlated_samples');
});
