/** Verifies narrow GTK3 backports against independently pinned original crate archives. */
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstat, readFile, readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { isDeepStrictEqual } from 'node:util';

const approved = new Map([
  ['glib', { version: '0.18.5', checksum: '233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5' }],
  ['glib-macros', { version: '0.18.5', checksum: '0bb0228f477c0900c880fd78c8759b95c7636dbd7842707f49e132378aa2acdc' }],
  ['gtk3-macros', { version: '0.18.2', checksum: '52ff3c5b21f14f0736fed6dcfc0bfb4225ebf5725f3c0209edeec181e4d73e9d' }],
]);
// Exact reviewed outputs for the syn::Error migration, independent of patches.json.
// The original inputs are pinned by the archive checksums above. These full-file
// identities admit no additional source edits; every other file must match the archive.
const macroSources = new Map([
  ['glib-macros', new Map([
    ['src/boxed_derive.rs', '8324f0c5757126df97b1fc7dfc44e68d67ff0b0fb8e7e0649b19b686f7fc2cc4'],
    ['src/closure.rs', 'f8c9c8bb0206514ce7ad6bf9ac3cb457901dbeaf943023dcb0d55bbf0bc27938'],
    ['src/derived_properties_attribute.rs', '03e78e15633da02ead8715c72368c8893b975cdb40e77749a54577af5f07195c'],
    ['src/enum_derive.rs', '0eedd4040bde96467894b60ec7b903b2045794cacdcfb8b7ec613bd72aee7bd7'],
    ['src/error_domain_derive.rs', 'fbf2fe4bfd73c3d5ba3a8aeff1bab6ca8cc7a34ae9dc00185e348e950d1717d2'],
    ['src/flags_attribute.rs', 'e34a100e69b1b2e876b93e1d30bf952eef560d7785d4fc36a351e25963233bbf'],
    ['src/lib.rs', '4b397c6e6e65f0f4d82bf9831bf9d597c35d514e6b831b67600879afa7c1513b'],
    ['src/object_interface_attribute.rs', 'ee76ccb28a17c7b8a22d39fc171e63946d252e9fb11ab5c67ce014d2cf718195'],
    ['src/object_subclass_attribute.rs', '5397e9914175c1edb7551d2283cf30ab8b453bf088b97fd2ddeb5cd8e80b28fa'],
    ['src/shared_boxed_derive.rs', '51cc07450ffa6da1857ffbf34a3f75684eac6fed13422df5aeaa4d55f6bb67fa'],
    ['src/variant_derive.rs', '10722ea0d50afc976b1caf3dfde4aaf14e978c0b956cb6f41084af59ce76d24c'],
  ])],
  ['gtk3-macros', new Map([
    ['src/composite_template_derive.rs', '27b633e1747c912f1e2d9a85aaaa4208382c73481f57c48799c0baa4fadce105'],
    ['src/lib.rs', '6283f1e56c2dca0f4675bf1fb1b1c24a66128dd481abc97d52593dcfeb73186a'],
  ])],
]);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

function replaceExact(text, before, after, count = 1) {
  const parts = text.split(before);
  if (parts.length !== count + 1) throw new Error('vendor_patch_preimage');
  return parts.join(after);
}

