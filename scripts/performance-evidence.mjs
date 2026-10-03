/** Validates bounded journey measurements; supplied observations are not independent native certification. */
import { isDeepStrictEqual } from 'node:util';
import { resolve } from 'node:path';
import { isEntryPoint, OUTPUT_LIMIT, parseOptions, readJsonBounded, writeEvidence } from './evidence.mjs';

const hash = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const version = value => typeof value === 'string' && /^\d{1,6}(?:\.\d{1,6}){1,3}$/.test(value);
const integer = value => Number.isSafeInteger(value) && value >= 0;
const duration = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= 86_400_000;
const nullableDuration = value => value === null || duration(value);
const oneOf = (value, choices) => choices.includes(value);
const outcome = value => oneOf(value, ['pass', 'fail', 'not_run', 'blocked']);
const check = value => oneOf(value, ['pass', 'fail', 'not_run', 'not_applicable']);
const nativeEngines = { 'macos-arm64': 'WKWebView', 'windows-x64': 'WebView2', 'ubuntu-24.04-x64': 'WebKitGTK' };
const metricNames = ['cpu_ms', 'rss_bytes', 'io_bytes', 'git_processes', 'live_processes', 'live_workers', 'mounted_rows', 'layout_count', 'highlight_starts', 'highlight_discards', 'anchor_shift_px', 'frame_ms'];
const metricReasons = ['none', 'unsupported', 'not_collected', 'collector_failed'];
const failureReasons = ['timeout', 'cancelled', 'resource_limit', 'unsupported', 'incorrect_result', 'fixture_failed', 'interrupted'];

function requireValue(valid, code) {
  if (!valid) throw new Error(code);
}

function fields(value, names, code) {
  requireValue(value !== null && typeof value === 'object' && !Array.isArray(value) && Object.getPrototypeOf(value) === Object.prototype, code);
  const keys = Object.keys(value);
  requireValue(keys.length === names.length && keys.every(key => names.includes(key)), code);
}

function array(value, minimum, maximum, code) {
  requireValue(Array.isArray(value) && value.length >= minimum && value.length <= maximum, code);
  const keys = Object.keys(value);
  requireValue(keys.length === value.length && keys.every((key, index) => key === String(index)), code);
}

function validateScenario(scenario) {
  fields(scenario, ['id', 'version', 'stories', 'eligibility'], 'invalid_scenario');
  requireValue(typeof scenario.id === 'string' && /^(?:W-0[1-5]|F-0[1-6]|H-0[1-4]|baseline)$/.test(scenario.id), 'invalid_scenario');
  requireValue(integer(scenario.version) && scenario.version > 0 && oneOf(scenario.eligibility, ['implemented', 'proposed', 'contradicted', 'unverified', 'unsupported']), 'invalid_scenario');
  array(scenario.stories, 1, 32, 'invalid_scenario');
  const keys = new Set();
  for (const story of scenario.stories) {
    fields(story, ['key', 'version'], 'invalid_scenario');
    requireValue(typeof story.key === 'string' && /^US-[1-9]\d{0,5}$/.test(story.key) && integer(story.version) && story.version > 0 && !keys.has(story.key), 'invalid_scenario');
    keys.add(story.key);
  }
}

function validateSource(source) {
  fields(source, ['revision', 'dirty', 'artifactHash', 'build'], 'invalid_source');
  requireValue(typeof source.revision === 'string' && /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/.test(source.revision), 'invalid_source');
  requireValue(typeof source.dirty === 'boolean' && hash(source.artifactHash) && oneOf(source.build, ['optimized', 'profiling', 'debug']), 'invalid_source');
}

function validateFixture(fixture) {
  fields(fixture, ['generatorHash', 'seed', 'files', 'commits', 'tags', 'variant'], 'invalid_fixture');
  requireValue(hash(fixture.generatorHash) && ['seed', 'files', 'commits', 'tags'].every(key => integer(fixture[key])), 'invalid_fixture');
  requireValue(oneOf(fixture.variant, ['empty', 'clean', 'few_changes', 'mixed', 'near_limit', 'unavailable', 'corrupt']), 'invalid_fixture');
}

