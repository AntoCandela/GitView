/** Checks the exact-edit and independently pinned archive boundary used by the license gate. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { verifyVendorFile, verifyVendorPatch } from '../../scripts/vendor-patches.mjs';

const originalIterator = Buffer.from([
  '    fn impl_get(&self, i: usize) -> &str {',
  '            let p: *mut libc::c_char = std::ptr::null_mut();',
  '            ffi::g_variant_get_child(',
  '                &p,',
  '            );',
  '    }',
].join('\n'));
const patchedIterator = Buffer.from([
  '    fn impl_get(&self, i: usize) -> &str {',
  '            let mut p: *mut libc::c_char = std::ptr::null_mut();',
  '            ffi::g_variant_get_child(',
  '                &mut p,',
  '            );',
  '    }',
].join('\n'));
const iteratorFile = { name: 'glib', path: 'src/variant_iter.rs', originalBytes: originalIterator };
const glibRecord = {
  name: 'glib', version: '0.18.5', path: 'src-tauri/vendor/glib',
  source: 'https://static.crates.io/crates/glib/glib-0.18.5.crate',
  archiveSha256: '233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5',
  files: {},
};

test('GLib accepts exactly the upstream output-pointer mutability correction', () => {
  assert.doesNotThrow(() => verifyVendorFile({ ...iteratorFile, localBytes: patchedIterator }));
});

test('GLib rejects unchanged vulnerable code and partial fixes', () => {
  assert.throws(() => verifyVendorFile({ ...iteratorFile, localBytes: originalIterator }), /vendor_unapproved_transformation/);
  const partialFix = Buffer.from(originalIterator.toString().replace('let p:', 'let mut p:'));
  assert.throws(() => verifyVendorFile({ ...iteratorFile, localBytes: partialFix }), /vendor_unapproved_transformation/);
});

test('extra source changes cannot accompany an otherwise correct backport', () => {
  const unrelatedChange = Buffer.concat([patchedIterator, Buffer.from('\npub fn unrelated() {}\n')]);
  assert.throws(() => verifyVendorFile({ ...iteratorFile, localBytes: unrelatedChange }), /vendor_unapproved_transformation/);
});

test('ambiguous or missing original expressions fail closed', () => {
  assert.throws(() => verifyVendorFile({
    ...iteratorFile, originalBytes: Buffer.concat([originalIterator, originalIterator]), localBytes: patchedIterator,
  }), /vendor_patch_preimage/);
  assert.throws(() => verifyVendorFile({
    ...iteratorFile, originalBytes: patchedIterator, localBytes: patchedIterator,
  }), /vendor_patch_preimage/);
});

test('macro dependency migration leaves all other manifest bytes unchanged', () => {
  const originalBytes = Buffer.from('[dependencies.proc-macro-error]\nversion = "1.0"\n\n[dependencies.syn]\nversion = "2.0"\n');
  const localBytes = Buffer.from('[dependencies.syn]\nversion = "2.0"\n');
  assert.doesNotThrow(() => verifyVendorFile({ name: 'gtk3-macros', path: 'Cargo.toml', originalBytes, localBytes }));
  assert.throws(() => verifyVendorFile({
    name: 'gtk3-macros', path: 'Cargo.toml', originalBytes,
    localBytes: Buffer.from(localBytes.toString().replace('version = "2.0"', 'version = "3.0"')),
  }), /vendor_unapproved_transformation/);
  assert.doesNotThrow(() => verifyVendorFile({
    name: 'glib-macros', path: 'Cargo.toml.orig',
    originalBytes: Buffer.from('proc-macro-error = "1.0"\n'),
    localBytes: Buffer.alloc(0),
  }));
});

test('reviewed syn diagnostic source cannot be replaced by another self-consistent file', async () => {
  const localBytes = await readFile(new URL('../../src-tauri/vendor/gtk3-macros/src/lib.rs', import.meta.url));
  const originalBytes = Buffer.from('original bytes are independently authenticated by the outer archive boundary');
  assert.doesNotThrow(() => verifyVendorFile({ name: 'gtk3-macros', path: 'src/lib.rs', originalBytes, localBytes }));
  const alteredBytes = Buffer.concat([localBytes, Buffer.from('\nfn unrelated() {}\n')]);
  assert.throws(() => verifyVendorFile({
    name: 'gtk3-macros', path: 'src/lib.rs', originalBytes: alteredBytes, localBytes: alteredBytes,
  }), /vendor_unapproved_transformation/);
  assert.throws(() => verifyVendorFile({ name: 'gtk3-macros', path: 'src/other.rs', originalBytes, localBytes }), /vendor_unapproved_transformation/);
});

test('unmodified files preserve every byte including original grants', () => {
  const originalBytes = Buffer.from('Original grant\r\n\x00');
  assert.doesNotThrow(() => verifyVendorFile({ name: 'glib', path: 'LICENSE', originalBytes, localBytes: originalBytes }));
  assert.throws(() => verifyVendorFile({ name: 'glib', path: 'LICENSE', originalBytes, localBytes: Buffer.from('Replacement grant') }), /vendor_unapproved_transformation/);
});

test('self-consistent forged archive and manifest checksums cannot redefine the trusted origin', async () => {
  const archiveBytes = Buffer.from('not the published archive');
  const archiveSha256 = createHash('sha256').update(archiveBytes).digest('hex');
  await assert.rejects(verifyVendorPatch({
    packageRecord: { ...glibRecord, archiveSha256 }, archiveBytes, root: '.',
  }), /vendor_origin_integrity/);
  await assert.rejects(verifyVendorPatch({ packageRecord: glibRecord, archiveBytes, root: '.' }), /vendor_origin_integrity/);
});

test('unknown versions and redirected source identities fail before filesystem access', async () => {
  await assert.rejects(verifyVendorPatch({
    packageRecord: { ...glibRecord, version: '0.18.6' }, archiveBytes: Buffer.alloc(0), root: '.',
  }), /unsupported_vendor_package/);
  await assert.rejects(verifyVendorPatch({
    packageRecord: { ...glibRecord, path: 'src-tauri/vendor/../other' }, archiveBytes: Buffer.alloc(0), root: '.',
  }), /vendor_origin_identity/);
  await assert.rejects(verifyVendorPatch({
    packageRecord: { ...glibRecord, source: 'https://example.invalid/glib.crate' }, archiveBytes: Buffer.alloc(0), root: '.',
  }), /vendor_origin_identity/);
});
