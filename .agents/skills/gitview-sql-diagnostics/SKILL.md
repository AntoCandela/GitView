---
name: gitview-sql-diagnostics
description: Query GitView's authorized local SQLite diagnostics, trace failures across renderer/native/Git layers, and add privacy-safe typed instrumentation. Use when investigating errors, warnings, missing logs, capture health, operation correlation, or instrumenting a new native/frontend operation.
---

# GitView SQL diagnostics

## Contract first

GitView already has native-managed diagnostics. Reuse them; do not install an HTTP log server, create `logs/debug.sqlite`, introduce a second logger, or apply a generic `message`/JSON `data` schema.

The equivalent of “message and data” is **a fixed event/code plus typed, bounded metadata**. A human explanation belongs in source documentation or the investigation report, not a persisted free-form message. Arbitrary strings/objects can leak paths, credentials, repository contents and subprocess output.

Read [DEVELOPMENT.md](../../../DEVELOPMENT.md) and [CODE-STYLE.md](../../../CODE-STYLE.md). Inspect the current contracts before changing instrumentation:

- [Native vocabulary, details and operation context](../../../src-tauri/src/diagnostics/types.rs).
- [Lifecycle tracing and outcome classification](../../../src-tauri/src/diagnostic_operation.rs).
- [Schema and read-only validation](../../../src-tauri/src/diagnostics/schema.rs).
- [CLI filters](../../../src-tauri/src/bin/gitview-diagnostics.rs).
- [Renderer contract](../../../src/contracts/Diagnostics.ts) and [platform adapter](../../../src/platform/RepositoryClient.ts).

## 1. Establish authorization and capture health

1. Obtain explicit authorization for the specific local database. Repo access alone does not authorize browsing application-data directories.
2. Reproduce one action, preferably with a disposable repository, and note its time.
3. Inspect `diagnostic_health` through the main-window platform adapter's `diagnosticHealth()` method. Record state, accepted/written/dropped counters and `last_error_code`.
4. Validate the database with the real read-only CLI before querying it.

The host stores `diagnostics/diagnostics.sqlite` under Tauri's per-user app-data directory, outside observed repositories. On macOS, after authorizing this path:

```sh
DB="$HOME/Library/Application Support/com.gitview.app/diagnostics/diagnostics.sqlite"
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" schema
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --level error --limit 20
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --level warn --limit 20
```

Use the actual app-data path and shell syntax on other platforms. Do not guess another user's store. Missing or incompatible stores are not created/repaired by the CLI; preserve bytes and report its fixed error code. Do not wipe the database, run `VACUUM`, change pragmas/schema or dump unrelated tables as a workaround.

## 2. Narrow and follow causality

The CLI supports exact `--level`, `--component`, `--event`, `--operation-id`, `--session-id`, `--since-ms` filters and `--limit` (1–200). Time is nonnegative Unix milliseconds; IDs must be canonical lowercase UUID v4 values. Each option may occur once. An `error` filter does not include warnings.

Copy a returned ID into `OPERATION_ID`, then remove the severity filter to inspect its whole lifecycle:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --operation-id "$OPERATION_ID" --limit 200
```

If `has_more` is true, the result is incomplete. Narrow by operation/session/time/component. Parent IDs link background work; one operation filter does not automatically include its descendants.

### Authorized read-only SQL

Prefer the validated CLI for ordinary triage. If a maintainer authorizes direct SQL against that validated store, use a read-only connection and bounded `SELECT`s. The real table is `events`, not `logs`; timestamps are `timestamp_ms`, not ISO strings, and there are no `message`/`data` columns.

For recent failure facts, with SQLite's CLI installed:

```sh
sqlite3 -readonly -json "$DB" <<'SQL'
SELECT id, timestamp_ms, session_id, operation_id, parent_operation_id,
       operation_kind, component, event, code, duration_ms
FROM events
WHERE level IN ('error', 'warn')
ORDER BY timestamp_ms DESC, id DESC
LIMIT 20;
SQL
```

For a time-filtered query in a SQL client that supports bound parameters:

```sql
SELECT id, timestamp_ms, operation_id, parent_operation_id,
       operation_kind, component, event, code, duration_ms
FROM events
WHERE level IN ('error', 'warn')
  AND timestamp_ms >= :since_ms
ORDER BY timestamp_ms DESC, id DESC
LIMIT 20;
```

Bind `:since_ms` through the database client's parameter API. For a causal slice, bind the validated UUID rather than interpolating untrusted strings:

```sql
SELECT id, operation_id, parent_operation_id, component, event, code,
       duration_ms, exit_code, stdout_bytes, stderr_bytes, cleanup_failed
