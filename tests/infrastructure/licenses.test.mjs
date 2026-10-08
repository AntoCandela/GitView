/** Exercise the same archive collector and vendored-source boundary used by refresh. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { collectArchiveNotices, deriveDiagnosticCommonJs, inspectMplSource, npmPackageEntries, verifyCanonicalGpl, verifyGlslGrammar, verifyVendoredSource } from '../../scripts/check-licenses.mjs';

test('canonical GPL verification uses the pinned identical GNU mirror when the primary source is unavailable', async () => {
  const license = await readFile(new URL('../../LICENSE', import.meta.url));
  const requested = [];
  const source = verifyCanonicalGpl(license, url => {
    requested.push(url);
    if (url === 'https://www.gnu.org/licenses/gpl-3.0.txt') throw new Error('fixture network unavailable');
    return license;
  });
  assert.equal(source, 'https://raw.githubusercontent.com/coreutils/coreutils/5b9d747261590ffde5f47fcf8cef06ee5bb5df63/COPYING');
  assert.equal(requested.length, 2);
});

test('canonical GPL verification rejects altered bytes without trying another source', async () => {
  const license = await readFile(new URL('../../LICENSE', import.meta.url));
  const requested = [];
  assert.throws(() => verifyCanonicalGpl(license, url => {
    requested.push(url);
    return Buffer.from('altered license');
  }), /authoritative_gpl_text_mismatch/);
  assert.equal(requested.length, 1);
});

test('canonical GPL verification rejects an altered mirror and unavailable sources', async () => {
  const license = await readFile(new URL('../../LICENSE', import.meta.url));
  assert.throws(() => verifyCanonicalGpl(license, url => {
    if (url === 'https://www.gnu.org/licenses/gpl-3.0.txt') throw new Error('unavailable');
    return Buffer.from('altered mirror');
  }), /authoritative_gpl_text_mismatch/);
  assert.throws(() => verifyCanonicalGpl(license, () => { throw new Error('unavailable'); }), /authoritative_gpl_source_unavailable/);
  assert.throws(() => verifyCanonicalGpl(Buffer.from('altered local'), () => {
    assert.fail('must validate local bytes before downloading');
  }), /authoritative_gpl_text_mismatch/);
});

test('canonical GPL verification records the primary source when it matches', async () => {
  const license = await readFile(new URL('../../LICENSE', import.meta.url));
  assert.equal(verifyCanonicalGpl(license, () => license), 'https://www.gnu.org/licenses/gpl-3.0.txt');
});

async function fixture(t, entries) {
  const root = await mkdtemp(join(tmpdir(), 'gitview-licenses-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const [path, bytes] of Object.entries(entries)) {
    await mkdir(dirname(join(root, path)), { recursive: true });
    await writeFile(join(root, path), bytes);
  }
  return root;
}

async function archiveFixture(t, entries) {
  const root = await fixture(t, entries);
  const archive = join(root, 'fixture.tgz');
  execFileSync('tar', ['-czf', archive, '-C', root, 'package']);
  return { archive, options: { component: 'npm/fixture/1.0.0', sourceUrl: 'https://example.invalid/fixture.tgz', outputRoot: root } };
}

const hash = bytes => createHash('sha256').update(bytes).digest('hex');

test('archive collector retains third-party notices and bundled JS license sidecars byte-for-byte', async t => {
  const thirdParty = Buffer.from('Original third-party attribution\r\n\x00UTF-8: é\r\n');
  const sidecar = Buffer.from('/*! Embedded package copyright and grant */\r\n');
  const { archive, options } = await archiveFixture(t, {
    'package/LICENSE': 'Wrapper grant\n',
    'package/ThirdPartyNotices.txt': thirdParty,
    'package/lib/bundle.js.LICENSE': sidecar,
    'package/lib/vendor.js.LICENSE.txt': 'Nested third-party grant\n',
    'package/lib/NOTICE.md': 'Nested notice\n',
    'package/lib/bundle.js': 'ordinary program',
    'package/NOTICE.svg': '<svg/>',
    'package/lib/copyright.mjs': 'ordinary icon source',
    'package/lib/copyright.mjs.map': '{"sources":["copyright.ts"]}',
  });

  const notices = await collectArchiveNotices(archive, options);

  assert.deepEqual(notices.map(notice => notice.sourcePath).sort(), [
    'LICENSE', 'ThirdPartyNotices.txt', 'lib/NOTICE.md', 'lib/bundle.js.LICENSE', 'lib/vendor.js.LICENSE.txt',
  ]);
  const original = notices.find(notice => notice.sourcePath === 'ThirdPartyNotices.txt');
  const bundled = notices.find(notice => notice.sourcePath === 'lib/bundle.js.LICENSE');
  assert.deepEqual(await readFile(join(options.outputRoot, original.path)), thirdParty);
  assert.deepEqual(await readFile(join(options.outputRoot, bundled.path)), sidecar);
  assert.equal(original.sha256, hash(thirdParty));
  assert.equal(bundled.sha256, hash(sidecar));
  assert.equal(original.sourceUrl, 'https://example.invalid/fixture.tgz#package/ThirdPartyNotices.txt');
});

