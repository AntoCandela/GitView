/** Validates repo-local documentation and executes trusted SQL-skill examples with isolated data. */
import assert from 'node:assert/strict';
import { copyFile, lstat, mkdir, mkdtemp, readFile, readdir, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execute, isEntryPoint, OUTPUT_LIMIT, parseOptions } from '../../scripts/evidence.mjs';
import { diagnosticSource } from '../examples/diagnostics-source.mjs';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const excluded = new Set(['node_modules', '.git', '.verification', 'target', 'dist', 'build']);
const schemaColumns = ['id', 'timestamp_ms', 'session_id', 'operation_id', 'parent_operation_id', 'operation_kind', 'level', 'component', 'event', 'code', 'duration_ms', 'exit_code', 'stdout_bytes', 'stderr_bytes', 'cleanup_failed'];
const codes = new Set(['invalid_arguments', 'invalid_doc_path', 'invalid_doc_link', 'missing_doc_fragment', 'invalid_skill_frontmatter', 'missing_doc_examples', 'invalid_example_snippets', 'invalid_example_output', 'doc_examples_failed']);
const exerciseStages = new Set(['cargo_metadata', 'driver_build', 'driver_run', 'driver_output', 'cli_build', 'cli_schema', 'cli_errors', 'cli_children', 'privacy']);
const processCodes = new Set(['ok', 'start_failed', 'cancelled', 'signal_termination', 'output_limit', 'check_failed', 'invalid_output', 'assertion_failed']);
const compilerCode = /^(?:E\d{4}|LNK\d{4}|RC\d{4}|CVT\d{4}|C\d{4}|C[0-9A-F]{7})$/;

/** Accept only the closed CLI failure envelope; never copy borrowed fields into evidence. */
export function documentationFailureEvidence(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).length !== 5
      || !['error', 'stage', 'processCode', 'exitCode', 'compilerCodes'].every(key => Object.hasOwn(value, key))
      || !codes.has(value.error)
      || !(value.stage === null || exerciseStages.has(value.stage))
      || !(value.processCode === null || processCodes.has(value.processCode))
      || !(value.exitCode === null || (Number.isInteger(value.exitCode) && value.exitCode >= -2_147_483_648 && value.exitCode <= 4_294_967_295))
      || (value.processCode === null && value.exitCode !== null)
      || !Array.isArray(value.compilerCodes) || value.compilerCodes.length > 16
      || value.compilerCodes.some(code => typeof code !== 'string' || !compilerCode.test(code))
      || (value.compilerCodes.length > 0 && (!['driver_build', 'cli_build'].includes(value.stage) || value.processCode !== 'check_failed'))) return null;
  return { error: value.error, stage: value.stage, processCode: value.processCode, exitCode: value.exitCode, compilerCodes: [...value.compilerCodes] };
}

/** Recognize a complete success envelope without accepting contradictory failure fields. */
export function documentationSucceeded(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)
      || Object.keys(value).length !== 5
      || !['status', 'documents', 'links', 'skills', 'examples'].every(key => Object.hasOwn(value, key))
      || value.status !== 'passed'
      || !['documents', 'links', 'skills'].every(key => Number.isSafeInteger(value[key]) && value[key] >= 0)) return false;
  const examples = value.examples;
  return Boolean(examples && typeof examples === 'object' && !Array.isArray(examples)
    && Object.keys(examples).length === 5
    && ['rust', 'sql', 'reader', 'cli', 'privacy'].every(key => Object.hasOwn(examples, key))
    && ['rust', 'sql'].every(key => Number.isSafeInteger(examples[key]) && examples[key] > 0)
    && ['reader', 'cli', 'privacy'].every(key => examples[key] === 'passed'));
}

class DocumentationFailure extends Error {
  constructor(evidence) {
    super(evidence.error, { cause: evidence });
  }
}

function documentationError(error, stage = null) {
  if (error instanceof DocumentationFailure) {
    return new DocumentationFailure({ ...error.cause, stage: error.cause.stage ?? stage });
  }
  return new DocumentationFailure({
    error: codes.has(error?.message) ? error.message : 'doc_examples_failed',
    stage,
    processCode: error instanceof SyntaxError ? 'invalid_output' : error instanceof assert.AssertionError ? 'assertion_failed' : null,
    exitCode: null,
    compilerCodes: [],
  });
}

