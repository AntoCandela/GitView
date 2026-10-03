/** Installs repository-owned hooks without replacing another hook configuration or active hooks. */
import { chmod, readdir, realpath, access } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { isEntryPoint } from './evidence.mjs';

const repositoryRoot = fileURLToPath(new URL('../', import.meta.url));

/**
 * Configures local Git only for this package's worktree root.
 * Conflicting hooks remain untouched; skipped results never claim installation.
 * The environment override isolates fixture Git commands without changing global process state.
 */
export async function installHooks({ root = repositoryRoot, environment = process.env } = {}) {
  const git = args => spawnSync('git', args, { cwd: root, env: environment, encoding: 'utf8', maxBuffer: 64 * 1024 });
  const repository = git(['rev-parse', '--show-toplevel']);
  if (repository.status !== 0) return { status: 'skipped', reason: 'not_a_git_worktree' };
  if (await realpath(root) !== await realpath(repository.stdout.trim())) {
    return { status: 'failed', reason: 'package_is_not_worktree_root' };
  }
  const configured = git(['config', '--get', 'core.hooksPath']);
  if (configured.status !== 0 && configured.status !== 1) return { status: 'failed', reason: 'hook_configuration_unavailable' };
  if (configured.status === 0 && resolve(root, configured.stdout.trim()) !== resolve(root, '.githooks')) {
    return { status: 'failed', reason: 'custom_hooks_path_preserved' };
  }
  if (configured.status === 1) {
    // Redirecting hooksPath would disable every existing default hook, not only our two gates.
    const hooksPath = git(['rev-parse', '--git-path', 'hooks']);
    if (hooksPath.status !== 0) return { status: 'failed', reason: 'default_hooks_unavailable' };
    try {
      const hooks = await readdir(resolve(root, hooksPath.stdout.trim()));
      if (hooks.some(name => !name.endsWith('.sample'))) return { status: 'failed', reason: 'existing_default_hooks_preserved' };
    } catch (error) {
      if (error.code !== 'ENOENT') return { status: 'failed', reason: 'default_hooks_unavailable' };
    }
  }
  for (const name of ['pre-commit', 'pre-push']) {
    const hook = resolve(root, '.githooks', name);
    await access(hook);
    await chmod(hook, 0o755);
  }
  const installed = git(['config', '--local', 'core.hooksPath', '.githooks']);
  return installed.status === 0 ? { status: 'installed' } : { status: 'failed', reason: 'hook_configuration_write_failed' };
}

if (isEntryPoint(import.meta.url)) {
  try {
    const result = process.env.CI && process.env.npm_lifecycle_event === 'prepare'
      ? { status: 'skipped', reason: 'ci_uses_shared_verification_directly' }
      : await installHooks();
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exitCode = result.status === 'failed' ? 1 : 0;
  } catch {
    process.stderr.write('hook_installation_failed: repository hooks unavailable\n');
    process.exitCode = 1;
  }
}
