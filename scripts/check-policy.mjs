/** Enforces test placement, tracked-artifact privacy and existing architecture imports. */
import { lstat, readFile } from 'node:fs/promises';
import { resolve, posix, isAbsolute } from 'node:path';
import ts from 'typescript';
import { execute, isEntryPoint, parseOptions } from './evidence.mjs';

const humanReview = ['correctness', 'fixture_isolation', 'privacy_allowlist', 'trace_causality', 'native_observations'];
const forbiddenArtifact = path => /(?:^|\/)(?:\.verification|\.vitest|target|dist|node_modules|coverage)(?:\/|$)/.test(path) || /\.(?:sqlite(?:3)?|db)(?:-(?:wal|shm)|-journal)?$/i.test(path) || /(?:^|\/)diagnostics[^/]*\.(?:json|log|zip)$/i.test(path);
const safePath = path => /^[A-Za-z0-9_./-]+$/.test(path) && !posix.isAbsolute(path) && !path.split('/').includes('..');
const repositoryPath = path => typeof path === 'string' && !isAbsolute(path) && !posix.isAbsolute(path) && !path.includes('\0') && !path.split(/[\\/]/).includes('..');

function imports(path, source) {
  const dependencies = [];
  const tree = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true);
  function visit(node) {
    if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier && ts.isStringLiteralLike(node.moduleSpecifier)) dependencies.push(node.moduleSpecifier.text);
    if (ts.isImportTypeNode(node) && ts.isLiteralTypeNode(node.argument) && ts.isStringLiteralLike(node.argument.literal)) dependencies.push(node.argument.literal.text);
    if (ts.isCallExpression(node) && (node.expression.kind === ts.SyntaxKind.ImportKeyword || (ts.isIdentifier(node.expression) && node.expression.text === 'require'))) dependencies.push(node.arguments[0] && ts.isStringLiteralLike(node.arguments[0]) ? node.arguments[0].text : null);
    ts.forEachChild(node, visit);
  }
  visit(tree);
  return dependencies;
}