async function boundedText(path) {
  const content = await readFile(path);
  if (content.length > OUTPUT_LIMIT) throw new Error('invalid_doc_path');
  return content.toString('utf8');
}

/** Reject escapes, generated targets and symlinks before reading a repository path. */
async function ownedPath(root, path, { generated = false, missing = false } = {}) {
  const location = resolve(root, path);
  const parts = relative(root, location).split(sep);
  if (parts[0] === '..' || isAbsolute(relative(root, location)) || (!generated && parts.some(part => excluded.has(part)))) throw new Error('invalid_doc_path');
  let current = root;
  for (const part of parts.filter(Boolean)) {
    current = join(current, part);
    let metadata;
    try { metadata = await lstat(current); }
    catch (error) { if (missing && error.code === 'ENOENT') continue; throw new Error('invalid_doc_path'); }
    if (metadata.isSymbolicLink()) throw new Error('invalid_doc_path');
  }
  return location;
}
function prose(markdown) {
  let fence;
  return markdown.split(/\r?\n/).map(line => {
    const marker = /^\s{0,3}(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker && !fence) { fence = marker; return ''; }
    if (fence) { if (marker?.[0] === fence[0] && marker.length >= fence.length) fence = undefined; return ''; }
    return line;
  }).join('\n');
}

function headingFragments(markdown) {
  const fragments = new Set();
  const counts = new Map();
  const lines = prose(markdown).split('\n');
  for (let index = 0; index < lines.length; index += 1) {
    const heading = /^\s{0,3}#{1,6}\s+(.+?)(?:\s+#+\s*)?$/.exec(lines[index])?.[1]
      ?? (index + 1 < lines.length && /^\s{0,3}(?:=+|-+)\s*$/.test(lines[index + 1]) && lines[index].trim() ? lines[index].trim() : undefined);
    if (!heading) continue;
    const slug = heading.toLowerCase().replace(/<[^>]+>/g, '').replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu, '').replace(/\s/g, '-');
    const count = counts.get(slug) ?? 0;
    counts.set(slug, count + 1);
    fragments.add(`${slug}${count ? `-${count}` : ''}`);
  }
  for (const match of prose(markdown).matchAll(/\b(?:id|name)=["']([^"']+)["']/g)) fragments.add(match[1]);
  return fragments;
}

