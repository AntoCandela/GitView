---
name: gitview-doc-maintenance
description: Check repo-local agent documentation, execute its native SQLite examples, or perform a periodic read-only drift scan and propose bounded code-evidenced maintenance. Use when agent instructions, links, skill contracts, or referenced APIs change.
---

# GitView documentation maintenance

## Keep the instruction map short

[AGENTS.md](../../../AGENTS.md) is the short authoritative map: mandatory workflow, relevant skill triggers and links, not a copy of each skill. The [.agents index](../../README.md) explains available tools. [DEVELOPMENT.md](../../../DEVELOPMENT.md) owns daily commands and evidence limits; [CODE-STYLE.md](../../../CODE-STYLE.md) owns code/test conventions. Keep detailed task instructions in their owning skill, without a competing global installation or consumer-specific copy.

A skill lives in `skills/<name>/SKILL.md` with matching lowercase hyphenated `name` and a nonempty single-line `description` in frontmatter. Links are relative to their document. Use existing Markdown headings for fragments; avoid generated output, symlinks or machine-local absolute destinations.

## Run the real documentation check

From the repository root:

```sh
node .agents/scripts/check-docs.mjs
```

The checker validates repository-owned `.agents` Markdown plus the root instruction maps, local link destinations and heading fragments, and skill frontmatter. It does not fetch online links or recursively scan unrelated application/generated directories. Missing paths, escaped destinations, symlinks, unsupported local URI schemes and invalid fragments fail closed. It does not repair files.

The default command also extracts the exact two Rust and three SQL examples from the [SQL diagnostics skill](../gitview-sql-diagnostics/SKILL.md). It compiles the Rust examples against the real native library, triggers an actual save failure in an owned disposable location, awaits a scoped child, flushes the real SQLite writer, validates the read-only reader and CLI, and executes the SQL with read-only connections and bound parameters. Assertions cover measured duration, parent/child identity, fixed event/code facts, capture health and the exact non-payload schema. Compiler scaffolding is removed even when a check fails. The diagnostic store and isolated native build cache remain in ignored `.verification/`; no tracked Rust example or lockfile is changed.

Cargo dependencies must already be locally cached and native build prerequisites must be available. The standalone driver derives its bundled `rusqlite` requirement from the owning package and reapplies the registered audited Cargo path patches; no external SQLite CLI installation is required. On Windows/MSVC it owns its Common Controls activation dependency, since dependency linker arguments do not propagate to downstream executables. Missing prerequisites fail the command rather than silently skipping examples.

CLI success is a closed aggregate summary. Failure is `{ error, stage, processCode, exitCode, compilerCodes }`, using fixed vocabularies, nullable stage/process facts and a process-sized numeric exit code. Failed builds may retain up to 16 bounded Rust/MSVC diagnostic identifiers; other stages use an empty array. These identifiers are diagnostic clues, not complete compiler reports. `documentationFailureEvidence` and `documentationSucceeded` validate these envelopes for the shared verifier; contradictory success/error fields are invalid. Raw compiler output, private paths, arbitrary source, database rows and exception text are never public evidence.

`exerciseDiagnostics({ root, output, snippets })` is exported by [check-docs.mjs](../../scripts/check-docs.mjs) for local evaluations. `root` is the repository; `output` defaults to its `.verification/docs` and may instead be an explicitly supplied disposable directory under the canonical OS temporary root. Other external destinations and symlink escapes are rejected. Every exercise creates its own private subdirectory and fresh database, never opens an existing user store, and returns canonical paths. Optional `snippets` is exactly `[recordSaveFailureSource, spawnScanSource]`; omit it to use the current skill fences. Candidate sources must retain the skill's `record_save_failure(Instant)` and generic `spawn_scan(parent, work)` function contracts. Success returns `{ database, operationId, childOperationId }` for local querying; those paths and IDs are not printed by the documentation CLI. Supplying Rust is **intentional trusted local code execution**. A temporary Cargo project, worktree or isolated target is not a security sandbox; never execute untrusted candidate code.

## Periodic read-only drift scan

At a periodic maintainer review, or after an owning API changes:

1. Read the root map, relevant skills and their referenced implementation/callers. Use code evidence for API names, ownership, supported commands and privacy boundaries. Do not infer product approval from code.
2. Run the checker and inspect only the bounded failure category. Reproduce a mismatch against the actual owning interface before proposing a change. Online links are unverified, not declared healthy by this local check.
3. Propose a small fix-up listing the document/section, observed implementation evidence, precise mismatch and bounded replacement. Distinguish a confirmed mismatch from an inference or missing prerequisite. Prefer deleting obsolete instructions over preserving compatibility aliases.
4. Leave proposed writing, commits, merges and product/status changes for explicit human review. The scan itself is read-only: no automated rewriting, bulk churn, acceptance promotion or unattended merging.
5. After an approved edit, update all affected maps/callers and execute the real checker. Report exercised boundaries and limits; passing Rust/SQLite examples do not establish native picker, packaged-platform or release readiness.

The checker complements the existing shared verifier and typed diagnostics; do not add a second logger, verifier, arbitrary log payload or global agent configuration.
