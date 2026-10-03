/** Inventories exact locked archives and retained legal texts; unknown provenance never becomes clearance. */
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { readFile, writeFile, mkdir, readdir, mkdtemp, rm } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { tmpdir, homedir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { verifyVendorPatch } from './vendor-patches.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const inventoryPath = join(root, 'licenses/inventory.json');
const canonicalGplSha256 = '3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986';
const legalName = /^(?!.*\.(?:svg|png|jpe?g|ico|icns|woff2?|ttf|otf)$)(?:(?:licen[cs]es?|copying|copyright|notices?|authors|third[._ -]?party[._ -]?(?:notices?|licen[cs]es?))(?:[._-].*)?|.+\.licen[cs]e(?:[._-].*)?)$/i;
const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');
const json = path => readFile(join(root, path), 'utf8').then(JSON.parse);
const trees = new Map();
let sourceArchivesDirectory;

function command(name, args) {
  return execFileSync(name, args, { cwd: root, maxBuffer: 128 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
}
function download(url) {
  if (!/^https:\/\//.test(url)) throw new Error('non_https_source');
  return command('curl', ['--fail', '--silent', '--show-error', '--location', '--retry', '2', '--max-time', '180', url]);
}
async function files(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isSymbolicLink()) throw new Error('symlink_source');
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await files(path));
    else if (entry.isFile()) result.push(path);
  }
  return result.sort();
}
async function retain(component, sourcePath, bytes, sourceUrl, outputRoot = root) {
  const name = sourcePath.replaceAll('\\', '/');
  if (name.startsWith('/') || name.split('/').includes('..') || component.startsWith('/') || component.split('/').includes('..')) throw new Error('unsafe_notice_path');
  if (!bytes.length) throw new Error('empty_notice');
  const path = `licenses/texts/${component}/${name}`;
  await mkdir(dirname(join(outputRoot, path)), { recursive: true });
  await writeFile(join(outputRoot, path), bytes);
  return { path, sourcePath: name, sourceUrl, sha256: sha256(bytes) };
}
export async function collectArchiveNotices(archive, { component, sourceUrl, outputRoot = root }) {
  const entries = command('tar', ['-tzf', archive]).toString().trim().split('\n');
  const notices = [];
  for (const entry of entries.filter(entry => !entry.endsWith('/') && legalName.test(basename(entry)))) {
    const sourcePath = entry.split('/').slice(1).join('/');
    notices.push(await retain(component, sourcePath, command('tar', ['-xzOf', archive, entry]), `${sourceUrl}#${entry}`, outputRoot));
  }
  return notices;
}
function selection(expression) {
  if (typeof expression !== 'string') return null;
  const normalized = expression.replaceAll('/', ' OR ').replace(/\s+/g, ' ').trim().replace(/^\(([^()]+)\)$/, '$1');
  const admitted = new Set(['MIT', 'MIT-0', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', '0BSD', 'ISC', 'Zlib', 'Unlicense', 'Unicode-3.0', 'OFL-1.1', 'MPL-2.0', 'CC0-1.0', 'Apache-2.0 WITH LLVM-exception']);
  if (normalized === '(MIT OR Apache-2.0) AND Unicode-3.0') return 'MIT AND Unicode-3.0';
  if (normalized.includes(' AND ')) return normalized.split(' AND ').every(atom => admitted.has(atom)) ? normalized : null;
  if (normalized.includes(' OR ')) {
    const choices = normalized.split(' OR ');
    if (choices.some(atom => !admitted.has(atom) && atom !== 'LGPL-2.1-or-later')) return null;
    return ['MIT', 'Apache-2.0', 'MIT-0', 'Apache-2.0 WITH LLVM-exception', '0BSD', 'BSD-3-Clause', 'Zlib', 'Unlicense', 'CC0-1.0'].find(choice => choices.includes(choice)) ?? null;
  }
  return admitted.has(normalized) ? normalized : null;
}
function selectedLicense(pkg) {
  if (pkg.ecosystem === 'npm' && pkg.name === 'lucide-react' && pkg.version === '1.49.0' && pkg.declaredLicense === 'ISC') return 'ISC AND MIT';
  if (pkg.ecosystem === 'cargo' && pkg.name === 'brotli-decompressor' && pkg.version === '6.0.1' && pkg.declaredLicense === 'BSD-3-Clause/MIT') return 'BSD-3-Clause AND MIT';
  if (pkg.ecosystem === 'npm' && pkg.name === '@shikijs/engine-oniguruma' && pkg.version === '4.0.2' && pkg.declaredLicense === 'MIT') return 'MIT AND BSD-2-Clause';
  if (pkg.ecosystem === 'npm' && pkg.name === '@shikijs/langs' && pkg.version === '4.0.2' && pkg.declaredLicense === 'MIT') return 'MIT AND LicenseRef-TextMate-Bundle';
  return selection(pkg.declaredLicense);
}
async function upstreamTexts(pkg, directory, component, providedVcs) {
  let vcs = providedVcs;
  if (!vcs) {
    try { vcs = JSON.parse(await readFile(join(directory, '.cargo_vcs_info.json'), 'utf8')); } catch { return []; }
  }
  const match = /^https:\/\/github.com\/([^/]+\/[^/#]+?)(?:\.git)?\/?$/.exec(pkg.repository ?? '');
  const commit = vcs.git?.sha1;
  if (!match || !/^[a-f0-9]{40}$/.test(commit ?? '')) return [];
  const repository = match[1];
  const key = `${repository}@${commit}`;
  if (!trees.has(key)) {
    const archivePath = join(sourceArchivesDirectory, `${sha256(key)}.tgz`);
    await writeFile(archivePath, download(`https://codeload.github.com/${repository}/tar.gz/${commit}`));
    const entries = command('tar', ['-tzf', archivePath]).toString().trim().split('\n').filter(entry => !entry.endsWith('/'));
    trees.set(key, { archivePath, entries: entries.map(entry => ({ path: entry.split('/').slice(1).join('/'), archiveEntry: entry })) });
  }
  const tree = trees.get(key);
  const packagePath = vcs.path_in_vcs ?? '';
  const ancestors = new Set(['']);
  let parent = packagePath;
  while (parent && parent !== '.') { ancestors.add(parent); parent = dirname(parent); }
  const candidates = tree.entries.filter(entry => legalName.test(basename(entry.path)) && ancestors.has(dirname(entry.path) === '.' ? '' : dirname(entry.path)));
  const notices = [];
  for (const entry of candidates) {
    const url = `https://raw.githubusercontent.com/${repository}/${commit}/${entry.path}`;
    const bytes = command('tar', ['-xzOf', tree.archivePath, entry.archiveEntry]);
    if (bytes.length) notices.push(await retain(component, `upstream/${entry.path}`, bytes, url));
  }
  return notices;
}
export function npmPackageEntries(lock, localManifests) {
  for (const [path, info] of Object.entries(lock.packages)) {
    if (!info.link) continue;
    if (!info.resolved?.startsWith('vendor/') || info.resolved.includes('\\') || info.resolved.split('/').includes('..') || !lock.packages[info.resolved] || lock.packages[info.resolved].link) throw new Error('unregistered_npm_link');
    const manifest = localManifests?.[info.resolved] ?? JSON.parse(readFileSync(join(root, info.resolved, 'package.json'), 'utf8'));
    if (path.split('node_modules/').at(-1) !== manifest.name || lock.packages[info.resolved].version !== manifest.version) throw new Error('npm_link_identity');
  }
  return Object.entries(lock.packages).filter(([path, info]) => path && !info.link);
}
export function deriveDiagnosticCommonJs(source) {
  let result = source;
  for (const [name, module] of [['createHook', 'async_hooks'], ['readFileSync', 'fs'], ['relative', 'path'], ['fileURLToPath', 'url']]) {
    const before = `import { ${name} } from 'node:${module}'`;
    if (result.split(before).length !== 2) throw new Error('npm_vendor_patch_preimage');
    result = result.replace(before, `const { ${name} } = require('node:${module}')`);
  }
  const before = 'export default function whyIsNodeRunning';
  if (result.split(before).length !== 2) throw new Error('npm_vendor_patch_preimage');
  return result.replace(before, 'module.exports = function whyIsNodeRunning');
}
async function verifyNpmVendor(rule) {
  const source = 'https://registry.npmjs.org/why-is-node-running/-/why-is-node-running-3.2.2.tgz';
  const checksum = 'feaedd03cd2c28d3753f1bd0492c2ffe981677b1672331f607c59ce5f9aed9af';
  if (rule.name !== 'why-is-node-running' || rule.version !== '3.2.2' || rule.path !== 'vendor/why-is-node-running' || rule.source !== source || rule.archiveSha256 !== checksum) throw new Error('unsupported_npm_vendor_patch');
  const archive = download(source);
  if (sha256(archive) !== checksum) throw new Error('vendor_origin_integrity');
  const tar = args => execFileSync('tar', args, { input: archive, maxBuffer: 16 * 1024 * 1024 });
  const entries = tar(['-tzf', '-']).toString().trim().split('\n').filter(entry => !entry.endsWith('/'));
  const expected = new Map(entries.map(entry => [entry.replace(/^package\//, ''), tar(['-xzOf', '-', entry])]));
  const manifest = JSON.parse(expected.get('package.json').toString());
  manifest.exports['.'] = { types: './index.d.ts', require: './index.cjs', default: './index.js' };
  manifest.files.splice(manifest.files.indexOf('index.js'), 0, 'index.cjs');
  expected.set('package.json', Buffer.from(`${JSON.stringify(manifest, null, 2)}\n`));
  expected.set('index.cjs', Buffer.from(deriveDiagnosticCommonJs(expected.get('index.js').toString())));
  const hashes = await verifyVendoredSource(rule);
  if (!isDeepStrictEqual(Object.keys(hashes).sort(), [...expected.keys()].sort())) throw new Error('npm_vendor_file_set');
  for (const [path, bytes] of expected) {
    if (!(await readFile(join(root, rule.path, path))).equals(bytes)) throw new Error('npm_vendor_patch_bytes');
  }
  return hashes;
}
async function vendoredNpmPackage(path, info) {
  const sourcePath = info.resolved?.startsWith('file:') ? info.resolved.slice(5) : path;
  const rule = (await vendorRules('npm')).find(rule => rule.path === sourcePath);
  if (!rule) throw new Error('unregistered_npm_path_dependency');
  const sourceFiles = await verifyNpmVendor(rule);
  const manifest = await json(`${rule.path}/package.json`);
  if (manifest.name !== rule.name || manifest.version !== rule.version || info.version !== rule.version) throw new Error('npm_manifest_identity');
  const notices = [];
  for (const sourcePath of Object.keys(sourceFiles).filter(path => legalName.test(basename(path)))) {
    notices.push(await retain(`npm/${rule.name}/${rule.version}`, sourcePath, await readFile(join(root, rule.path, sourcePath)), `${rule.path}/${sourcePath}`));
  }
  return {
    ecosystem: 'npm', name: rule.name, version: rule.version, lockPath: path,
    source: rule.source, sourceKind: 'vendored', sourcePath: rule.path, sourceFiles,
    archiveSha256: rule.archiveSha256, repository: manifest.repository ?? null,
    scope: info.dev ? ['development/build/test'] : ['runtime'], optional: Boolean(info.optional),
    os: info.os ?? [], cpu: info.cpu ?? [], declaredLicense: info.license ?? null,
    upstreamLicense: manifest.license ?? null, selectedLicense: selection(info.license), notices,
  };
}
async function npmInventory(lock, temporary) {
  const packages = [];
  for (const [path, info] of npmPackageEntries(lock)) {
    if (path.startsWith('vendor/') || info.resolved?.startsWith('file:')) {
      packages.push(await vendoredNpmPackage(path, info));
      continue;
    }
    const name = path.split('node_modules/').at(-1);
    const component = `npm/${name}/${info.version}`;
    const bytes = download(info.resolved);
    const integrity = info.integrity?.split(' ').find(value => value.startsWith('sha512-')) ?? info.integrity?.split(' ')[0];
    const [algorithm, expected] = (integrity ?? '').split('-');
    if (!algorithm || createHash(algorithm).update(bytes).digest('base64') !== expected) throw new Error('npm_archive_integrity');
    const archive = join(temporary, 'package.tgz');
    await writeFile(archive, bytes);
    const entries = command('tar', ['-tzf', archive]).toString().trim().split('\n');
    const manifestEntry = entries.find(entry => entry === 'package/package.json') ?? entries.find(entry => /^[^/]+\/package.json$/.test(entry));
    if (!manifestEntry) throw new Error('npm_manifest_missing');
    const manifest = JSON.parse(command('tar', ['-xzOf', archive, manifestEntry]).toString());
    if (manifest.name !== name || manifest.version !== info.version) throw new Error('npm_manifest_identity');
    const notices = await collectArchiveNotices(archive, { component, sourceUrl: info.resolved });
    const repository = typeof manifest.repository === 'string' ? manifest.repository : manifest.repository?.url;
    if (!notices.length && !/^@(esbuild|rollup)\//.test(name) && !name.startsWith('@tauri-apps/cli-')) {
      try {
        const registry = JSON.parse(download(`https://registry.npmjs.org/${encodeURIComponent(name)}/${info.version}`));
        const upstreamRepository = (typeof registry.repository === 'string' ? registry.repository : registry.repository?.url)?.replace(/^git\+/, '').replace(/^git:\/\//, 'https://');
        notices.push(...await upstreamTexts({ repository: upstreamRepository }, null, component, { git: { sha1: registry.gitHead }, path_in_vcs: registry.repository?.directory ?? '' }));
      } catch { /* Missing upstream evidence stays unresolved, rather than fabricating a copyright grant. */ }
    }
    const scope = info.dev ? ['development/build/test'] : ['runtime'];
    if (name === 'material-icon-theme') scope.push('bundled-icon-assets');
    packages.push({ ecosystem: 'npm', name, version: info.version, lockPath: path, source: info.resolved, integrity: info.integrity, archiveSha256: sha256(bytes), repository: repository ?? null, scope, optional: Boolean(info.optional), os: info.os ?? [], cpu: info.cpu ?? [], declaredLicense: info.license ?? null, upstreamLicense: manifest.license ?? null, selectedLicense: name === 'lucide-react' && info.version === '1.49.0' ? 'ISC AND MIT' : selection(info.license), notices });
  }
  return packages;
}
function cargoScopes(metadata) {
  const nodes = new Map(metadata.resolve.nodes.map(node => [node.id, node]));
  const procMacros = new Set(metadata.packages.filter(pkg => pkg.targets.some(target => target.kind.includes('proc-macro'))).map(pkg => pkg.id));
  const scopes = new Map();
  const queue = [{ id: metadata.resolve.root, scope: 'runtime' }];
  const visited = new Set();
  for (let index = 0; index < queue.length; index += 1) {
    const { id, scope } = queue[index];
    if (visited.has(`${id}:${scope}`)) continue;
    visited.add(`${id}:${scope}`);
    if (!scopes.has(id)) scopes.set(id, new Set());
    scopes.get(id).add(scope);
    for (const dependency of nodes.get(id)?.deps ?? []) {
      for (const kind of dependency.dep_kinds) {
        queue.push({ id: dependency.pkg, scope: scope === 'development/test' || kind.kind === 'dev' ? 'development/test' : scope === 'build' || kind.kind === 'build' || procMacros.has(dependency.pkg) ? 'build' : 'runtime' });
      }
    }
  }
  return scopes;
}
async function vendorRules(ecosystem = 'cargo') {
  const manifest = await json(ecosystem === 'cargo' ? 'src-tauri/vendor/patches.json' : 'vendor/patches.json');
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.packages)) throw new Error('vendor_manifest_schema');
  return manifest.packages;
}
export async function verifyVendoredSource(rule, sourceRoot = root) {
  if (!(rule.path?.startsWith('src-tauri/vendor/') || rule.path?.startsWith('vendor/')) || rule.path.includes('\\') || rule.path.split('/').includes('..')) throw new Error('unsafe_vendor_path');
  const directory = join(sourceRoot, rule.path);
  const hashes = Object.fromEntries(await Promise.all((await files(directory)).map(async path => [
    relative(directory, path).replaceAll('\\', '/'), sha256(await readFile(path)),
  ])));
  if (!isDeepStrictEqual(hashes, rule.files)) throw new Error('vendor_source_integrity');
  return hashes;
}
async function cargoOriginArchive(rule) {
  const cache = join(process.env.CARGO_HOME ?? join(homedir(), '.cargo'), 'registry/cache');
  let registries;
  try { registries = await readdir(cache); } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    registries = [];
  }
  for (const registry of registries) {
    try {
      const bytes = await readFile(join(cache, registry, `${rule.name}-${rule.version}.crate`));
      if (sha256(bytes) !== rule.archiveSha256) throw new Error('vendor_origin_integrity');
      return bytes;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  return download(rule.source);
}
async function verifyCargoVendor(rule) {
  const source = `https://static.crates.io/crates/${rule.name}/${rule.name}-${rule.version}.crate`;
  if (rule.source !== source) throw new Error('vendor_origin_identity');
  const archiveBytes = await cargoOriginArchive(rule);
  return verifyVendorPatch({ packageRecord: rule, archiveBytes, root });
}
async function vendoredCargoPackage(pkg, scopes, metadata) {
  const rule = (await vendorRules()).find(rule => rule.name === pkg.name && rule.version === pkg.version);
  if (!rule || resolve(root, rule.path) !== dirname(pkg.manifest_path)) throw new Error('unregistered_cargo_path_dependency');
  const sourceFiles = await verifyCargoVendor(rule);
  const source = rule.source;
  const notices = [];
  for (const path of Object.keys(sourceFiles).filter(path => legalName.test(basename(path)))) {
    notices.push(await retain(`cargo/${pkg.name}/${pkg.version}`, path, await readFile(join(root, rule.path, path)), `${rule.path}/${path}`));
  }
  return {
    ecosystem: 'cargo', name: pkg.name, version: pkg.version, source, sourceKind: 'vendored',
    sourcePath: rule.path, archiveSha256: rule.archiveSha256, sourceFiles, repository: pkg.repository,
    scope: [...(scopes.get(pkg.id) ?? ['locked-not-in-resolved-graph'])].sort(),
    resolvedFeatures: metadata.resolve.nodes.find(node => node.id === pkg.id)?.features ?? [],
    targetConditions: [...new Set(metadata.packages.flatMap(parent => parent.dependencies.filter(dep => dep.name === pkg.name).map(dep => dep.target).filter(Boolean)))].sort(),
    declaredLicense: pkg.license, selectedLicense: selection(pkg.license), notices,
  };
}
async function cargoInventory() {
  const metadata = JSON.parse(command('cargo', ['metadata', '--manifest-path', 'src-tauri/Cargo.toml', '--locked', '--format-version', '1']));
  const scopes = cargoScopes(metadata);
  const packages = [];
  for (const pkg of metadata.packages.filter(pkg => pkg.id !== metadata.resolve.root).sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`))) {
    if (!pkg.source) {
      packages.push(await vendoredCargoPackage(pkg, scopes, metadata));
      continue;
    }
    if (!pkg.source.startsWith('registry+')) throw new Error('unsupported_cargo_source');
    const directory = dirname(pkg.manifest_path);
    const registry = basename(dirname(directory));
    const cargoHome = process.env.CARGO_HOME ?? join(homedir(), '.cargo');
    const archivePath = join(cargoHome, 'registry/cache', registry, `${pkg.name}-${pkg.version}.crate`);
    const archive = await readFile(archivePath);
    const checksum = sha256(archive);
    const lockEntry = cargoLockEntries().find(entry => entry.name === pkg.name && entry.version === pkg.version && entry.source === pkg.source);
    if (lockEntry?.checksum !== checksum) throw new Error('cargo_archive_integrity');
    const component = `cargo/${pkg.name}/${pkg.version}`;
    const source = `https://static.crates.io/crates/${pkg.name}/${pkg.name}-${pkg.version}.crate`;
    const entries = command('tar', ['-tzf', archivePath]).toString().trim().split('\n');
    const notices = await collectArchiveNotices(archivePath, { component, sourceUrl: source });
    if (!notices.length) {
      try { notices.push(...await upstreamTexts(pkg, directory, component)); } catch { /* Missing upstream evidence remains a blocker in the inventory. */ }
    }
    if (pkg.name === 'libsqlite3-sys' || pkg.name === 'sqlite-wasm-rs') {
      for (const entry of entries.filter(entry => basename(entry) === 'sqlite3.c')) {
        const bytes = command('tar', ['-xzOf', archivePath, entry]);
        const dedication = bytes.indexOf(Buffer.from('The author disclaims copyright'));
        const start = dedication >= 0 ? bytes.lastIndexOf(Buffer.from('/*'), dedication) : -1;
        const end = start >= 0 ? bytes.indexOf(Buffer.from('*/'), dedication) : -1;
        const sourcePath = entry.split('/').slice(1).join('/');
        if (start >= 0 && end >= 0) notices.push(await retain(component, `${sourcePath}.public-domain-header.txt`, bytes.subarray(start, end + 2), `${source}#${sourcePath}`));
      }
    }
    if (pkg.name === 'brotli-decompressor' && pkg.version === '6.0.1') {
      const entry = entries.find(entry => entry.endsWith('/src/context.rs'));
      const bytes = command('tar', ['-xzOf', archivePath, entry]);
      const start = bytes.indexOf(Buffer.from('// Copyright 2013 Google'));
      const end = bytes.indexOf(Buffer.from('\n\n'), start);
      if (start < 0 || end < 0) throw new Error('brotli_embedded_header_missing');
      notices.push(await retain(component, 'context-copyright-header.txt', bytes.subarray(start, end + 2), `${source}#src/context.rs`));
    }
    if (pkg.name === 'vswhom-sys' && pkg.version === '0.1.3') {
      const entry = entries.find(entry => entry.endsWith('/ext/vswhom.cpp'));
      if (!entry) throw new Error('vswhom_embedded_source_missing');
      const bytes = command('tar', ['-xzOf', archivePath, entry]);
      const end = bytes.indexOf(Buffer.from('#include <windows.h>'));
      const header = bytes.subarray(0, end);
      if (end < 0 || !header.includes(Buffer.from('Author:   Jonathan Blow')) || !header.includes(Buffer.from('This code is released under the MIT license'))) throw new Error('vswhom_embedded_header_missing');
      notices.push(await retain(component, 'ext/vswhom-original-mit-header.txt', header, `${source}#ext/vswhom.cpp`));
    }
    const incompatibleSecondaryLicense = pkg.license?.includes('MPL-2.0')
      ? entries.filter(entry => entry.endsWith('.rs')).some(entry => command('tar', ['-xzOf', archivePath, entry]).toString().includes('Incompatible With Secondary Licenses'))
      : false;
    packages.push({ ecosystem: 'cargo', name: pkg.name, version: pkg.version, source, checksum, repository: pkg.repository, scope: [...(scopes.get(pkg.id) ?? ['locked-not-in-resolved-graph'])].sort(), resolvedFeatures: metadata.resolve.nodes.find(node => node.id === pkg.id)?.features ?? [], targetConditions: [...new Set(metadata.packages.flatMap(parent => parent.dependencies.filter(dep => dep.name === pkg.name).map(dep => dep.target).filter(Boolean)))].sort(), declaredLicense: pkg.license, selectedLicense: selection(pkg.license), incompatibleSecondaryLicense, notices });
  }
  return packages;
}
function cargoLockEntries() {
  const text = readFileSync(join(root, 'src-tauri/Cargo.lock'), 'utf8');
  return text.split('[[package]]').slice(1).map(block => Object.fromEntries([...block.matchAll(/^(name|version|source|checksum) = "([^"]+)"$/gm)].map(match => [match[1], match[2]])));
}
function cargoDependencyLockEntries() {
  const manifest = readFileSync(join(root, 'src-tauri/Cargo.toml'), 'utf8').split(/^\[/m).find(section => section.startsWith('package]'));
  const name = /^name = "([^"]+)"$/m.exec(manifest ?? '')?.[1];
  const version = /^version = "([^"]+)"$/m.exec(manifest ?? '')?.[1];
  if (!name || !version) throw new Error('cargo_project_identity');
  return cargoLockEntries().filter(pkg => pkg.source || pkg.name !== name || pkg.version !== version);
}
async function supplementalNotices(packages) {
  for (const rule of (await json('licenses/supplemental-sources.json')).rules) {
    for (const pkg of packages.filter(pkg => pkg.ecosystem === rule.ecosystem && pkg.version === rule.version && (rule.name ? pkg.name === rule.name : pkg.name.startsWith(rule.namePrefix)))) {
      pkg.noticeEvidence = rule.evidence;
      if (rule.referenceParent) {
        const parent = packages.find(parent => parent.ecosystem === pkg.ecosystem && parent.name === rule.referenceParent && parent.version === (rule.parentVersion ?? pkg.version));
        if (!parent?.notices.length) throw new Error('supplemental_parent_missing');
        pkg.notices.push(...parent.notices);
      }
      for (const source of rule.sources ?? []) {
        pkg.notices.push(await retain(`${pkg.ecosystem}/${pkg.name}/${pkg.version}`, source.path, download(source.url), source.url));
      }
    }
  }
}
async function legalTextHashes() {
  return Object.fromEntries(await Promise.all((await files(join(root, 'licenses/texts'))).map(async path => [relative(root, path).replaceAll('\\', '/'), sha256(await readFile(path))])));
}
async function sourceHashes() {
  const paths = ['package.json', 'package-lock.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock', 'scripts/check-licenses.mjs', 'scripts/fileIconThemes.ts', 'licenses/supplemental-sources.json', 'CODE_OF_CONDUCT.md', 'THIRD_PARTY_NOTICES.md'];
  paths.push('src/features/diff/highlighting/highlighting.worker.ts', 'scripts/generate-brand.mjs', 'public/favicon.svg', 'scripts/glslGrammar.ts', 'vite.config.ts', 'scripts/vendor-patches.mjs');
  for (const directory of ['src/assets', 'src-tauri/icons', '.agents/skills', 'src-tauri/vendor', 'vendor']) {
    paths.push(...(await files(join(root, directory))).map(path => relative(root, path).replaceAll('\\', '/')));
  }
  return Object.fromEntries(await Promise.all(paths.sort().map(async path => [path, sha256(await readFile(join(root, path)))])));
}
function plistValue(element) {
  if (element.localName === 'string') return element.textContent;
  if (element.localName === 'array') return [...element.children].map(plistValue);
  if (element.localName !== 'dict' || element.children.length % 2 !== 0) throw new Error('glsl_plist_shape');
  const result = {};
  const children = [...element.children];
  for (let index = 0; index < children.length; index += 2) {
    const key = children[index];
    if (key.localName !== 'key' || Object.hasOwn(result, key.textContent)) throw new Error('glsl_plist_key');
    Object.defineProperty(result, key.textContent, { value: plistValue(children[index + 1]), enumerable: true, writable: true });
  }
  return result;
}
export async function verifyGlslGrammar(sourceRoot = root) {
  const { JSDOM } = await import('jsdom');
  const xml = await readFile(join(sourceRoot, 'src/assets/grammars/GLSLX.tmLanguage'), 'utf8');
  const document = new JSDOM(xml, { contentType: 'text/xml' });
  let expected;
  try {
    const plist = document.window.document.documentElement;
    if (plist.localName !== 'plist' || plist.children.length !== 1 || plist.firstElementChild.localName !== 'dict') throw new Error('glsl_plist_shape');
    expected = plistValue(plist.firstElementChild);
  } finally {
    document.window.close();
  }
  expected.name = 'glsl';
  expected.scopeName = 'source.glsl';
  expected.fileTypes = ['glsl'];
  const actual = JSON.parse(await readFile(join(sourceRoot, 'src/assets/grammars/glsl.json'), 'utf8'));
  if (!isDeepStrictEqual(actual, expected)) throw new Error('glsl_grammar_derivation');
}
async function verifyAssetOrigins(assets, packages, temporary) {
  const source = await json('src/assets/icon-themes/catppuccin-latte/source.json');
  const catppuccin = assets.groups.find(group => group.id === 'catppuccin-latte');
  if (source.sha256 !== catppuccin.archiveSha256 || source.version !== catppuccin.version || source.archive !== catppuccin.archive) throw new Error('catppuccin_provenance_drift');
  const bytes = download(source.archive);
  if (sha256(bytes) !== source.sha256) throw new Error('catppuccin_archive_integrity');
  const archive = join(temporary, 'catppuccin.vsix');
  await writeFile(archive, bytes);
  const prefix = 'src/assets/icon-themes/catppuccin-latte';
  const local = await json(`${prefix}/theme.json`);
  const upstream = JSON.parse(command('unzip', ['-p', archive, 'extension/dist/latte/theme.json']));
  const keys = ['file', 'folder', 'folderExpanded', 'fileNames', 'fileExtensions', 'folderNames', 'folderNamesExpanded'];
  if (!isDeepStrictEqual(Object.keys(local).sort(), [...keys, 'iconDefinitions'].sort())) throw new Error('catppuccin_theme_schema');
  for (const key of keys) if (!isDeepStrictEqual(local[key], upstream[key])) throw new Error('catppuccin_association_drift');
  for (const [key, definition] of Object.entries(local.iconDefinitions)) if (!isDeepStrictEqual(definition, upstream.iconDefinitions[key])) throw new Error('catppuccin_definition_drift');
  for (const path of await files(join(root, prefix, 'icons'))) {
    const upstreamBytes = command('unzip', ['-p', archive, `extension/dist/latte/icons/${basename(path)}`]);
    if (!(await readFile(path)).equals(upstreamBytes)) throw new Error('catppuccin_icon_drift');
  }
  const license = command('unzip', ['-p', archive, 'extension/LICENSE.txt']);
  if (!(await readFile(join(root, catppuccin.noticePaths[0]))).equals(license)) throw new Error('catppuccin_license_drift');
  await retain('assets/catppuccin-latte', 'LICENSE.txt', license, `${source.archive}#extension/LICENSE.txt`);
  for (const [id, name] of [['lucide-ui-icons', 'lucide-react'], ['material-file-icons', 'material-icon-theme']]) {
    if (assets.groups.find(group => group.id === id).version !== packages.find(pkg => pkg.ecosystem === 'npm' && pkg.name === name).version) throw new Error('asset_package_version_drift');
  }
  const fonts = assets.groups.find(group => group.id === 'geist-fonts');
  for (const name of ['geist', 'geist-mono']) if (fonts.versions[name] !== packages.find(pkg => pkg.name === `@fontsource-variable/${name}`).version) throw new Error('font_version_drift');
  const covenant = assets.groups.find(group => group.id === 'contributor-covenant-2.1');
  await retain('assets/contributor-covenant-2.1', 'LICENSE.txt', download(covenant.licenseUrl), covenant.licenseUrl);
  await retain('assets/contributor-covenant-2.1', 'upstream-code-of-conduct.txt', download(covenant.textUrl), covenant.textUrl);
  const codeNotices = assets.groups.find(group => group.id === 'code-highlighting-notices');
  if (codeNotices.version !== packages.find(pkg => pkg.ecosystem === 'npm' && pkg.name === 'shiki').version) throw new Error('code_assets_version_drift');
  for (const source of codeNotices.sources) {
    const bytes = download(source.url);
    if (!(await readFile(join(root, source.path))).equals(bytes)) throw new Error('code_notice_source_drift');
  }
  const glsl = assets.groups.find(group => group.id === 'shiki-embedded-glsl');
  for (const source of glsl.sources) {
    if (!(await readFile(join(root, source.path))).equals(download(source.url))) throw new Error('glsl_source_drift');
  }
  await verifyGlslGrammar();
}
async function refresh() {
  const temporary = await mkdtemp(join(tmpdir(), 'gitview-legal-'));
  sourceArchivesDirectory = temporary;
  try {
    const npm = await npmInventory(await json('package-lock.json'), temporary);
    const cargo = await cargoInventory();
    await supplementalNotices([...npm, ...cargo]);
    for (const pkg of [...npm, ...cargo]) pkg.selectedLicense = selectedLicense(pkg);
    const assets = await json('licenses/asset-provenance.json');
    await verifyAssetOrigins(assets, [...npm, ...cargo], temporary);
    if (sha256(await readFile(join(root, 'LICENSE'))) !== canonicalGplSha256 || !(await readFile(join(root, 'LICENSE'))).equals(download('https://www.gnu.org/licenses/gpl-3.0.txt'))) throw new Error('authoritative_gpl_text_mismatch');
    const inventory = { schemaVersion: 1, projectLicense: 'GPL-3.0-only', gplSource: 'https://www.gnu.org/licenses/gpl-3.0.txt', gplSha256: sha256(await readFile(join(root, 'LICENSE'))), sourceHashes: await sourceHashes(), legalTextHashes: await legalTextHashes(), assets, packages: [...npm, ...cargo] };
    await mkdir(dirname(inventoryPath), { recursive: true });
    await writeFile(inventoryPath, `${JSON.stringify(inventory, null, 2)}\n`);
    console.log(JSON.stringify({ inventoryRefreshed: true, npm: npm.length, cargo: cargo.length }));
  } finally { await rm(temporary, { recursive: true, force: true }); }
}
async function check(integrityOnly) {
  const inventory = await json('licenses/inventory.json');
  const drift = [];
  if (inventory.schemaVersion !== 1) throw new Error('inventory_schema');
  await verifyGlslGrammar();
  const current = await sourceHashes();
  if (JSON.stringify(current) !== JSON.stringify(inventory.sourceHashes)) drift.push('source_or_lock_drift');
  if (inventory.gplSha256 !== canonicalGplSha256 || sha256(await readFile(join(root, 'LICENSE'))) !== canonicalGplSha256) drift.push('gpl_text_drift');
  if (JSON.stringify(await json('licenses/asset-provenance.json')) !== JSON.stringify(inventory.assets)) drift.push('asset_evidence_drift');
  if (JSON.stringify(await legalTextHashes()) !== JSON.stringify(inventory.legalTextHashes)) drift.push('retained_legal_text_drift');
  const npm = await json('package-lock.json');
  const cargo = cargoDependencyLockEntries();
  const identities = inventory.packages.map(pkg => `${pkg.ecosystem}:${pkg.ecosystem === 'npm' ? pkg.lockPath : `${pkg.name}@${pkg.version}`}`).sort();
  const expected = [...npmPackageEntries(npm).map(([path]) => `npm:${path}`), ...cargo.map(pkg => `cargo:${pkg.name}@${pkg.version}`)].sort();
  if (JSON.stringify(identities) !== JSON.stringify(expected)) drift.push('incomplete_inventory');
  if ((await json('package.json')).license !== 'GPL-3.0-only' || npm.packages[''].license !== 'GPL-3.0-only' || !/^license = "GPL-3.0-only"$/m.test((await readFile(join(root, 'src-tauri/Cargo.toml'))).toString())) drift.push('project_license_metadata');
  const resources = (await json('src-tauri/tauri.conf.json')).bundle.resources;
  if (resources?.['../LICENSE'] !== 'legal/LICENSE' || resources?.['../THIRD_PARTY_NOTICES.md'] !== 'legal/THIRD_PARTY_NOTICES.md' || resources?.['../licenses/'] !== 'legal/licenses/') drift.push('legal_resource_mapping');
  const blockers = [];
  let noticeCount = 0;
  for (const pkg of inventory.packages) {
    const expectedSelection = selectedLicense(pkg);
    if (!expectedSelection || pkg.selectedLicense !== expectedSelection || !pkg.notices.length) blockers.push(`${pkg.ecosystem}:${pkg.name}@${pkg.version}:unresolved_license_evidence`);
    if (pkg.ecosystem === 'npm' && (!isDeepStrictEqual(pkg.declaredLicense, npm.packages[pkg.lockPath]?.license ?? null) || !isDeepStrictEqual(pkg.upstreamLicense, pkg.declaredLicense))) blockers.push(`${pkg.ecosystem}:${pkg.name}@${pkg.version}:license_metadata_mismatch`);
    if (pkg.ecosystem === 'npm') {
      const locked = npm.packages[pkg.lockPath];
      if (pkg.sourceKind === 'vendored') {
        const rule = (await vendorRules('npm')).find(rule => rule.path === pkg.sourcePath);
        const lockedPath = locked?.resolved?.startsWith('file:') ? locked.resolved.slice(5) : pkg.lockPath;
        if (!rule || lockedPath !== rule.path || pkg.version !== locked?.version || pkg.name !== rule.name || pkg.version !== rule.version || pkg.source !== rule.source || pkg.archiveSha256 !== rule.archiveSha256 || !isDeepStrictEqual(await verifyNpmVendor(rule), pkg.sourceFiles)) drift.push('npm_vendor_identity_drift');
      } else if (pkg.version !== locked?.version || pkg.source !== locked?.resolved || pkg.integrity !== locked?.integrity) drift.push('npm_record_identity_drift');
    } else if (pkg.ecosystem === 'cargo') {
      const locked = cargo.find(entry => entry.name === pkg.name && entry.version === pkg.version);
      if (pkg.sourceKind === 'vendored') {
        const rule = (await vendorRules()).find(rule => rule.name === pkg.name && rule.version === pkg.version);
        if (!locked || locked.source || !rule || rule.path !== pkg.sourcePath || rule.source !== pkg.source || rule.archiveSha256 !== pkg.archiveSha256 || !isDeepStrictEqual(await verifyCargoVendor(rule), pkg.sourceFiles)) drift.push('cargo_vendor_identity_drift');
      } else if (!locked?.source?.startsWith('registry+') || pkg.checksum !== locked.checksum) drift.push('cargo_record_identity_drift');
    }
    if (pkg.incompatibleSecondaryLicense) blockers.push(`${pkg.ecosystem}:${pkg.name}@${pkg.version}:incompatible_secondary_license`);
    for (const notice of pkg.notices) {
      if (!notice.path.startsWith('licenses/texts/') || notice.path.split('/').includes('..')) throw new Error('unsafe_notice_path');
      try { if (sha256(await readFile(join(root, notice.path))) !== notice.sha256) drift.push('notice_text_drift'); } catch { drift.push('notice_text_missing'); }
      noticeCount += 1;
    }
  }
  for (const asset of inventory.assets.groups) if (asset.status !== 'verified') blockers.push(`assets:${asset.id}:${asset.status}`);
  console.log(JSON.stringify({ integrityOnly, publicationCleared: !integrityOnly && !drift.length && !blockers.length, npm: inventory.packages.filter(pkg => pkg.ecosystem === 'npm').length, cargo: inventory.packages.filter(pkg => pkg.ecosystem === 'cargo').length, retainedNoticeReferences: noticeCount, drift: [...new Set(drift)], blockers }, null, 2));
  process.exitCode = drift.length || (!integrityOnly && blockers.length) ? 1 : 0;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const mode = process.argv.length === 2 ? ['--check'] : process.argv.slice(2);
    if (mode.length !== 1 || !['--refresh', '--check', '--inventory-only'].includes(mode[0])) throw new Error('usage_check_refresh_inventory_only');
    if (mode[0] === '--refresh') await refresh();
    else await check(mode[0] === '--inventory-only');
  } catch {
    console.error('license_inventory_failed: source access, integrity or prerequisites unavailable; no clearance issued');
    process.exitCode = 2;
  }
}
