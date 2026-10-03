/** Exercises local link, heading and skill boundaries against owned filesystem fixtures. */
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import test from 'node:test';
import { checkDocumentation, diagnosticOutput, documentationFailureEvidence, exerciseDiagnostics } from '../../.agents/scripts/check-docs.mjs';

async function fixture(context) {
  const root = await mkdtemp(join(tmpdir(), 'gitview-doc-boundary-'));
  context.after(() => rm(root, { recursive: true, force: true }));
  const skill = join(root, '.agents/skills/local-check/SKILL.md');
  await mkdir(join(root, '.agents/skills/local-check'), { recursive: true });
  await writeFile(join(root, 'AGENTS.md'), '# Map\n[Toolkit](.agents/README.md)\n');
  await writeFile(join(root, 'DEVELOPMENT.md'), '# Development\n');
  await writeFile(join(root, '.agents/README.md'), '# Toolkit\n[Skill](skills/local-check/SKILL.md#boundary)\n');
  await writeFile(skill, '---\nname: local-check\ndescription: Inspect the local boundary.\n---\n# Skill\n## Boundary\n');
  return { root, skill };
}

test('broken links in root instructions and owned skills fail rather than reporting health', async context => {
  const { root, skill } = await fixture(context);
  await writeFile(join(root, 'AGENTS.md'), '[Missing toolkit](.agents/missing.md)\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_doc_path' });
  await writeFile(join(root, 'AGENTS.md'), '# Map\n');
  await writeFile(skill, '---\nname: local-check\ndescription: Local check.\n---\n## Boundary\n[Missing implementation](../../../missing.rs)\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_doc_path' });
});

test('fragments resolve duplicate and code-formatted headings but reject obsolete headings', async context => {
  const { root } = await fixture(context);
  await writeFile(join(root, 'DEVELOPMENT.md'), '# Development\n## `cargo run`\n## `cargo run`\n');
  await writeFile(join(root, 'AGENTS.md'), '[Command](DEVELOPMENT.md#cargo-run-1)\n');
  const result = await checkDocumentation({ root });
  assert.equal(result.skills, 1);
  await writeFile(join(root, 'AGENTS.md'), '[Command](DEVELOPMENT.md#removed-command)\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'missing_doc_fragment' });
});

test('reference links validate destinations while fenced examples are not interpreted as prose', async context => {
  const { root } = await fixture(context);
  await writeFile(join(root, 'AGENTS.md'), '[Workflow][flow]\n\n[flow]: DEVELOPMENT.md#development\n\n```md\n[Example](missing.md)\n```\n');
  assert.equal((await checkDocumentation({ root })).links, 3);
  await writeFile(join(root, 'AGENTS.md'), '[Workflow][flow]\n\n[flow]: DEVELOPMENT.md#obsolete\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'missing_doc_fragment' });
});

test('skill identities cannot disagree with their owner directory or omit a usable trigger', async context => {
  const { root, skill } = await fixture(context);
  await writeFile(skill, '---\nname: another-owner\ndescription: Inspect the boundary.\n---\n# Boundary\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_skill_frontmatter' });
  await writeFile(skill, '---\nname: local-check\ndescription: ""\n---\n# Boundary\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_skill_frontmatter' });
});

test('duplicate or missing skill frontmatter cannot conceal a conflicting identity', async context => {
  const { root, skill } = await fixture(context);
  await writeFile(skill, '---\nname: local-check\nname: other\ndescription: Local check.\n---\n# Boundary\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_skill_frontmatter' });
  await writeFile(skill, '# Boundary\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_skill_frontmatter' });
});

test('path escapes and generated dependencies are not accepted as owned documentation', async context => {
  const { root } = await fixture(context);
  await writeFile(join(root, 'AGENTS.md'), '[Outside](../private-doc-payload.md)\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_doc_path' });
  await mkdir(join(root, 'node_modules/dependency'), { recursive: true });
  await writeFile(join(root, 'node_modules/dependency/README.md'), '# Dependency\n');
  await writeFile(join(root, 'AGENTS.md'), '[Dependency](node_modules/dependency/README.md)\n');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_doc_path' });
});

test('symlinked documentation is rejected even when its target exists', async context => {
  const { root } = await fixture(context);
  await symlink(join(root, 'DEVELOPMENT.md'), join(root, '.agents/linked.md'), 'file');
  await assert.rejects(checkDocumentation({ root }), { message: 'invalid_doc_path' });
});