function links(markdown) {
  const text = prose(markdown).replace(/(`+)[^\n]*?\1/g, '');
  const references = new Map();
  for (const match of text.matchAll(/^\s{0,3}\[([^\]]+)\]:\s*(<[^>]+>|\S+)/gm)) references.set(match[1].trim().toLowerCase(), match[2].replace(/^<|>$/g, ''));
  const targets = [];
  for (const match of text.matchAll(/!?\[[^\]\n]*\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+["'][^\n]*?["'])?\s*\)/g)) targets.push(match[1].replace(/^<|>$/g, ''));
  for (const match of text.matchAll(/!?\[([^\]\n]+)\]\[([^\]\n]*)\]/g)) {
    const target = references.get((match[2] || match[1]).trim().toLowerCase());
    if (!target) throw new Error('invalid_doc_link');
    targets.push(target);
  }
  // Definitions also count: dead references must not conceal broken paths.
  targets.push(...references.values());
  for (const match of text.matchAll(/\b(?:href|src)=["']([^"']+)["']/g)) targets.push(match[1]);
  return targets;
}

function validateSkill(markdown, path) {
  const header = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(markdown)?.[1];
  if (!header) throw new Error('invalid_skill_frontmatter');
  const fields = new Map();
  for (const line of header.split(/\r?\n/)) {
    if (!line.trim() || line.trimStart().startsWith('#')) continue;
    const match = /^([a-z][a-z_-]*):\s*(.+)$/.exec(line);
    if (!match || fields.has(match[1])) throw new Error('invalid_skill_frontmatter');
    let value = match[2].trim();
    if (value.startsWith('"')) { try { value = JSON.parse(value); } catch { throw new Error('invalid_skill_frontmatter'); } }
    else if (value.startsWith("'")) { if (!value.endsWith("'")) throw new Error('invalid_skill_frontmatter'); value = value.slice(1, -1).replace(/''/g, "'"); }
    if (typeof value !== 'string' || !value.trim() || /^[>|[\]{}]/.test(value)) throw new Error('invalid_skill_frontmatter');
    fields.set(match[1], value);
  }
  const name = fields.get('name');
  if (!name || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(name) || name !== basename(dirname(path)) || !fields.get('description')) throw new Error('invalid_skill_frontmatter');
}

/** Scan only .agents Markdown and the two root instruction maps; never fetch remote links. */
export async function checkDocumentation({ root = repositoryRoot } = {}) {
  root = await realpath(root);
  const files = ['AGENTS.md', 'DEVELOPMENT.md'];
  async function visit(path) {
    const directory = await ownedPath(root, path);
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      if (excluded.has(entry.name)) continue;
      const child = join(path, entry.name);
      if (entry.isSymbolicLink()) throw new Error('invalid_doc_path');
      if (entry.isDirectory()) {
        if (path === join('.agents', 'skills')) {
          const skill = await ownedPath(root, join(child, 'SKILL.md'));
          if (!(await lstat(skill)).isFile()) throw new Error('invalid_skill_frontmatter');
        }
        await visit(child);
      }
      else if (entry.isFile() && entry.name.endsWith('.md')) files.push(child);
    }
  }
  await visit('.agents');
  const documents = new Map();
  for (const file of files) documents.set(file, await boundedText(await ownedPath(root, file)));
  let linkCount = 0;
  let skillCount = 0;
  for (const [file, markdown] of documents) {
    if (basename(file) === 'SKILL.md') { validateSkill(markdown, file); skillCount += 1; }
    for (const target of links(markdown)) {
      if (/^(?:https?:|mailto:)/i.test(target)) continue;
      if (/^[a-z][a-z\d+.-]*:/i.test(target) || target.startsWith('//') || target.startsWith('/')) throw new Error('invalid_doc_link');
      let decoded;
      try { decoded = decodeURIComponent(target); } catch { throw new Error('invalid_doc_link'); }
      if (decoded.includes('\0') || decoded.includes('\\') || decoded.includes('?')) throw new Error('invalid_doc_link');
      const [destination, fragment, ...extra] = decoded.split('#');
      if (extra.length) throw new Error('invalid_doc_link');
      const linked = await ownedPath(root, destination ? join(dirname(file), destination) : file);
      if (fragment) {
        if (!linked.endsWith('.md') || !headingFragments(await boundedText(linked)).has(fragment)) throw new Error('missing_doc_fragment');
      }
      linkCount += 1;
    }
  }
  return { documents: documents.size, links: linkCount, skills: skillCount };
}

async function skillExamples(root) {
  const markdown = await boundedText(await ownedPath(root, '.agents/skills/gitview-sql-diagnostics/SKILL.md'));
  const rust = [...markdown.matchAll(/^```rust\r?\n([\s\S]*?)^```\s*$/gm)].map(match => match[1]);
  const sql = [...markdown.matchAll(/^```sql\r?\n([\s\S]*?)^```\s*$/gm)].map(match => match[1]);
  const recent = /<<'SQL'\r?\n([\s\S]*?)\r?\nSQL\r?\n/.exec(markdown)?.[1];
  if (rust.length !== 2 || sql.length !== 2 || !recent) throw new Error('missing_doc_examples');
  return { rust, queries: { recent, timed: sql[0], causal: sql[1] } };
}

function compilerFailureCodes(output) {
  const codes = new Set();
  const plain = output.replace(/\x1b\[[0-9;]*m/g, '');
  for (const match of plain.matchAll(/\berror\[(E\d{4})\]|\b(?:fatal )?error (LNK\d{4}|RC\d{4}|CVT\d{4}|C\d{4}|c[0-9a-f]{7}):/g)) {
    codes.add((match[1] ?? match[2]).toUpperCase());
    if (codes.size === 16) break;
  }
  return [...codes].sort();
}

async function run(executable, args, cwd, { compiler = false } = {}) {
  const result = await execute(executable, args, { cwd, signal: AbortSignal.timeout(15 * 60 * 1000) });
  if (result.code !== 'ok') throw new DocumentationFailure({
    error: 'doc_examples_failed', stage: null, processCode: result.code, exitCode: result.exitCode,
    compilerCodes: compiler && result.code === 'check_failed' ? compilerFailureCodes(result.output) : [],
  });
  return result.output;
}

/** Validate and canonicalize an ignored/disposable destination without creating it. */
export async function diagnosticOutput(root, output) {
  root = await realpath(root);
  let destination = resolve(root, output);
  const temporary = await realpath(tmpdir());
  const visibleTemporary = resolve(tmpdir());
  const temporaryRelative = relative(visibleTemporary, destination);
  if (temporaryRelative && temporaryRelative !== '..' && !temporaryRelative.startsWith(`..${sep}`) && !isAbsolute(temporaryRelative)) {
    destination = resolve(temporary, temporaryRelative);
  }
  if (relative(root, destination).startsWith(`.verification${sep}`)) {
    return ownedPath(root, destination, { generated: true, missing: true });
  }
  const local = relative(root, destination);
  if (!local || (local !== '..' && !local.startsWith(`..${sep}`) && !isAbsolute(local))) throw new Error('invalid_doc_path');
  // Disposable evaluation fixtures may live under the explicit OS temporary root.
  const path = relative(temporary, destination);
  if (isAbsolute(output) && path && path !== '..' && !path.startsWith(`..${sep}`) && !isAbsolute(path)) {
    return ownedPath(temporary, path, { generated: true, missing: true });
  }
  throw new Error('invalid_doc_path');
}

/** Compiles and executes two trusted Rust source strings. This is deliberate local code execution, not isolation from malicious code.
 * Output is a repo-owned .verification directory or explicit disposable OS-temp directory. Scaffold cleanup is unconditional.
 */
export async function exerciseDiagnostics({ root = repositoryRoot, output = '.verification/docs', snippets } = {}) {
  root = await realpath(root);
  const destination = await diagnosticOutput(root, output);
  await mkdir(destination, { recursive: true, mode: 0o700 });
  const examples = await skillExamples(root);
  snippets ??= examples.rust;
  if (!Array.isArray(snippets) || snippets.length !== 2 || snippets.some(source => typeof source !== 'string' || !source.trim() || Buffer.byteLength(source) > OUTPUT_LIMIT)) throw new Error('invalid_example_snippets');
  const data = await mkdtemp(join(destination, 'diagnostics-'));
  const database = join(data, 'diagnostics.sqlite');
  const packageName = `gitview-doc-${basename(data).toLowerCase()}`;
  const scaffold = await mkdtemp(join(tmpdir(), 'gitview-doc-examples-'));
  let stage = null;
  try {
    await mkdir(join(scaffold, 'src'));
    const native = await ownedPath(root, 'src-tauri');
    stage = 'cargo_metadata';
    const metadata = JSON.parse(await run('cargo', ['metadata', '--offline', '--locked', '--no-deps', '--format-version', '1', '--manifest-path', join(native, 'Cargo.toml')], root));
    const sqlite = metadata.packages.find(pkg => pkg.name === 'gitview').dependencies.find(dependency => dependency.name === 'rusqlite');
    stage = null;
    const vendors = JSON.parse(await boundedText(await ownedPath(root, 'src-tauri/vendor/patches.json')));
    // Cargo ignores dependency-level patches; the standalone root must select the same audited sources.
    const patches = await Promise.all(vendors.packages.map(async pkg =>
      `${JSON.stringify(pkg.name)} = { path = ${JSON.stringify(await ownedPath(root, pkg.path))} }`));
    const manifest = `[package]\nname = ${JSON.stringify(packageName)}\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\ngitview_lib = { package = "gitview", path = ${JSON.stringify(native)} }\ntokio = { version = "1", features = ["macros", "rt", "time"] }\nrusqlite = { version = ${JSON.stringify(sqlite.req)}, features = ["bundled"] }\nuuid = { version = "1", features = ["v4", "serde"] }\nserde_json = "1"\n\n[patch.crates-io]\n${patches.join('\n')}\n`;
    await writeFile(join(scaffold, 'Cargo.toml'), manifest);
    // Dependency linker arguments do not reach this executable; own both CRT and activation policy.
    await writeFile(join(scaffold, 'build.rs'), `fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // tauri-build 2.7.1's private static_vcruntime module shadows msvcrt.lib via a
        // transitive search path. Its replacement-library arguments are not transitive.
        // Keep its CRT policy here without invoking unrelated Tauri app/resource setup.
        // https://docs.rs/crate/tauri-build/2.7.1/source/src/static_vcruntime.rs
        for library in [
            "libvcruntimed.lib", "vcruntime.lib", "vcruntimed.lib", "libcmtd.lib",
            "msvcrt.lib", "msvcrtd.lib", "libucrt.lib", "libucrtd.lib",
        ] {
            println!("cargo:rustc-link-arg=/NODEFAULTLIB:{library}");
        }
        for library in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
            println!("cargo:rustc-link-arg=/DEFAULTLIB:{library}");
        }
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    }
}
`);
    await writeFile(join(scaffold, 'src/main.rs'), diagnosticSource(snippets, examples.queries));
    await copyFile(await ownedPath(root, 'src-tauri/Cargo.lock'), join(scaffold, 'Cargo.lock'));
    // The copied lock retains dependency pins; only the throwaway root package requires resolution.
    const target = await ownedPath(root, '.verification/docs-native-target', { generated: true, missing: true });
    stage = 'driver_build';
    await run('cargo', ['build', '--offline', '--manifest-path', join(scaffold, 'Cargo.toml'), '--target-dir', target], root, { compiler: true });
    const executableSuffix = process.platform === 'win32' ? '.exe' : '';
    stage = 'driver_run';
    const driverOutput = await run(join(target, 'debug', `${packageName}${executableSuffix}`), [database], root);
    stage = 'driver_output';
    const result = JSON.parse(driverOutput);
    const canonicalId = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
    if (!canonicalId.test(result.operationId) || !canonicalId.test(result.childOperationId) || result.operationId === result.childOperationId) throw new Error('invalid_example_output');
    stage = 'cli_build';
    await run('cargo', ['build', '--offline', '--locked', '--manifest-path', join(native, 'Cargo.toml'), '--target-dir', target, '--bin', 'gitview-diagnostics'], root, { compiler: true });
    const cli = join(target, 'debug', `gitview-diagnostics${executableSuffix}`);
    stage = 'cli_schema';
    const schema = JSON.parse(await run(cli, ['--database', database, 'schema'], root));
    assert.deepEqual(schema.columns.map(column => column.name), schemaColumns);
    stage = 'cli_errors';
    const errors = JSON.parse(await run(cli, ['--database', database, 'events', '--level', 'error', '--limit', '20'], root));
    assert.equal(errors.has_more, false);
    assert.equal(errors.events.length, 1);
    assert.equal(errors.events[0].operation_id, result.operationId);
    assert.equal(errors.events[0].code, 'save_failed');
    stage = 'cli_children';
    const children = JSON.parse(await run(cli, ['--database', database, 'events', '--operation-id', result.childOperationId, '--limit', '200'], root));
    assert.equal(children.has_more, false);
    assert.equal(children.events.length, 1);
    assert.equal(children.events[0].parent_operation_id, result.operationId);
    for (const row of [...errors.events, ...children.events]) assert.deepEqual(Object.keys(row), schemaColumns);
    stage = 'privacy';
    assert.equal((await readFile(database)).includes(Buffer.from('private-doc-example-payload')), false);
    return { database, operationId: result.operationId, childOperationId: result.childOperationId };
  } catch (error) {
    throw documentationError(error, stage);
  } finally {
    try { await rm(scaffold, { recursive: true, force: true }); }
    catch (error) { throw documentationError(error); }
  }
}

if (isEntryPoint(import.meta.url)) {
  try {
    const options = parseOptions(process.argv.slice(2), ['root', 'output']);
    const root = options.root ?? repositoryRoot;
    const checked = await checkDocumentation({ root });
    await exerciseDiagnostics({ root, output: options.output });
    process.stdout.write(`${JSON.stringify({ status: 'passed', ...checked, examples: { rust: 2, sql: 3, reader: 'passed', cli: 'passed', privacy: 'passed' } })}\n`);
  } catch (error) {
    process.stderr.write(`${JSON.stringify(documentationError(error).cause)}\n`);
    process.exitCode = 1;
  }
}