function validateRuntime(runtime) {
  fields(runtime, ['platform', 'osVersion', 'engine', 'engineVersion', 'gitVersion', 'harnessVersion', 'hardwareHash', 'cpuCount', 'memoryBytes', 'power', 'viewportWidth', 'viewportHeight', 'scale', 'refreshHz'], 'invalid_runtime');
  requireValue(Object.hasOwn(nativeEngines, runtime.platform) && oneOf(runtime.engine, ['Chromium', 'WKWebView', 'WebView2', 'WebKitGTK', 'none']), 'invalid_runtime');
  requireValue(['osVersion', 'gitVersion', 'harnessVersion'].every(key => version(runtime[key])) && (runtime.engine === 'none' ? runtime.engineVersion === null : version(runtime.engineVersion)), 'invalid_runtime');
  requireValue(hash(runtime.hardwareHash) && ['cpuCount', 'memoryBytes', 'viewportWidth', 'viewportHeight'].every(key => integer(runtime[key]) && runtime[key] > 0), 'invalid_runtime');
  requireValue(oneOf(runtime.power, ['ac', 'battery', 'unknown']) && typeof runtime.scale === 'number' && runtime.scale > 0 && runtime.scale <= 10, 'invalid_runtime');
  requireValue(runtime.refreshHz === null || (typeof runtime.refreshHz === 'number' && runtime.refreshHz > 0 && runtime.refreshHz <= 1000), 'invalid_runtime');
}

function validateProtocol(protocol, source, runtime) {
  fields(protocol, ['boundary', 'process', 'cache', 'profile', 'clock', 'presentation', 'collectorHash', 'warmups', 'attempts', 'independent', 'budget'], 'invalid_protocol');
  requireValue(oneOf(protocol.boundary, ['service', 'browser-real-service', 'packaged-native', 'profiling-native']), 'invalid_protocol');
  requireValue(oneOf(protocol.process, ['warm', 'fresh']) && oneOf(protocol.cache, ['uncontrolled', 'warm', 'verified_cold']) && oneOf(protocol.profile, ['fresh', 'reused']), 'invalid_protocol');
  requireValue(protocol.clock === 'single-origin-monotonic' && oneOf(protocol.presentation, ['service-result', 'dom-raf-proxy', 'observed-frame', 'unavailable']), 'invalid_protocol');
  requireValue(hash(protocol.collectorHash) && integer(protocol.warmups) && integer(protocol.attempts) && protocol.attempts > 0 && protocol.warmups + protocol.attempts <= 10000, 'invalid_protocol');
  requireValue(typeof protocol.independent === 'boolean' && oneOf(protocol.budget, ['none', 'provisional']), 'invalid_protocol');
  if (protocol.boundary.endsWith('native')) {
    requireValue(runtime.engine === nativeEngines[runtime.platform] && source.build === (protocol.boundary === 'packaged-native' ? 'optimized' : 'profiling'), 'boundary_mismatch');
    requireValue(protocol.presentation !== 'service-result', 'boundary_mismatch');
  } else if (protocol.boundary === 'browser-real-service') {
    requireValue(runtime.engine === 'Chromium' && protocol.presentation !== 'service-result', 'boundary_mismatch');
  } else {
    requireValue(runtime.engine === 'none' && oneOf(protocol.presentation, ['service-result', 'unavailable']), 'boundary_mismatch');
  }
}

function validateMetric(metric, elapsedMs) {
  fields(metric, ['name', 'scope', 'collector', 'atMs', 'value', 'reason'], 'invalid_metric');
  requireValue(oneOf(metric.name, metricNames) && oneOf(metric.scope, ['host', 'webview', 'worker', 'git', 'renderer', 'process_tree']), 'invalid_metric');
  requireValue(oneOf(metric.collector, ['native_sampler', 'browser_performance', 'diagnostic_counts', 'dom_geometry', 'presentation_trace']) && duration(metric.atMs), 'invalid_metric');
  requireValue(elapsedMs === null || metric.atMs <= elapsedMs, 'invalid_metric');
  requireValue(oneOf(metric.reason, metricReasons) && (metric.reason === 'none' ? typeof metric.value === 'number' && Number.isFinite(metric.value) && metric.value >= 0 && metric.value <= Number.MAX_SAFE_INTEGER : metric.value === null), 'invalid_metric');
}

