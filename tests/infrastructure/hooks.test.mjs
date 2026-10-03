/** Proves Git refuses commit/push after failed checks and preserves existing hook ownership. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { installHooks } from '../../scripts/install-hooks.mjs';

async function fixture(t, { policy = 'pass', unit = 'fail', verification = 'fail' } = {}) {
  const directory = await mkdtemp(join(tmpdir(), 'gitview-hook-fixture-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const root = join(directory, 'working');
  await mkdir(join(root, '.githooks'), { recursive: true });
  await mkdir(join(root, 'checks'));
  const environment = { ...process.env, GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: join(directory, 'empty-global-config') };
  const git = args => spawnSync('git', args, { cwd: root, env: environment, encoding: 'utf8' });
  for (const variable of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR', 'GIT_CONFIG_PARAMETERS', 'GIT_CONFIG_COUNT']) delete environment[variable];
  assert.equal(git(['init', '--quiet', '--initial-branch=main']).status, 0);
  assert.equal(git(['config', 'user.name', 'Hook fixture']).status, 0);
  assert.equal(git(['config', 'user.email', 'fixture@example.org']).status, 0);
  for (const name of ['pre-commit', 'pre-push']) {
    await writeFile(join(root, '.githooks', name), await readFile(new URL(`../../.githooks/${name}`, import.meta.url)));
  }
  await writeFile(join(root, 'checks/pass.mjs'), 'process.exit(0);\n');
  await writeFile(join(root, 'checks/fail.mjs'), 'process.exit(1);\n');
  await writeFile(join(root, 'package.json'), JSON.stringify({ scripts: {
    policy: `node checks/${policy}.mjs`,
    'test:unit': `node checks/${unit}.mjs`,
    verify: `node checks/${verification}.mjs`,
  } }));
  return { root, directory, environment, git };
}

async function assertCommitBlocked(fixture) {
  assert.equal((await installHooks(fixture)).status, 'installed');
  assert.notEqual(fixture.git(['commit', '--allow-empty', '--quiet', '-m', 'blocked']).status, 0);
  assert.notEqual(fixture.git(['rev-parse', '--verify', 'HEAD']).status, 0, 'failed gate must not create a commit');
}

test('pre-commit prevents a commit when unit verification fails', async t => {
  await assertCommitBlocked(await fixture(t));
});

test('pre-commit prevents a commit when policy fails even if unit checks would pass', async t => {
  await assertCommitBlocked(await fixture(t, { policy: 'fail', unit: 'pass' }));
});

test('pre-push prevents remote ref creation when full verification fails', async t => {
  const f = await fixture(t);
  assert.equal(f.git(['commit', '--allow-empty', '--quiet', '-m', 'fixture']).status, 0);
  const remote = join(f.directory, 'remote.git');
  assert.equal(f.git(['init', '--bare', '--quiet', remote]).status, 0);
  assert.equal(f.git(['remote', 'add', 'fixture', remote]).status, 0);
  assert.equal((await installHooks(f)).status, 'installed');
  assert.notEqual(f.git(['push', '--quiet', 'fixture', 'HEAD:refs/heads/main']).status, 0);
  assert.notEqual(f.git(['--git-dir', remote, 'show-ref', '--verify', 'refs/heads/main']).status, 0, 'failed gate must not publish a remote ref');
});

test('hook installation preserves another configured hooks path', async t => {
  const f = await fixture(t);
  assert.equal(f.git(['config', 'core.hooksPath', 'user-owned-hooks']).status, 0);
  assert.deepEqual(await installHooks(f), { status: 'failed', reason: 'custom_hooks_path_preserved' });
  assert.equal(f.git(['config', '--get', 'core.hooksPath']).stdout.trim(), 'user-owned-hooks');
});

test('hook installation preserves active default hooks without redirecting Git', async t => {
  const f = await fixture(t);
  const existing = join(f.root, '.git/hooks/pre-commit');
  const bytes = '#!/bin/sh\nexit 7\n';
  await writeFile(existing, bytes);
  assert.deepEqual(await installHooks(f), { status: 'failed', reason: 'existing_default_hooks_preserved' });
  assert.equal(f.git(['config', '--get', 'core.hooksPath']).status, 1);
  assert.equal(await readFile(existing, 'utf8'), bytes);
});