test('CLI failure output omits untrusted link text and paths', async context => {
  const { root } = await fixture(context);
  await writeFile(join(root, 'AGENTS.md'), '[private-doc-payload](file:///private-doc-payload)\n');
  const result = spawnSync(process.execPath, [resolve('.agents/scripts/check-docs.mjs'), '--root', root], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.deepEqual(JSON.parse(result.stderr), { error: 'invalid_doc_link', stage: null, processCode: null, exitCode: null });
  assert.equal(result.stdout, '');
  assert.equal(result.stderr.includes('private-doc-payload'), false);
  assert.equal(result.stderr.includes(root), false);
});

test('example outputs cannot write tracked source even when the repository itself is temporary', async context => {
  const { root } = await fixture(context);
  await assert.rejects(diagnosticOutput(root, join(root, '.agents/generated')), { message: 'invalid_doc_path' });
  const output = await diagnosticOutput(root, join(root, '.verification/docs'));
  assert.equal(output.endsWith(join('.verification', 'docs')), true);
});

test('failed Cargo metadata retains only closed process facts, never borrowed diagnostics', async context => {
  const { root } = await fixture(context);
  const skill = join(root, '.agents/skills/gitview-sql-diagnostics');
  await mkdir(skill, { recursive: true });
  await writeFile(join(skill, 'SKILL.md'), await readFile(resolve('.agents/skills/gitview-sql-diagnostics/SKILL.md')));
  await mkdir(join(root, 'src-tauri'));
  await writeFile(join(root, 'src-tauri/Cargo.toml'), 'private-metadata-payload = [');

  await assert.rejects(exerciseDiagnostics({ root }), error => {
    assert.equal(error.message, 'doc_examples_failed');
    assert.deepEqual(error.cause, { error: 'doc_examples_failed', stage: 'cargo_metadata', processCode: 'check_failed', exitCode: 101 });
    assert.deepEqual(documentationFailureEvidence(error.cause), error.cause);
    assert.equal(JSON.stringify(error.cause).includes('private-metadata-payload'), false);
    assert.equal(JSON.stringify(error.cause).includes(root), false);
    return true;
  });
});

test('borrowed scanner exceptions become one closed CLI failure without filesystem details', async context => {
  const { root } = await fixture(context);
  const missing = join(root, 'private-scanner-payload');
  const result = spawnSync(process.execPath, [resolve('.agents/scripts/check-docs.mjs'), '--root', missing], { encoding: 'utf8' });
  assert.equal(result.status, 1);
  assert.equal(result.stdout, '');
  assert.deepEqual(JSON.parse(result.stderr), { error: 'doc_examples_failed', stage: null, processCode: null, exitCode: null });
  assert.equal(result.stderr.includes(root), false);
  assert.equal(result.stderr.includes('private-scanner-payload'), false);
});

test('documentation failure evidence retains bounded startup and assertion facts', () => {
  const startup = { error: 'doc_examples_failed', stage: 'driver_run', processCode: 'check_failed', exitCode: 3221225785 };
  const assertion = { error: 'doc_examples_failed', stage: 'cli_children', processCode: 'assertion_failed', exitCode: null };
  const invalidOutput = { error: 'invalid_example_output', stage: 'driver_output', processCode: 'invalid_output', exitCode: null };
  assert.deepEqual(documentationFailureEvidence(startup), startup);
  assert.deepEqual(documentationFailureEvidence(assertion), assertion);
  assert.deepEqual(documentationFailureEvidence(invalidOutput), invalidOutput);
});

test('documentation failure evidence rejects untrusted fields and values rather than forwarding them', () => {
  const failure = { error: 'doc_examples_failed', stage: 'driver_run', processCode: 'check_failed', exitCode: 1 };
  assert.equal(documentationFailureEvidence({ ...failure, output: 'private-process-payload' }), null);
  assert.equal(documentationFailureEvidence({ ...failure, cause: { path: 'private-path' } }), null);
  assert.equal(documentationFailureEvidence({ ...failure, error: 'private-error' }), null);
  assert.equal(documentationFailureEvidence({ ...failure, stage: 'private-stage' }), null);
  assert.equal(documentationFailureEvidence({ ...failure, processCode: 'private-code' }), null);
  assert.equal(documentationFailureEvidence({ ...failure, exitCode: 'private-exit' }), null);
  assert.equal(documentationFailureEvidence({ ...failure, exitCode: Number.MAX_SAFE_INTEGER + 1 }), null);
  assert.equal(documentationFailureEvidence({ ...failure, exitCode: 4_294_967_296 }), null);
  assert.equal(documentationFailureEvidence({ ...failure, exitCode: -2_147_483_649 }), null);
  assert.equal(documentationFailureEvidence({ ...failure, exitCode: 1.5 }), null);
  assert.equal(documentationFailureEvidence({ ...failure, processCode: null }), null);
  assert.equal(documentationFailureEvidence({ error: 'doc_examples_failed' }), null);
  assert.equal(documentationFailureEvidence(null), null);
  assert.equal(documentationFailureEvidence([]), null);
});