function validateSample(sample, index, report) {
  fields(sample, ['index', 'phase', 'behavior', 'timing', 'threshold', 'reason', 'elapsedMs', 'firstUsefulMs', 'completedMs', 'styledMs', 'checks', 'metrics'], 'invalid_sample');
  requireValue(sample.index === index + 1 && sample.phase === (index < report.protocol.warmups ? 'warmup' : 'measurement'), 'sample_count_mismatch');
  requireValue(outcome(sample.behavior) && oneOf(sample.timing, ['measured', 'unavailable']) && sample.threshold === 'not_evaluated', 'invalid_outcome');
  fields(sample.checks, ['exactResult', 'readOnly', 'continuity'], 'invalid_checks');
  requireValue(Object.values(sample.checks).every(check), 'invalid_checks');
  requireValue(['elapsedMs', 'firstUsefulMs', 'completedMs', 'styledMs'].every(key => nullableDuration(sample[key])), 'invalid_timing');
  if (sample.timing === 'unavailable') {
    requireValue(['elapsedMs', 'firstUsefulMs', 'completedMs', 'styledMs'].every(key => sample[key] === null), 'invalid_timing');
  } else {
    requireValue(sample.elapsedMs !== null, 'invalid_timing');
    requireValue(['firstUsefulMs', 'completedMs', 'styledMs'].every(key => sample[key] === null || sample[key] <= sample.elapsedMs), 'invalid_timing');
    requireValue(sample.completedMs === null || (sample.firstUsefulMs !== null && sample.completedMs >= sample.firstUsefulMs), 'invalid_timing');
    requireValue(sample.styledMs === null || (sample.firstUsefulMs !== null && sample.styledMs >= sample.firstUsefulMs), 'invalid_timing');
  }
  if (sample.behavior === 'pass') {
    requireValue(report.scenario.eligibility === 'implemented' && sample.checks.exactResult === 'pass' && sample.checks.readOnly === 'pass' && oneOf(sample.checks.continuity, ['pass', 'not_applicable']), 'invalid_outcome');
    requireValue(sample.timing === 'measured' ? sample.completedMs !== null && sample.reason === 'none' : sample.reason === 'collector_unavailable', 'invalid_outcome');
  } else {
    requireValue(oneOf(sample.reason, [...failureReasons, 'not_run', 'collector_unavailable']), 'invalid_outcome');
    if (sample.behavior === 'not_run' || sample.behavior === 'blocked') requireValue(sample.timing === 'unavailable', 'invalid_outcome');
  }
  requireValue(report.protocol.presentation !== 'unavailable' || sample.timing === 'unavailable', 'invalid_timing');
  array(sample.metrics, 0, 10000, 'invalid_metric');
  for (const metric of sample.metrics) validateMetric(metric, sample.elapsedMs);
}

function validateReport(report) {
  fields(report, ['schemaVersion', 'runId', 'observedAt', 'scenario', 'source', 'fixture', 'runtime', 'protocol', 'samples', 'cleanup'], 'invalid_report');
  requireValue(report.schemaVersion === 1 && typeof report.runId === 'string' && /^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/.test(report.runId), 'invalid_report');
  requireValue(typeof report.observedAt === 'string' && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$/.test(report.observedAt) && Number.isFinite(Date.parse(report.observedAt)) && new Date(report.observedAt).toISOString() === report.observedAt, 'invalid_report');
  validateScenario(report.scenario);
  validateSource(report.source);
  validateFixture(report.fixture);
  validateRuntime(report.runtime);
  validateProtocol(report.protocol, report.source, report.runtime);
  const count = report.protocol.warmups + report.protocol.attempts;
  array(report.samples, count, count, 'sample_count_mismatch');
  report.samples.forEach((sample, index) => validateSample(sample, index, report));
  fields(report.cleanup, ['processes', 'fixtures', 'appState'], 'invalid_cleanup');
  requireValue(Object.values(report.cleanup).every(value => oneOf(value, ['pass', 'fail', 'not_run'])), 'invalid_cleanup');
  requireValue(Buffer.byteLength(JSON.stringify(report)) <= OUTPUT_LIMIT, 'input_limit');
}

function distribution(values, independent) {
  values.sort((a, b) => a - b);
  const count = values.length;
  const middle = Math.floor(count / 2);
  return {
    count,
    medianMs: count === 0 ? null : count % 2 ? values[middle] : (values[middle - 1] + values[middle]) / 2,
    p95Ms: count >= 50 && independent ? values[Math.ceil(count * 0.95) - 1] : null,
    maxMs: count === 0 ? null : values[count - 1],
    tailReason: !independent ? 'correlated_samples' : count < 50 ? 'insufficient_samples' : 'empirical_not_confidence_bound',
  };
}