FROM events
WHERE operation_id = :operation_id
ORDER BY id ASC
LIMIT 200;
```

Direct SQL bypasses the CLI's per-row decoder. Treat unexpected values as untrusted, stop and report corruption; do not publish raw results. Never give the renderer a SQL endpoint.

### Interpretation rules

- Session IDs separate runs; retained rows do not establish current capture health.
- `started` without a terminal fact does not establish an outcome. Queue drops, cancellation or process exit can leave gaps.
- `cancelled` and `superseded` are not ordinary domain failures.
- Process cleanup has separate facts. A cancelled request does not prove its child was reaped.
- Nonzero Git exit status can be expected. Prefer the Git/application classification over guessing from exit status.
- Accepted submissions may be queued, not committed. Queue overflow/storage failure means capture is incomplete; report dropped counts.
- The writer drains at most 64 immediately queued records per full-synchronous transaction and stops before flush/shutdown barriers. Written counts advance only after commit; failed batches roll back rows and retention changes and count all consumed records as dropped.
- Retention is bounded to 20,000 events/seven days at write time. Do not claim complete historical coverage.

## 3. Add a diagnostic fact, not a generic logger

First check whether the boundary already has `OperationTrace`, `trace_operation`, process tracing or adapter capture. Do not emit duplicate start/terminal facts for the same component interval.

Inside an existing native scope, use `OperationContext::current()` and its typed `record()` method. This example maps an actual save failure to an existing fixed code; it is illustrative, not a request to duplicate the already instrumented persistence path:

```rust
use std::time::Instant;
use crate::diagnostics::{Code, Component, DiagnosticDetails, Event, Level, OperationContext};

fn record_save_failure(started: Instant) {
    if let Some(operation) = OperationContext::current() {
        let _ = operation.record(
            Level::Error,
            Component::Persistence,
            Event::Failed,
            Some(Code::SaveFailed),
            DiagnosticDetails {
                duration_ms: Some(started.elapsed().as_millis().min(i64::MAX as u128) as u64),
                ..Default::default()
            },
        );
    }
}
```

Invoke this only after the save actually fails, then preserve its existing result/retry behavior. `record()` returns queue admission, not a persistence acknowledgement. A false return is handled by capture health; do not retry, panic or substitute a logging error for the domain result. No current scope means skip this fact, not invent a disconnected request ID.

`DiagnosticDetails` permits only `duration_ms`, `exit_code`, `stdout_bytes`, `stderr_bytes` and `cleanup_failed`. Durations/byte counts must fit SQLite's signed integer range. Record measured output lengths, never output bytes. Set `cleanup_failed` only from actual cleanup evidence, not from cancellation alone.

### Own complete lifecycles

For a new component interval, reuse `OperationTrace` in the native crate: construct with the current context/component, finish with the actual classified result, and let its Drop behavior mark unfinished work cancelled. Follow `RepositoryService::trace_operation` and existing Git/process wrappers. Do not widen private interfaces just to call them externally. Scoping alone does not emit lifecycle records.

For background work, explicitly derive and scope a child. This helper shows correlation ownership; the work itself must own its lifecycle/cleanup:

```rust
use std::future::Future;
use crate::diagnostics::{OperationContext, OperationKind};

fn spawn_scan<T: Send + 'static>(
    parent: &OperationContext,
    work: impl Future<Output = T> + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    let child = parent.child().with_kind(OperationKind::ScanContext);
    tokio::spawn(async move { child.scope(work).await })
}
```

Keep the returned handle under the existing controller's task ownership; do not detach application work accidentally. Tokio task-local context is not automatically inherited by `spawn`. Reuse the existing request context for synchronous descendants; create child IDs at deliberate background boundaries, not for every log line.

### Frontend instrumentation

Repository commands go through `src/platform/RepositoryClient.ts`. Its adapter already creates the request UUID and records typed terminal transport facts without changing returned/rejected objects. Do not add component-level `invoke`/`fetch` logging or serialize exceptions.

For a new command, migrate the actual adapter call, native main-window handler, capabilities/permission registration and closed renderer vocabulary together. Keep command identity distinct from authorization. `RendererDiagnostic` permits only `operationId`, `command`, `phase` and bounded `durationMs`; the native host rejects unknown fields. Domain rejection still means transport completed, not transport failed. Read the current duration bound from the contract rather than copying a stale constant.

## 4. Extend metadata deliberately

When existing fields cannot explain a failure:

1. Decide which non-sensitive fact is missing and whether a new fixed `Code`, `Event` or `OperationKind` is enough.
2. Add the minimum closed vocabulary at its owner. Update outcome classification, frontend mirrors and query decoding where affected.
3. A new metadata field needs an explicit type, bound and privacy rationale, plus coordinated writer/reader/schema-version handling. Existing stores must not be silently overwritten or declared compatible with a different schema.
4. Do not add `message: String`, `data: Value`, `error.to_string()`, `format!("{error:?}")`, stack traces, paths, branch/file contents, command arguments/output, credentials or environment values. Naming a field “sanitized” is not a privacy guarantee.
5. Do not implement a schema change or product-scope expansion solely because this skill mentions it. Follow the governing approved contract and obtain necessary decisions first.

## 5. Verify and report

Exercise the changed path with real Git/SQLite where appropriate. Prove that records have the expected operation/parent IDs, fixed codes and measured metadata; private payloads are absent; and capture failure does not alter the domain outcome. Preserve cancellation/cleanup ownership and test plausible consumer-visible boundaries, not source text or field wiring alone.

Focused checks:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --test diagnostics --test diagnostic_operations
npm run policy
```

Run the relevant unit/integration category and full shared verification before handoff. Keep examples/fixtures and generated data out of commits. Report authorization scope, reproduced action, queried time/session/operation, relevant safe facts, capture-health gaps and exercised verification. Native picker/packaged-platform readiness still requires separate actual observations.