test('archive collector leaves packages with no legal text unresolved', async t => {
  const { archive, options } = await archiveFixture(t, { 'package/index.js': 'module.exports = {};' });
  assert.deepEqual(await collectArchiveNotices(archive, options), []);
});

test('archive collector rejects empty legal evidence', async t => {
  const { archive, options } = await archiveFixture(t, { 'package/ThirdPartyNotices.txt': '' });
  await assert.rejects(collectArchiveNotices(archive, options), /empty_notice/);
});

test('archive collector rejects output traversal', async t => {
  const { archive, options } = await archiveFixture(t, { 'package/LICENSE': 'Original grant' });
  await assert.rejects(collectArchiveNotices(archive, { ...options, component: '../escape' }), /unsafe_notice_path/);
});

test('MPL source review distinguishes the license template from an attached incompatibility notice', async t => {
  const license = 'MPL license template: Incompatible With Secondary Licenses\n';
  const { archive } = await archiveFixture(t, {
    'package/LICENSE': license,
    'package/nested/LICENSE': ` ${license}`,
    'package/src/lib.rs': '// MPL-2.0 source\npub fn transform() {}\n',
  });
  const source = { archiveSha256: hash(await readFile(archive)), licenseSha256: hash(license) };
  const evidence = inspectMplSource(archive, source);
  assert.equal(evidence.incompatibleSecondaryLicense, false);
  assert.equal(evidence.filesReviewed, 3);
  assert.throws(() => inspectMplSource(archive, { ...source, archiveSha256: '0'.repeat(64) }), /mpl_source_integrity/);
  assert.throws(() => inspectMplSource(archive, { ...source, licenseSha256: '0'.repeat(64) }), /mpl_license_integrity/);

  const incompatible = await archiveFixture(t, {
    'package/LICENSE': license,
    'package/src/lib.rs': '// This Source Code Form is Incompatible With Secondary Licenses.\n',
  });
  assert.equal(inspectMplSource(incompatible.archive, { ...source, archiveSha256: hash(await readFile(incompatible.archive)) }).incompatibleSecondaryLicense, true);
});

test('MPL source review refuses archives with no source files', async t => {
  const license = 'MPL template\n';
  const { archive } = await archiveFixture(t, { 'package/LICENSE': license });
  const source = { archiveSha256: hash(await readFile(archive)), licenseSha256: hash(license) };
  assert.throws(() => inspectMplSource(archive, source), /mpl_source_missing/);
});