// Erase Rust comments and literals before inspecting tokens; test-only #[path] is not an import.
function rustTokens(source) {
  let tokens = '';
  let index = 0;
  while (index < source.length) {
    if (source.startsWith('//', index)) {
      const end = source.indexOf('\n', index + 2);
      index = end < 0 ? source.length : end;
      tokens += ' ';
      continue;
    }
    if (source.startsWith('/*', index)) {
      let depth = 1;
      index += 2;
      while (index < source.length && depth > 0) {
        if (source.startsWith('/*', index)) { depth++; index += 2; }
        else if (source.startsWith('*/', index)) { depth--; index += 2; }
        else index++;
      }
      tokens += ' ';
      continue;
    }
    const raw = source[index] === 'r' || source[index] === 'b' ? source.slice(index).match(/^(?:b)?r(#{0,255})"/) : null;
    if (raw) {
      const closer = `"${raw[1]}`;
      const end = source.indexOf(closer, index + raw[0].length);
      index = end < 0 ? source.length : end + closer.length;
      tokens += ' ';
      continue;
    }
    if (source[index] === '"') {
      index++;
      while (index < source.length) {
        if (source[index] === '\\') index += 2;
        else if (source[index++] === '"') break;
      }
      tokens += ' ';
      continue;
    }
    const character = source[index] === "'" ? source.slice(index).match(/^'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'/u) : null;
    if (character) { index += character[0].length; tokens += ' '; continue; }
    tokens += source[index++];
  }
  return tokens;
}

export async function checkPolicy({ root = process.cwd(), trackedFiles } = {}) {
  const violations = [];
  const add = (rule, path) => violations.push({ rule, file: safePath(path) ? path : 'omitted_private_filename' });
  let files = trackedFiles;
  if (!files) {
    const inventory = await execute('git', ['ls-files', '-z'], { cwd: root });
    const additions = await execute('git', ['ls-files', '--others', '--exclude-standard', '-z'], { cwd: root });
    if (inventory.code !== 'ok' || additions.code !== 'ok') return { schemaVersion: 1, passed: false, violations: [{ rule: 'inventory_unavailable', file: null }], humanReview };
    trackedFiles = inventory.output.split('\0').filter(Boolean);
    files = [...new Set([...trackedFiles, ...additions.output.split('\0').filter(Boolean)])].sort();
  }
  const tracked = new Set(trackedFiles);
  for (const path of files) {
    if (!repositoryPath(path)) { add('inventory_path', path); continue; }
    try { await lstat(resolve(root, path)); }
    catch (error) {
      if (error.code !== 'ENOENT') add('source_unavailable', path);
      continue;
    }
    if (tracked.has(path) && forbiddenArtifact(path)) add('private_artifact', path);
    if (/\.(?:test|spec)\.[cm]?[jt]sx?$/.test(path) && !/^tests\/(?:unit|integration|infrastructure)\//.test(path)) add('test_placement', path);
    if (/^src-tauri\/tests\/.+\.rs$/.test(path) && !/^src-tauri\/tests\/(?:unit|integration|support)\//.test(path)) add('test_placement', path);
    if (!/^(?:src\/.+\.[jt]sx?|src-tauri\/src\/.+\.rs)$/.test(path)) continue;
    let source;
    try { source = await readFile(resolve(root, path), 'utf8'); }
    catch { add('source_unavailable', path); continue; }
    if (path.endsWith('.rs')) {
      const tokens = rustTokens(source);
      if (/#\s*\[\s*(?:tokio\s*::\s*)?test(?:\s*\([^\]]*\))?\s*\]/.test(tokens)) add('test_placement', path);
      const nativeImports = [...tokens.matchAll(/\b(?:use|extern\s+crate)\s+([^;]+);/g)].map(match => match[1].replace(/\s+/g, ''));
      const importsTauri = /\btauri(?:_plugin_[a-z0-9_]+)?\s*(?:::|[,;}]|\bas\b)/.test(tokens);
      // A qualified process ID read is identity metadata, not process execution.
      const importsIo = /\bstd\s*::\s*(?:fs\s*::|process\s*::\s*(?:Command|Stdio|exit|abort)\b)/.test(tokens)
        || nativeImports.some(item => /std::(?:fs|process)\b/.test(item) || /std::\{[^;]*\b(?:fs|process)\b/.test(item));
      if (path.startsWith('src-tauri/src/workspace/') && path !== 'src-tauri/src/workspace/persistence.rs' && importsIo) add('native_dependency', path);
      if (!path.startsWith('src-tauri/src/host/') && importsTauri) add('native_dependency', path);
      continue;
    }
    const sourceFeature = path.match(/^src\/features\/([^/]+)\//)?.[1];
    const boundary = path.startsWith('src/contracts/') ? 'contract_dependency'
      : path.startsWith('src/ui/') ? 'ui_dependency'
        : path.startsWith('src/platform/') ? 'platform_dependency' : null;
    for (const dependency of imports(path, source)) {
      if (dependency === null) {
        if (boundary || sourceFeature || path.startsWith('src/app/')) add('dynamic_dependency', path);
        continue;
      }
      const target = dependency.startsWith('.') ? posix.normalize(posix.join(posix.dirname(path), dependency)) : dependency;
      const prohibited = boundary === 'contract_dependency'
        ? /^(?:react(?:-dom)?(?:\/|$)|@tauri-apps\/|src\/(?:app|platform|features|ui)(?:\/|$))/.test(target)
        : boundary === 'ui_dependency'
          ? /^src\/(?:app|features|platform|contracts)(?:\/|$)/.test(target) || target.startsWith('@tauri-apps/')
          : boundary === 'platform_dependency'
            && /^(?:react(?:-dom)?(?:\/|$)|src\/(?:app|features|ui)(?:\/|$))/.test(target);
      if (prohibited) { add(boundary, path); continue; }
      const moduleTarget = target.replace(/\.[cm]?[jt]sx?$/, '').replace(/\/$/, '');
      const targetFeature = moduleTarget.match(/^src\/features\/([^/]+)(?:\/(.*))?$/);
      if ((sourceFeature && /^src\/app(?:\/|$)/.test(moduleTarget))
          || (targetFeature && targetFeature[1] !== sourceFeature && targetFeature[2] !== undefined && targetFeature[2] !== 'index')) {
        add('feature_dependency', path);
      }
    }
  }
  return { schemaVersion: 1, passed: violations.length === 0, violations, humanReview };
}

if (isEntryPoint(import.meta.url)) {
  try {
    parseOptions(process.argv.slice(2), []);
    const result = await checkPolicy();
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exitCode = result.passed ? 0 : 1;
  } catch {
    process.stdout.write(`${JSON.stringify({ schemaVersion: 1, passed: false, violations: [{ rule: 'source_unavailable', file: null }], humanReview })}\n`);
    process.exitCode = 1;
  }
}
