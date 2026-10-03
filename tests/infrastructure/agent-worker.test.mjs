/** Exercises ownership, detached baselines, port contention and live Vite lifecycle against disposable Git. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm, realpath, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { once } from 'node:events';
import { prepareWorker, runWorker, removeWorker } from '../../.agents/scripts/worker.mjs';

async function freePort() {
  const server = createServer((request, response) => response.end('unrelated server'));
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise(done => server.close(done));
  return port;
}

async function fixture(t) {
  const directory = await realpath(await mkdtemp(join(tmpdir(), 'gitview-worker-fixture-')));
  const cleanups = [];
  t.after(async () => {
    await Promise.all(cleanups.map(cleanup => cleanup()));
    await rm(directory, { recursive: true, force: true });
  });
  const root = join(directory, 'parent');
  await mkdir(root);
  const git = (args, cwd = root) => {
    const env = { ...process.env };
    for (const key of Object.keys(env)) if (key.startsWith('GIT_')) delete env[key];
    env.GIT_CONFIG_NOSYSTEM = '1';
    env.GIT_CONFIG_GLOBAL = join(directory, 'empty-global-config');
    const result = spawnSync('git', args, { cwd, env, encoding: 'utf8' });
    assert.equal(result.status, 0, 'disposable Git fixture command succeeds');
    return result.stdout.trim();
  };
  git(['init', '--quiet', '--initial-branch=main']);
  git(['config', 'user.name', 'Worker fixture']);
  git(['config', 'user.email', 'fixture@example.org']);
  await writeFile(join(root, '.gitignore'), 'node_modules/\n.verification/\n*.sqlite\nsrc-tauri/permissions/\n');
  await writeFile(join(root, 'index.html'), '<!doctype html><title>Committed worker baseline</title>');
  await writeFile(join(root, 'vite.config.ts'), 'export default { clearScreen: false };\n');
  git(['add', '.']);
  git(['commit', '--quiet', '-m', 'fixture baseline']);
  const rev = git(['rev-parse', 'HEAD']);
  const prepare = async (name, port) => prepareWorker({ root, path: join(directory, name), rev, port: port ?? await freePort() });
  return { directory, root, rev, git, prepare, cleanups };
}

async function readyWorker(t, f, worker) {
  await symlink(resolve('node_modules'), join(worker.root, 'node_modules'), 'dir');
  const controller = new AbortController();
  let ready;
  const listening = new Promise(done => { ready = done; });
  const running = runWorker({ root: f.root, path: worker.root, mode: 'web', signal: controller.signal, onReady: ready });
  // Teardown always waits for owned child groups, even when an assertion fails.
  f.cleanups.push(async () => { controller.abort(); await running.catch(() => {}); });
  await Promise.race([listening, running.then(result => { throw new Error(`web exited before readiness: ${result.code}`); })]);
  return { running, stop: () => controller.abort() };
}

test('worker preparation excludes dirty parent edits and remains detached from its explicit revision', async t => {
  const f = await fixture(t);
  await writeFile(join(f.root, 'index.html'), 'private dirty parent edits');
  await writeFile(join(f.root, 'unstaged.txt'), 'never copied');
  const worker = await f.prepare('detached');
  assert.equal(await readFile(join(worker.root, 'index.html'), 'utf8'), '<!doctype html><title>Committed worker baseline</title>');
  await assert.rejects(readFile(join(worker.root, 'unstaged.txt')), { code: 'ENOENT' });
  assert.equal(f.git(['rev-parse', 'HEAD'], worker.root), f.rev);
  assert.notEqual(spawnSync('git', ['symbolic-ref', '-q', 'HEAD'], { cwd: worker.root }).status, 0);
  assert.equal(await readFile(join(f.root, 'index.html'), 'utf8'), 'private dirty parent edits');
  assert.equal((await removeWorker({ root: f.root, path: worker.root })).status, 'removed');
});

test('worker preparation preserves existing paths and refuses reserved ports', async t => {
  const f = await fixture(t);
  const existing = join(f.directory, 'user-owned');
  await mkdir(existing);
  await writeFile(join(existing, 'keep.txt'), 'user data');
  await assert.rejects(prepareWorker({ root: f.root, path: existing, rev: f.rev, port: await freePort(t) }), /worker_path_exists/);
  const worker = await f.prepare('first');
  await assert.rejects(f.prepare('second', worker.port), /worker_port_reserved/);
  await assert.rejects(removeWorker({ root: f.root, path: existing }), /worker_not_owned/);
  await assert.rejects(runWorker({ root: f.root, path: existing, mode: 'web' }), /worker_not_owned/);
  assert.equal(await readFile(join(existing, 'keep.txt'), 'utf8'), 'user data');
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker removal preserves tracked modifications and untracked source', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('dirty');
  await writeFile(join(worker.root, 'index.html'), 'worker edits');
  await assert.rejects(removeWorker({ root: f.root, path: worker.root }), /worker_dirty/);
  assert.equal(await readFile(join(worker.root, 'index.html'), 'utf8'), 'worker edits');
  await writeFile(join(worker.root, 'index.html'), '<!doctype html><title>Committed worker baseline</title>');
  await writeFile(join(worker.root, 'new-source.ts'), 'export const work = true;');
  await assert.rejects(removeWorker({ root: f.root, path: worker.root }), /worker_dirty/);
  assert.equal(await readFile(join(worker.root, 'new-source.ts'), 'utf8'), 'export const work = true;');
  await rm(join(worker.root, 'new-source.ts'));
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker removal preserves ignored files outside generated runtime directories', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('ignored-source');
  await writeFile(join(worker.root, 'user.sqlite'), 'not worker-owned');
  await assert.rejects(removeWorker({ root: f.root, path: worker.root }), /worker_untracked_files/);
  assert.equal(await readFile(join(worker.root, 'user.sqlite'), 'utf8'), 'not worker-owned');
  await rm(join(worker.root, 'user.sqlite'));
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker removal deletes generated native permissions nested under an ignored parent', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('generated-permissions');
  const generated = join(worker.root, 'src-tauri', 'permissions', 'autogenerated');
  await mkdir(generated, { recursive: true });
  await writeFile(join(generated, 'commands.toml'), 'generated native permissions');
  await removeWorker({ root: f.root, path: worker.root });
  await assert.rejects(readFile(join(generated, 'commands.toml')), { code: 'ENOENT' });
});

test('worker removal preserves ignored permission sources beside generated permissions', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('private-permissions');
  const generated = join(worker.root, 'src-tauri', 'permissions', 'autogenerated');
  await mkdir(generated, { recursive: true });
  await writeFile(join(generated, 'commands.toml'), 'generated native permissions');
  const source = join(worker.root, 'src-tauri', 'permissions', 'private.toml');
  await writeFile(source, 'user-authored permission');
  await assert.rejects(removeWorker({ root: f.root, path: worker.root }), /worker_untracked_files/);
  assert.equal(await readFile(source, 'utf8'), 'user-authored permission');
  await rm(source);
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker refuses occupied ports without disturbing the unrelated listener', async t => {
  const f = await fixture(t);
  const server = createServer((request, response) => response.end('unrelated server'));
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => new Promise(done => server.close(done)));
  const worker = await f.prepare('busy', server.address().port);
  await assert.rejects(runWorker({ root: f.root, path: worker.root, mode: 'web' }), process.platform === 'win32' ? /worker_lifecycle_requires_posix/ : /worker_port_busy/);
  assert.equal(await (await fetch(`http://127.0.0.1:${worker.port}`)).text(), 'unrelated server');
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker runtime isolates live POSIX workers or rejects unsupported Windows execution', async t => {
  const f = await fixture(t);
  const first = await f.prepare('first');
  const second = await f.prepare('second');
  if (process.platform === 'win32') {
    // This proves fail-closed Windows admission, not POSIX serving or child-tree shutdown.
    await assert.rejects(runWorker({ root: f.root, path: first.root, mode: 'web' }), /worker_lifecycle_requires_posix/);
    await assert.rejects(runWorker({ root: f.root, path: second.root, mode: 'native' }), /worker_lifecycle_requires_posix/);
    assert.notEqual(first.identifier, second.identifier);
    assert.notEqual(first.appData, second.appData);
    await assert.rejects(readFile(join(first.root, '.verification', 'worker', 'runtime.json')), { code: 'ENOENT' });
    await removeWorker({ root: f.root, path: first.root });
    await removeWorker({ root: f.root, path: second.root });
    return;
  }
  await writeFile(join(first.root, 'index.html'), '<!doctype html><title>First isolated worker</title>');
  const firstRun = await readyWorker(t, f, first);
  const secondRun = await readyWorker(t, f, second);
  assert.match(await (await fetch(`http://127.0.0.1:${first.port}`)).text(), /First isolated worker/);
  assert.match(await (await fetch(`http://127.0.0.1:${second.port}`)).text(), /Committed worker baseline/);
  const runtime = JSON.parse(await readFile(join(first.root, '.verification', 'worker', 'runtime.json'), 'utf8'));
  const temporaryFixture = await mkdtemp(join(runtime.temporary, 'non-repository-'));
  await writeFile(join(runtime.temporary, 'retained.txt'), 'owned temporary state');
  assert.notEqual(spawnSync('git', ['rev-parse', '--absolute-git-dir'], { cwd: temporaryFixture }).status, 0);
  assert.notEqual(first.identifier, second.identifier);
  assert.notEqual(first.appData, second.appData);
  assert.notEqual(first.identifier, 'com.gitview.app');
  await assert.rejects(removeWorker({ root: f.root, path: first.root }), /worker_active_or_interrupted/);
  await assert.rejects(runWorker({ root: f.root, path: first.root, mode: 'verify' }), /worker_active_or_interrupted/);
  firstRun.stop();
  secondRun.stop();
  assert.equal((await firstRun.running).status, 'cancelled');
  assert.equal((await secondRun.running).status, 'cancelled');
  await assert.rejects(readFile(join(runtime.temporary, 'retained.txt')), { code: 'ENOENT' });
  const probe = createServer();
  probe.listen(first.port, '127.0.0.1');
  await once(probe, 'listening');
  await new Promise(done => probe.close(done));
  await writeFile(join(first.root, 'index.html'), '<!doctype html><title>Committed worker baseline</title>');
  await rm(join(first.root, 'node_modules'));
  await rm(join(second.root, 'node_modules'));
  await removeWorker({ root: f.root, path: first.root });
  await removeWorker({ root: f.root, path: second.root });
});

test('worker refuses a temporary root inside Git rather than changing repository discovery', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('checkout-temporary-root');
  const result = spawnSync(process.execPath, [resolve('.agents/scripts/worker.mjs'), 'run', '--root', f.root, '--path', worker.root, '--mode', 'web'], {
    env: { ...process.env, TMPDIR: worker.root }, encoding: 'utf8',
  });
  assert.equal(result.status, 1);
  assert.equal(JSON.parse(result.stderr).code, process.platform === 'win32' ? 'worker_lifecycle_requires_posix' : 'worker_temporary_root_in_repository');
  assert.equal(await readFile(join(worker.root, 'index.html'), 'utf8'), '<!doctype html><title>Committed worker baseline</title>');
  await removeWorker({ root: f.root, path: worker.root });
});

test('interrupted worker lock requires manual inspection and never authorizes removal', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('interrupted');
  const lock = join(f.git(['rev-parse', '--absolute-git-dir'], worker.root), 'gitview-worker-active');
  await mkdir(lock);
  await writeFile(join(lock, 'untrusted-pid'), String(process.pid));
  await assert.rejects(removeWorker({ root: f.root, path: worker.root }), /worker_active_or_interrupted/);
  assert.equal(await readFile(join(lock, 'untrusted-pid'), 'utf8'), String(process.pid));
  await rm(lock, { recursive: true });
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker run rejects arbitrary executable modes before touching a checkout', async () => {
  await assert.rejects(runWorker({ path: '/not-a-worker', mode: 'sh' }), /invalid_mode/);
});

test('worker lifecycle terminates POSIX descendants or refuses unsupported Windows execution', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('child-tree');
  if (process.platform === 'win32') {
    // No child is launched on this platform; this is rejection evidence only.
    await assert.rejects(runWorker({ root: f.root, path: worker.root, mode: 'verify' }), /worker_lifecycle_requires_posix/);
    await assert.rejects(readFile(join(worker.root, '.verification', 'worker', 'runtime.json')), { code: 'ENOENT' });
    await removeWorker({ root: f.root, path: worker.root });
    return;
  }
  const descendantPort = await freePort();
  const binary = join(worker.root, 'node_modules', 'vite', 'bin');
  await mkdir(binary, { recursive: true });
  const descendant = `require('node:http').createServer((req,res)=>res.end('owned descendant')).listen(${descendantPort},'127.0.0.1',()=>process.send('ready'));process.on('SIGTERM',()=>{});`;
  const service = `const {spawn}=require('node:child_process');const http=require('node:http');const child=spawn(process.execPath,['-e',${JSON.stringify(descendant)}],{stdio:['ignore','ignore','ignore','ipc']});child.once('message',()=>http.createServer((req,res)=>{res.setHeader('X-GitView-Worker',${JSON.stringify(worker.id)});res.end('owned runtime');}).listen(${worker.port},'127.0.0.1'));process.on('SIGTERM',()=>process.exit(0));`;
  await writeFile(join(binary, 'vite.js'), service);
  const controller = new AbortController();
  let ready;
  const listening = new Promise(done => { ready = done; });
  const running = runWorker({ root: f.root, path: worker.root, mode: 'web', signal: controller.signal, onReady: ready });
  f.cleanups.push(async () => { controller.abort(); await running.catch(() => {}); });
  await Promise.race([listening, running.then(() => { throw new Error('runtime exited before readiness'); })]);
  assert.equal(await (await fetch(`http://127.0.0.1:${descendantPort}`)).text(), 'owned descendant');
  controller.abort();
  assert.equal((await running).status, 'cancelled');
  const probe = createServer();
  probe.listen(descendantPort, '127.0.0.1');
  await once(probe, 'listening');
  await new Promise(done => probe.close(done));
  await removeWorker({ root: f.root, path: worker.root });
});

test('worker runtime fails closed for symlinked cache or unsupported lifecycle without touching user data', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('cache-symlink');
  const external = join(f.directory, 'user-cache');
  await mkdir(external);
  await writeFile(join(external, 'keep.txt'), 'user cache');
  await symlink(external, join(worker.root, '.verification', 'worker', 'cache'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(runWorker({ root: f.root, path: worker.root, mode: 'web' }), process.platform === 'win32' ? /worker_lifecycle_requires_posix/ : /worker_runtime_symlink/);
  assert.equal(await readFile(join(external, 'keep.txt'), 'utf8'), 'user cache');
  await removeWorker({ root: f.root, path: worker.root });
  assert.equal(await readFile(join(external, 'keep.txt'), 'utf8'), 'user cache');
});

test('nested runtime cache symlinks cannot redirect writes outside the owned worker', async t => {
  const f = await fixture(t);
  const worker = await f.prepare('nested-cache-symlink');
  const external = join(f.directory, 'user-registry');
  await mkdir(external);
  await writeFile(join(external, 'keep.txt'), 'user registry');
  const cargo = join(worker.root, '.verification', 'worker', 'cache', 'cargo');
  await mkdir(cargo, { recursive: true });
  await symlink(external, join(cargo, 'registry'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(runWorker({ root: f.root, path: worker.root, mode: 'web' }), process.platform === 'win32' ? /worker_lifecycle_requires_posix/ : /worker_runtime_symlink/);
  await assert.rejects(readFile(join(worker.root, '.verification', 'worker', 'runtime.json')), { code: 'ENOENT' });
  assert.equal(await readFile(join(external, 'keep.txt'), 'utf8'), 'user registry');
  await removeWorker({ root: f.root, path: worker.root });
  assert.equal(await readFile(join(external, 'keep.txt'), 'utf8'), 'user registry');
});
