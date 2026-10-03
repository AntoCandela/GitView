/** Shared bounded process and evidence I/O; raw child output never goes to artifacts. */
import { spawn } from 'node:child_process';
import { mkdir, writeFile, readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const OUTPUT_LIMIT = 2 * 1024 * 1024;
export const safeIdentifier = value => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9 _.:,()'-]{0,159}$/.test(value);
export const isEntryPoint = url => process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === url;

export function parseOptions(argv, allowed, defaults = {}) {
  const options = { ...defaults };
  const seen = new Set();
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index]?.replace(/^--/, '');
    const value = argv[index + 1];
    if (!argv[index]?.startsWith('--') || !allowed.includes(key) || seen.has(key) || !value || value.startsWith('--')) throw new Error('invalid_arguments');
    seen.add(key);
    options[key] = value;
  }
  return options;
}

export async function writeEvidence(directory, filename, evidence) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await writeFile(join(directory, filename), `${JSON.stringify(evidence, null, 2)}\n`, { mode: 0o600 });
}

export async function sourceFiles(root, directory) {
  const paths = [];
  async function visit(relative) {
    let entries;
    try { entries = await readdir(join(root, relative), { withFileTypes: true }); }
    catch (error) { if (error.code === 'ENOENT') return; throw error; }
    for (const entry of entries) {
      const path = `${relative}/${entry.name}`;
      if (entry.isDirectory()) await visit(path);
      else if (entry.isFile()) paths.push(path);
    }
  }
  await visit(directory);
  return paths.sort();
}

export async function readJsonBounded(path) {
  const content = await readFile(path);
  if (content.length > OUTPUT_LIMIT) throw new Error('input_limit');
  return JSON.parse(content.toString('utf8'));
}

/** Start errors, signals and output overflow are facts independent of process exit. */
export function execute(executable, args, { cwd, env, signal, outputLimit = OUTPUT_LIMIT } = {}) {
  return new Promise(resolveResult => {
    let child;
    let output = '';
    let bytes = 0;
    let exceeded = false;
    let startFailed = false;
    let cancelled = Boolean(signal?.aborted);
    let killTimer;
    if (cancelled) return resolveResult({ output, exitCode: null, signal: null, code: 'cancelled' });
    try { child = spawn(executable, args, { cwd, env, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] }); }
    catch { return resolveResult({ output, exitCode: null, signal: null, code: 'start_failed' }); }
    const cancel = () => {
      cancelled = true;
      child.kill('SIGTERM');
      killTimer = setTimeout(() => child.kill('SIGKILL'), 1000);
      killTimer.unref();
    };
    signal?.addEventListener('abort', cancel, { once: true });
    if (signal?.aborted) cancel();
    const collect = chunk => {
      bytes += chunk.length;
      if (bytes <= outputLimit) output += chunk.toString('utf8');
      else exceeded = true;
    };
    child.stdout.on('data', collect);
    child.stderr.on('data', collect);
    child.on('error', () => { startFailed = true; });
    child.on('close', (exitCode, terminationSignal) => {
      clearTimeout(killTimer);
      signal?.removeEventListener('abort', cancel);
      resolveResult({ output, exitCode, signal: terminationSignal, code: startFailed ? 'start_failed' : cancelled ? 'cancelled' : terminationSignal ? 'signal_termination' : exceeded ? 'output_limit' : exitCode === 0 ? 'ok' : 'check_failed' });
    });
  });
}
