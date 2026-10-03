---
name: gitview-verification
description: Run GitView's shared unit/integration verification, diagnose failed sanitized manifests, use local Git hooks, and distinguish test/browser evidence from native-preview readiness. Use when validating a change, investigating a failed check, preparing a commit/push, or reporting release evidence.
---

# GitView verification and evidence

Read [DEVELOPMENT.md](../../../DEVELOPMENT.md) and [CODE-STYLE.md](../../../CODE-STYLE.md). Reuse existing infrastructure; do not introduce a second runner or label browser results native certification.

## Choose checks by boundary

From the repository root:

```sh
npm run test:unit
npm run test:integration
npm run policy
npm run verify
```

The full verifier runs frontend/native builds, executable agent documentation, unit/integration groups, policy and strict publication-license/source provenance checks. CI and pre-push use the same required checks. The license gate needs installed locked npm dependencies, `tar`, `curl` and access to the recorded public upstream sources; missing prerequisites fail closed. It verifies vendored patches independently because registry advisory scans omit Cargo path dependencies. `npm run agents:docs` also runs the documentation check directly; it compiles and exercises the SQL skill examples against isolated SQLite. Focused native reruns retain `--target-dir .verification/native-target` to isolate verification artifacts from Tauri development/packaging builds.

Frontend test bodies belong in `tests/unit` or `tests/integration`, infrastructure scripts in `tests/infrastructure`, reusable support in `tests/support`. Native bodies belong in `src-tauri/tests/unit` or `integration`; support in `src-tauri/tests/support`. Preserve private owner registrations and declared Cargo targets rather than widening APIs.

Use isolated fixtures, not users' repositories, workspace data, diagnostic stores or global configuration. Prefer tests of consumer-visible behavior, transitions and boundaries, not source text, mock forwarding or incidental defaults. After meaningful code changes, exercise the actual changed surface; tests alone do not prove runtime behavior.

## Inspect failures without publishing private output

```sh
node .agents/scripts/verification-report.mjs --manifest .verification/manifest.json
node .agents/scripts/verification-report.mjs --manifest .verification/pre-push/manifest.json
```

The utility reports recorded status and safe counts/check IDs, omits raw descriptions and regenerates current rerun commands from the shared runner. It does not execute supplied manifest commands or certify freshness. Paths in these examples are relative to the repository root, not this skill directory.

The shared runner's `failures` may include `source:` identifiers for catalogued test files and in-range source lines, bounded reported test durations, fixed timeout/assertion classifications, closed documentation stage/process facts, and bounded `compiler:` diagnostic identifiers from failed documentation builds. These add context when parameterized names must be omitted; they never contain parameter values, raw stacks or runtime filesystem locations. Several identifiers can describe one failed case: use the framework summary for test counts. The report utility still omits these supplied descriptions rather than trusting them as current source evidence.

Rust subprocess regressions can print nested harness results. The runner uses the last complete, line-anchored count summary from the enclosing harness, so a successful child cannot hide ignored or failed enclosing cases. Nonzero process status still takes precedence over reported success.

Exit `0` means the supplied report records a pass; `1` a recorded failure; `2` invalid/unreadable evidence or arguments. An invalid report is not a pass. Manifest dates/revision/dirty state describe the original run; compare that context with the change being reviewed.

For a failure:

1. Identify the failed check/code and use its current rerun command locally.
2. Inspect detailed output locally. Do not attach raw exceptions, process output, paths or environment values to shared reports.
3. Reproduce and resolve the cause at the owning boundary; do not suppress it in another layer.
4. Rerun affected checks and the full verifier. Keep skipped, TODO, missing and not-run evidence explicitly unverified.

A failed compile or abnormal termination can have no framework test summary. An empty failure-name list does not establish success. Do not increase stack limits, force serial gates, remove meaningful checks or re-pin implementation-wording tests to manufacture a pass. Isolated artifact targets do not establish the root cause of a runtime error.

## Git hooks

```sh
npm run hooks:install
git hook run pre-commit
git hook run pre-push
```

Pre-commit checks policy/unit groups; pre-push checks full verification. The latter commands exercise installed hooks without creating a commit or pushing. Installation preserves custom hooks and active default hook ownership. Hooks do not stage, stash, auto-fix or commit source.

Hook evidence is under ignored `.verification/pre-commit` and `.verification/pre-push`. Hooks inspect the current worktree, not a reconstructed staged index or each pushed ref. Review partial staging and rely on CI for the checked-out revision; local hooks remain bypassable.

## Native-preview boundary

```sh
npm run tauri build
npm run preview:evidence -- --artifact path/to/preview --platform macos-arm64 --report path/to/observation.json
```

Use the actual built artifact and observations from that exact platform/artifact. [README.md](../../../README.md#native-preview-evidence) describes the report contract; [preview-evidence.mjs](../../../scripts/preview-evidence.mjs) defines required scenarios.

Compilation, Chromium fixtures and MockRuntime do not prove install/launch, native picker or live secondary-window restrictions. Do not derive `observed: true` from them. Missing/failed/blocked/not-run human observations leave readiness false. Signing, notarization and public-release readiness remain separate; a schema-valid human report is not independent certification.

## Handoff

State the changed boundary, checks and real scenarios exercised, safe failure facts and all unverified platform/capture limitations in the public issue/PR or task conversation. Keep generated evidence/build caches ignored, remove owned temporary scaffolds and stop only services you own. Update a governing Auto-K task only for explicitly authorized maintainer-private work, without promoting protected statuses or changing product intent; do not expose private identifiers or payloads in public handoffs.