/** Compares a file to its original bytes plus only the reviewed source transformation. */
export function verifyVendorFile({ name, path, originalBytes, localBytes }) {
  if (!approved.has(name)) throw new Error('unsupported_vendor_package');
  let expected = Buffer.from(originalBytes);
  if (name === 'glib' && path === 'src/variant_iter.rs') {
    // gtk-rs-core b5a4071e439bef2b5eea76c3aa25e5ae84839e34, RUSTSEC-2024-0429.
    let text = replaceExact(expected.toString('utf8'),
      '            let p: *mut libc::c_char = std::ptr::null_mut();',
      '            let mut p: *mut libc::c_char = std::ptr::null_mut();');
    text = replaceExact(text, '                &p,', '                &mut p,');
    expected = Buffer.from(text);
  } else if (macroSources.has(name)) {
    if (macroSources.get(name).has(path)) {
      if (hash(localBytes) !== macroSources.get(name).get(path)) {
        throw new Error(`vendor_unapproved_transformation:${name}/${path}`);
      }
      return;
    }
    if (path === 'Cargo.toml') {
      expected = Buffer.from(replaceExact(expected.toString('utf8'),
        '[dependencies.proc-macro-error]\nversion = "1.0"\n\n', ''));
    } else if (path === 'Cargo.toml.orig') {
      expected = Buffer.from(replaceExact(expected.toString('utf8'), 'proc-macro-error = "1.0"\n', ''));
    }
  }
  if (!expected.equals(localBytes)) throw new Error(`vendor_unapproved_transformation:${name}/${path}`);
}

async function sourceFiles(directory, prefix = '') {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isSymbolicLink()) throw new Error('vendor_symlink_source');
    if (entry.isDirectory()) result.push(...await sourceFiles(join(directory, entry.name), path));
    else if (entry.isFile()) result.push(path);
    else throw new Error('vendor_unsupported_file_type');
  }
  return result.sort();
}

function archiveCommand(archiveBytes, args) {
  return execFileSync('tar', args, {
    input: archiveBytes, maxBuffer: 32 * 1024 * 1024, stdio: ['pipe', 'pipe', 'pipe'],
  });
}

/**
 * Returns the complete local hash inventory only after checking the trusted archive and exact edits.
 * The caller supplies downloaded/cached bytes; no network, advisory suppression or CLI lives here.
 */
export async function verifyVendorPatch({ packageRecord, archiveBytes, root }) {
  const rule = approved.get(packageRecord.name);
  if (!rule || packageRecord.version !== rule.version) throw new Error('unsupported_vendor_package');
  const { name, version } = packageRecord;
  const path = `src-tauri/vendor/${name}`;
  const source = `https://static.crates.io/crates/${name}/${name}-${version}.crate`;
  if (packageRecord.path !== path || packageRecord.source !== source) throw new Error('vendor_origin_identity');
  if (packageRecord.archiveSha256 !== rule.checksum || hash(archiveBytes) !== rule.checksum) {
    throw new Error('vendor_origin_integrity');
  }

  // Refuse symlinked ancestors as well as entries so an admitted relative path cannot escape root.
  let directory = root;
  for (const part of path.split('/')) {
    directory = join(directory, part);
    const info = await lstat(directory);
    if (info.isSymbolicLink() || !info.isDirectory()) throw new Error('vendor_unsafe_directory');
  }
  const prefix = `${name}-${version}/`;
  // Archive bytes are independently pinned above; tar never extracts them onto the filesystem.
  const entries = archiveCommand(archiveBytes, ['-tzf', '-']).toString('utf8').trim().split(/\r?\n/)
    .filter(entry => !entry.endsWith('/'));
  const originalPaths = entries.map(entry => {
    if (!entry.startsWith(prefix)) throw new Error('vendor_archive_path');
    const relative = entry.slice(prefix.length);
    if (!relative || relative.includes('\\') || relative.split('/').some(part => !part || part === '.' || part === '..')) {
      throw new Error('vendor_archive_path');
    }
    return relative;
  }).sort();
  if (new Set(originalPaths).size !== originalPaths.length) throw new Error('vendor_duplicate_archive_path');
  if (!isDeepStrictEqual(await sourceFiles(directory), originalPaths)) throw new Error('vendor_source_file_set');

  const hashes = {};
  for (const relative of originalPaths) {
    const originalBytes = archiveCommand(archiveBytes, ['-xzOf', '-', `${prefix}${relative}`]);
    const localBytes = await readFile(join(directory, relative));
    verifyVendorFile({ name, path: relative, originalBytes, localBytes });
    hashes[relative] = hash(localBytes);
  }
  if (!isDeepStrictEqual(hashes, packageRecord.files)) throw new Error('vendor_manifest_file_hashes');
  return hashes;
}
