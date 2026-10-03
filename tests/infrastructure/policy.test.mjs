/** Checks enforceable boundaries against temporary repositories, not prose. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { checkPolicy } from '../../scripts/check-policy.mjs';

async function fixture(t, files) {
  const root = await mkdtemp(join(tmpdir(), 'gitview-policy-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const [path, content] of Object.entries(files)) {
    await mkdir(dirname(join(root, path)), { recursive: true });
    await writeFile(join(root, path), content);
  }
  return { root, trackedFiles: Object.keys(files) };
}

test('misplaced tests and tracked private artifacts block policy', async t => {
  const result = await checkPolicy(await fixture(t, { 'src/view.test.ts': 'export {};', '.verification/manifest.json': '{}', 'data/diagnostics.sqlite-wal': '' }));
  assert.equal(result.passed, false);
  assert.ok(result.violations.some(item => item.rule === 'test_placement'));
  assert.ok(result.violations.some(item => item.rule === 'private_artifact'));
});

test('valid non-allowlisted filenames are checked without becoming private artifacts', async t => {
  const allowed = await checkPolicy(await fixture(t, {
    'src-tauri/icons/128x128@2x.png': '',
    'src/contracts/Überblick.ts': 'export const value = 1;',
  }));
  assert.equal(allowed.passed, true);
  const prohibited = await checkPolicy(await fixture(t, {
    'src/contracts/Überblick.ts': "export { invoke } from '@tauri-apps/api/core';",
  }));
  assert.equal(prohibited.passed, false);
  assert.ok(prohibited.violations.some(item => item.rule === 'contract_dependency'));
  assert.equal(prohibited.violations.some(item => item.rule === 'private_artifact'), false);
});

test('contract dynamic imports and reexports cannot bypass architecture policy', async t => {
  const result = await checkPolicy(await fixture(t, { 'src/contracts/model.ts': "export { invoke } from '@tauri-apps/api/core'; const load = () => import('react');" }));
  assert.equal(result.passed, false);
  assert.equal(result.violations.filter(item => item.rule === 'contract_dependency').length, 2);
});

test('import-like comments and strings are not dependencies and legitimate persistence is allowed', async t => {
  const result = await checkPolicy(await fixture(t, {
    'src/contracts/model.ts': "// import 'react';\nexport const example = \"import('@tauri-apps/api/core')\";",
    'src-tauri/src/workspace/persistence.rs': 'use std::fs;\n#[cfg(test)]\n#[path = "../../tests/unit/workspace_persistence.rs"]\nmod unit_tests;',
    'tests/unit/model.test.ts': 'export {};',
  }));
  assert.equal(result.passed, true);
  assert.ok(result.humanReview.includes('native_observations'));
});

test('Rust inline behavior tests must move but test-only path declarations remain valid', async t => {
  const result = await checkPolicy(await fixture(t, { 'src-tauri/src/workspace/mod.rs': '#[cfg(test)]\nmod tests { #[test] fn preserves_selection() {} }' }));
  assert.equal(result.passed, false);
  assert.ok(result.violations.some(item => item.rule === 'test_placement'));
});

test('Rust grouped native imports are enforced while nested comments and raw strings are ignored', async t => {
  const prohibited = await checkPolicy(await fixture(t, { 'src-tauri/src/workspace/mod.rs': 'use std::{fs, process::Command};' }));
  assert.equal(prohibited.passed, false);
  assert.ok(prohibited.violations.some(item => item.rule === 'native_dependency'));
  const allowed = await checkPolicy(await fixture(t, { 'src-tauri/src/workspace/mod.rs': '/* outer /* inner */ use std::fs; */ const EXAMPLE: &str = r#\"use tauri::Runtime; #[test]\"#;' }));
  assert.equal(allowed.passed, true);
});

test('app composition cannot leak into contracts, shared controls, platform or features', async t => {
  const result = await checkPolicy(await fixture(t, {
    'src/contracts/model.ts': "import type { Screen } from '../app/Screen';",
    'src/ui/Button.tsx': "export { Screen } from '../app/Screen';",
    'src/platform/client.ts': "const screen = () => import('../app/Screen');",
    'src/features/history/model.ts': "import { Screen } from '../../app/Screen';",
  }));
  assert.deepEqual(result.violations.map(item => item.rule).sort(),
    ['contract_dependency', 'feature_dependency', 'platform_dependency', 'ui_dependency']);
});