function summarize(report) {
  const measured = report.samples.slice(report.protocol.warmups);
  const outcomes = { pass: 0, fail: 0, not_run: 0, blocked: 0 };
  for (const sample of measured) outcomes[sample.behavior]++;
  const successful = measured.filter(sample => sample.behavior === 'pass' && sample.timing === 'measured');
  return {
    attempts: measured.length,
    warmups: report.protocol.warmups,
    outcomes,
    unavailableTiming: measured.filter(sample => sample.timing === 'unavailable').length,
    firstUseful: distribution(successful.map(sample => sample.firstUsefulMs), report.protocol.independent),
    completed: distribution(successful.map(sample => sample.completedMs), report.protocol.independent),
    styled: distribution(successful.filter(sample => sample.styledMs !== null).map(sample => sample.styledMs), report.protocol.independent),
  };
}

/** Rejects unknown fields and contradictory claims without echoing invalid input; never establishes observation truth. */
export function assessPerformance(input) {
  let report;
  try {
    // Callers may supply accessors: validate precisely the detached snapshot returned below.
    report = structuredClone(input);
    validateReport(report);
  } catch (error) {
    const codes = ['invalid_report', 'invalid_scenario', 'invalid_source', 'invalid_fixture', 'invalid_runtime', 'invalid_protocol', 'boundary_mismatch', 'invalid_metric', 'invalid_sample', 'sample_count_mismatch', 'invalid_outcome', 'invalid_checks', 'invalid_timing', 'invalid_cleanup', 'input_limit'];
    return { schemaVersion: 1, status: 'invalid', reasons: [codes.includes(error?.message) ? error.message : 'invalid_report'], report: null, summary: null, assurance: 'supplied_observations_not_certified' };
  }
  const complete = report.samples.every(sample => sample.behavior === 'pass' && sample.timing === 'measured') && Object.values(report.cleanup).every(value => value === 'pass');
  return { schemaVersion: 1, status: complete ? 'recorded' : 'incomplete', reasons: complete ? [] : ['incomplete_observations'], report, summary: summarize(report), assurance: 'supplied_observations_not_certified' };
}

/** Compares matching conditions, not platforms or profiling modes; incomplete evidence never yields a gain verdict. */
export function comparePerformance(baselineInput, candidateInput) {
  const baseline = assessPerformance(baselineInput);
  const candidate = assessPerformance(candidateInput);
  const result = { schemaVersion: 1, status: 'incomparable', reasons: [], baseline, candidate, delta: null, threshold: 'not_evaluated' };
  if (baseline.status === 'invalid' || candidate.status === 'invalid') return { ...result, reasons: ['invalid_evidence'] };
  for (const field of ['scenario', 'fixture', 'runtime', 'protocol']) {
    if (!isDeepStrictEqual(baseline.report[field], candidate.report[field])) result.reasons.push(`${field}_mismatch`);
  }
  if (baseline.report.source.build !== candidate.report.source.build) result.reasons.push('build_mismatch');
  if (result.reasons.length > 0) return result;
  if (baseline.status !== 'recorded' || candidate.status !== 'recorded') return { ...result, status: 'incomplete', reasons: ['incomplete_observations'] };
  return { ...result, status: 'comparable', delta: {
    firstUsefulMedianMs: candidate.summary.firstUseful.medianMs - baseline.summary.firstUseful.medianMs,
    completedMedianMs: candidate.summary.completed.medianMs - baseline.summary.completed.medianMs,
    completedP95Ms: baseline.summary.completed.p95Ms === null || candidate.summary.completed.p95Ms === null ? null : candidate.summary.completed.p95Ms - baseline.summary.completed.p95Ms,
  } };
}

if (isEntryPoint(import.meta.url)) {
  try {
    const options = parseOptions(process.argv.slice(2), ['report', 'output'], { output: '.verification/performance' });
    if (!options.report) throw new Error('invalid_arguments');
    let assessment;
    try { assessment = assessPerformance(await readJsonBounded(options.report)); }
    catch { assessment = { schemaVersion: 1, status: 'invalid', reasons: ['report_unavailable'], report: null, summary: null, assurance: 'supplied_observations_not_certified' }; }
    await writeEvidence(resolve(options.output), 'performance-evidence.json', assessment);
    console.log(JSON.stringify({ status: assessment.status, reasons: assessment.reasons }));
    process.exitCode = assessment.status === 'recorded' ? 0 : assessment.status === 'incomplete' ? 1 : 2;
  } catch {
    console.error('performance_evidence_failed: invalid arguments or evidence unavailable');
    process.exitCode = 2;
  }
}