test('vendored evidence includes all files and rejects patched-source drift', async t => {
  const root = await fixture(t, { 'src-tauri/vendor/example/LICENSE': 'Original grant', 'src-tauri/vendor/example/src/lib.rs': 'pub fn patched() {}' });
  const rule = { path: 'src-tauri/vendor/example', files: { LICENSE: hash('Original grant'), 'src/lib.rs': hash('pub fn patched() {}') } };
  assert.deepEqual(await verifyVendoredSource(rule, root), rule.files);
  await writeFile(join(root, 'src-tauri/vendor/example/src/lib.rs'), 'pub fn changed() {}');
  await assert.rejects(verifyVendoredSource(rule, root), /vendor_source_integrity/);
});

test('vendored evidence rejects unrecorded files', async t => {
  const root = await fixture(t, { 'src-tauri/vendor/example/LICENSE': 'Original grant', 'src-tauri/vendor/example/unrecorded.rs': 'extra source' });
  const rule = { path: 'src-tauri/vendor/example', files: { LICENSE: hash('Original grant') } };
  await assert.rejects(verifyVendoredSource(rule, root), /vendor_source_integrity/);
});

test('vendored evidence rejects symbolic links rather than reading outside the source tree', async t => {
  const root = await fixture(t, { 'src-tauri/vendor/example/LICENSE': 'Original grant' });
  await symlink(join(root, 'src-tauri/vendor/example/LICENSE'), join(root, 'src-tauri/vendor/example/linked'));
  await assert.rejects(verifyVendoredSource({ path: 'src-tauri/vendor/example', files: {} }, root), /symlink_source/);
});

test('npm linked vendor packages are inventoried once at the physical lock record', () => {
  const target = { version: '3.2.2', dev: true, license: 'MIT' };
  const lock = { packages: {
    '': { name: 'app' },
    'node_modules/diagnostic': { resolved: 'vendor/diagnostic', link: true },
    'vendor/diagnostic': target,
  } };
  assert.deepEqual(npmPackageEntries(lock, { 'vendor/diagnostic': { name: 'diagnostic', version: '3.2.2' } }), [['vendor/diagnostic', target]]);
});

test('npm dangling links and package-identity substitutions fail closed', () => {
  assert.throws(() => npmPackageEntries({ packages: {
    'node_modules/diagnostic': { resolved: 'vendor/missing', link: true },
  } }), /unregistered_npm_link/);
  assert.throws(() => npmPackageEntries({ packages: {
    'node_modules/diagnostic': { resolved: 'vendor/other', link: true },
    'vendor/other': { version: '1.0.0' },
  } }, { 'vendor/other': { name: 'other', version: '1.0.0' } }), /npm_link_identity/);
});

test('diagnostic CommonJS adaptation preserves the original body and rejects unknown preimages', async () => {
  const original = await readFile(new URL('../../vendor/why-is-node-running/index.js', import.meta.url), 'utf8');
  const commonJs = await readFile(new URL('../../vendor/why-is-node-running/index.cjs', import.meta.url), 'utf8');
  assert.equal(deriveDiagnosticCommonJs(original), commonJs);
  assert.throws(() => deriveDiagnosticCommonJs(original.replace('node:async_hooks', 'node:fs')), /npm_vendor_patch_preimage/);
});

test('GLSL replacement accepts the exact XML conversion and rejects grammar-body changes', async t => {
  const xml = await readFile(new URL('../../src/assets/grammars/GLSLX.tmLanguage', import.meta.url));
  const grammar = await readFile(new URL('../../src/assets/grammars/glsl.json', import.meta.url), 'utf8');
  const root = await fixture(t, {
    'src/assets/grammars/GLSLX.tmLanguage': xml,
    'src/assets/grammars/glsl.json': grammar,
  });
  await verifyGlslGrammar(root);
  const changed = JSON.parse(grammar);
  changed.patterns = [];
  await writeFile(join(root, 'src/assets/grammars/glsl.json'), JSON.stringify(changed));
  await assert.rejects(verifyGlslGrammar(root), /glsl_grammar_derivation/);
});