test('feature consumers use explicit public APIs while feature-local modules stay private', async t => {
  const allowed = await checkPolicy(await fixture(t, {
    'src/app/Workspace.tsx': "import { Browser } from '../features/repositories';",
    'src/features/history/graph/Graph.tsx': "import type { Selection } from '../../diff/index.ts';",
    'src/features/diff/text/TextDiff.tsx': "import { highlight } from '../highlighting/highlight';",
    'src/features/repositories/index.ts': "export { Browser } from './catalog/Browser';",
  }));
  assert.equal(allowed.passed, true);
  const prohibited = await checkPolicy(await fixture(t, {
    'src/app/Workspace.tsx': "import { Browser } from '../features/repositories/catalog/Browser.tsx';",
    'src/features/history/index.ts': "export type { Selection } from '../diff/selection';",
    'src/features/history/model.ts': "const load = () => import('../diff/text/../selection.ts');",
  }));
  assert.deepEqual(prohibited.violations.map(item => item.rule),
    ['feature_dependency', 'feature_dependency', 'feature_dependency']);
});

test('nested native owners isolate host dependencies and workspace persistence', async t => {
  const allowed = await checkPolicy(await fixture(t, {
    'src-tauri/src/host/mod.rs': 'use tauri::Runtime;',
    'src-tauri/src/host/commands.rs': 'use tauri_plugin_dialog::DialogExt;',
    'src-tauri/src/workspace/persistence.rs': 'use std::{fs, path::Path};',
    'src-tauri/src/git/process.rs': 'use std::process::Command;',
  }));
  assert.equal(allowed.passed, true);
  const prohibited = await checkPolicy(await fixture(t, {
    'src-tauri/src/workspace/mod.rs': 'use std::{fs, process::Command};',
    'src-tauri/src/diff/rooted_read.rs': 'use tauri::Runtime;',
    'src-tauri/src/git/status/snapshot.rs': 'use tauri_plugin_dialog::DialogExt;',
    'src-tauri/src/browsing/mod.rs': 'fn invoke() { tauri::generate_context!(); }',
  }));
  assert.deepEqual(prohibited.violations.map(item => item.rule),
    ['native_dependency', 'native_dependency', 'native_dependency', 'native_dependency']);
});

test('directory entry imports cannot bypass lower-layer app boundaries', async t => {
  const result = await checkPolicy(await fixture(t, {
    'src/contracts/model.ts': "import type { Screen } from '../app';",
    'src/ui/Button.tsx': "export { Screen } from '../app';",
    'src/platform/client.ts': "const load = () => import('../app');",
  }));
  assert.deepEqual(result.violations.map(item => item.rule),
    ['contract_dependency', 'ui_dependency', 'platform_dependency']);
});

test('inline import types obey feature and shared-layer boundaries', async t => {
  const result = await checkPolicy(await fixture(t, {
    'src/features/history/model.ts': "type Selection = import('../diff/selection.ts').Selection;",
    'src/ui/Button.tsx': "type Screen = typeof import('../app');",
  }));
  assert.deepEqual(result.violations.map(item => item.rule), ['feature_dependency', 'ui_dependency']);
});

test('aliased native crates remain restricted to host', async t => {
  const result = await checkPolicy(await fixture(t, {
    'src-tauri/src/history/reader.rs': 'use tauri as desktop;',
    'src-tauri/src/inspection.rs': 'extern crate tauri_plugin_dialog as dialog;',
  }));
  assert.deepEqual(result.violations.map(item => item.rule), ['native_dependency', 'native_dependency']);
});

test('workspace identity metadata is allowed but qualified execution and filesystem calls are not', async t => {
  const allowed = await checkPolicy(await fixture(t, {
    'src-tauri/src/workspace/mod.rs': 'fn identity() -> u32 { std::process::id() }',
  }));
  assert.equal(allowed.passed, true);
  const prohibited = await checkPolicy(await fixture(t, {
    'src-tauri/src/workspace/mod.rs': 'fn execute() { std::process::Command::new("git").output(); }',
    'src-tauri/src/workspace/transition.rs': 'fn load() { std::fs::read("workspace"); }',
  }));
  assert.deepEqual(prohibited.violations.map(item => item.rule), ['native_dependency', 'native_dependency']);
});
