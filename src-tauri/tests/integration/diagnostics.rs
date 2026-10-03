//! Exercises durable diagnostics and the explicitly authorized read-only CLI.

use gitview_lib::diagnostics::{Code, Component, DiagnosticDetails, DiagnosticStore, Event, HealthState, Level, OperationContext, Query, ReadOnlyDiagnostics};
use rusqlite::Connection;
use std::{fs, process::Command, time::Duration};
use tempfile::TempDir;

#[path = "../support/mod.rs"]
mod support;

const WAIT: Duration = Duration::from_secs(5);

fn database(temp: &TempDir) -> std::path::PathBuf {
    temp.path().canonicalize().unwrap().join("diagnostics").join("diagnostics.sqlite")
}

fn failure(context: &OperationContext, level: Level) {
    assert!(context.record(level, Component::Git, Event::Failed, Some(Code::NotRepository), DiagnosticDetails::default()));
}

fn cli(path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_gitview-diagnostics"))
        .arg("--database").arg(path).args(args).output().unwrap()
}

#[test]
fn committed_failures_survive_restart_and_filters_exclude_other_traces() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let operation = OperationContext::new(store.sink(), None, None);
    failure(&operation, Level::Error);
    failure(&operation.child(), Level::Warn);
    let other = OperationContext::new(store.sink(), None, None);
    failure(&other, Level::Error);
    store.flush(WAIT).unwrap();
    let session = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap().events[0].session_id;
    store.shutdown(WAIT).unwrap();
    let reopened = DiagnosticStore::open(&path);
    let mut query = Query::default();
    query.operation_id = Some(operation.id());
    query.session_id = Some(session);
    query.level = Some(Level::Error);
    query.component = Some(Component::Git);
    query.event = Some(Event::Failed);
    query.since_ms = Some(0);
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&query).unwrap();
    assert_eq!(rows.events.len(), 1);
    assert_eq!(rows.events[0].code, Some(Code::NotRepository));
    assert_eq!(rows.events[0].operation_id, operation.id());
    query.since_ms = Some(i64::MAX);
    assert!(ReadOnlyDiagnostics::open(&path).unwrap().events(&query).unwrap().events.is_empty());
    reopened.shutdown(WAIT).unwrap();
}

#[test]
fn hot_rollback_journal_restores_committed_events_and_capture() {
    support::terminate_diagnostic_transaction_if_requested();
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let committed = OperationContext::new(store.sink(), None, None);
    failure(&committed, Level::Error);
    store.shutdown(WAIT).unwrap();
    support::interrupt_diagnostic_transaction(&path, "hot_rollback_journal_restores_committed_events_and_capture");
    let journal = path.with_extension("sqlite-journal");
    let database_before = fs::read(&path).unwrap();
    let journal_before = fs::read(&journal).unwrap();

    assert_eq!(ReadOnlyDiagnostics::open(&path).err(), Some(Code::Schema));
    assert!(!cli(&path, &["events"]).status.success());
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);

    let restarted = DiagnosticStore::open(&path);
    assert_eq!(restarted.health().state, HealthState::Healthy);
    assert_eq!(restarted.health().last_error_code, None);
    let retained = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(retained.events.len(), 1);
    assert_eq!(retained.events[0].operation_id, committed.id());
    assert_eq!(retained.events[0].code, Some(Code::NotRepository));
    assert_eq!(retained.events[0].level, Level::Error);
    assert!(!journal.exists());

    let next = OperationContext::new(restarted.sink(), None, None);
    failure(&next, Level::Warn);
    restarted.shutdown(WAIT).unwrap();
    assert_eq!(restarted.health().accepted, 1);
    assert_eq!(restarted.health().written, 1);
    let durable = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(durable.events.len(), 2);
    assert_eq!(durable.events[0].operation_id, next.id());
    assert_eq!(durable.events[0].level, Level::Warn);
    assert_eq!(durable.events[1].operation_id, committed.id());
    assert_eq!(durable.events[1].code, Some(Code::NotRepository));
}

#[test]
fn incompatible_hot_rollback_journal_preserves_database_and_journal_bytes() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    failure(&OperationContext::new(store.sink(), None, None), Level::Error);
    store.shutdown(WAIT).unwrap();
    // The interrupted transaction writes version 1; only recovered committed bytes reveal version 99.
    Connection::open(&path).unwrap().pragma_update(None, "user_version", 99).unwrap();
    support::interrupt_diagnostic_transaction(&path, "hot_rollback_journal_restores_committed_events_and_capture");
    let journal = path.with_extension("sqlite-journal");
    let database_before = fs::read(&path).unwrap();
    let journal_before = fs::read(&journal).unwrap();

    let rejected = DiagnosticStore::open(&path);
    assert_eq!(rejected.health().state, HealthState::Degraded);
    assert_eq!(rejected.health().last_error_code, Some(Code::Schema));
    assert!(rejected.flush(WAIT).is_err());
    assert_eq!(rejected.health().written, 0);
    assert!(!cli(&path, &["schema"]).status.success());
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
}

#[test]
fn current_version_hot_journal_with_unapproved_trigger_remains_untouched() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    failure(&OperationContext::new(store.sink(), None, None), Level::Error);
    store.shutdown(WAIT).unwrap();
    Connection::open(&path).unwrap().execute_batch(
        "CREATE TRIGGER unapproved AFTER INSERT ON events BEGIN UPDATE events SET code='save_failed'; END;"
    ).unwrap();
    support::interrupt_diagnostic_transaction(&path, "hot_rollback_journal_restores_committed_events_and_capture");
    let journal = path.with_extension("sqlite-journal");
    let database_before = fs::read(&path).unwrap();
    let journal_before = fs::read(&journal).unwrap();

    let rejected = DiagnosticStore::open(&path);
    assert_eq!(rejected.health().state, HealthState::Degraded);
    assert_eq!(rejected.health().last_error_code, Some(Code::Schema));
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
}

#[test]
fn malformed_hot_journal_size_cannot_expand_or_modify_the_store() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    failure(&OperationContext::new(store.sink(), None, None), Level::Error);
    store.shutdown(WAIT).unwrap();
    support::interrupt_diagnostic_transaction(&path, "hot_rollback_journal_restores_committed_events_and_capture");
    let journal = path.with_extension("sqlite-journal");
    let mut journal_before = fs::read(&journal).unwrap();
    // A malformed original-page count must not let private or source recovery grow past 64 MiB.
    journal_before[16..20].copy_from_slice(&16385_u32.to_be_bytes());
    fs::write(&journal, &journal_before).unwrap();
    let database_before = fs::read(&path).unwrap();

    let rejected = DiagnosticStore::open(&path);
    assert_eq!(rejected.health().state, HealthState::Degraded);
    assert_eq!(rejected.health().last_error_code, Some(Code::Schema));
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
}

#[test]
fn corrupt_hot_journal_records_preserve_original_database_and_journal() {
    for truncated in [true, false] {
        let temp = TempDir::new().unwrap();
        let path = database(&temp);
        let store = DiagnosticStore::open(&path);
        failure(&OperationContext::new(store.sink(), None, None), Level::Error);
        store.shutdown(WAIT).unwrap();
        support::interrupt_diagnostic_transaction(&path, "hot_rollback_journal_restores_committed_events_and_capture");
        let journal = path.with_extension("sqlite-journal");
        let mut journal_before = fs::read(&journal).unwrap();
        let sector = u32::from_be_bytes(journal_before[20..24].try_into().unwrap()) as usize;
        assert!(u32::from_be_bytes(journal_before[8..12].try_into().unwrap()) > 0);
        if truncated {
            journal_before.truncate(sector);
        } else {
            // Damage only the first checksum: spilled database pages remain structurally valid.
            journal_before[sector + 4 + 4096] ^= 1;
        }
        fs::write(&journal, &journal_before).unwrap();
        let database_before = fs::read(&path).unwrap();

        let rejected = DiagnosticStore::open(&path);
        assert_eq!(rejected.health().state, HealthState::Degraded);
        assert_eq!(rejected.health().last_error_code, Some(Code::Schema));
        assert_eq!(fs::read(&path).unwrap(), database_before);
        assert_eq!(fs::read(&journal).unwrap(), journal_before);
    }
}

#[test]
fn capture_startup_does_not_recover_a_concurrent_uncommitted_writer() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    failure(&OperationContext::new(store.sink(), None, None), Level::Error);
    store.shutdown(WAIT).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE; UPDATE events SET code='save_failed';").unwrap();
    connection.cache_flush().unwrap();
    let journal = path.with_extension("sqlite-journal");
    let database_before = fs::read(&path).unwrap();
    let journal_before = fs::read(&journal).unwrap();

    let rejected = DiagnosticStore::open(&path);
    assert_eq!(rejected.health().state, HealthState::Degraded);
    assert_eq!(rejected.health().written, 0);
    assert_eq!(fs::read(&path).unwrap(), database_before);
    assert_eq!(fs::read(&journal).unwrap(), journal_before);
    connection.execute_batch("ROLLBACK").unwrap();
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(rows.events[0].code, Some(Code::NotRepository));
}

#[test]
fn orderly_shutdown_drains_accepted_records_and_closes_all_sink_clones() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    failure(&context, Level::Error);
    failure(&context, Level::Warn);
    store.shutdown(WAIT).unwrap();
    assert!(!context.record(Level::Error, Component::Git, Event::Failed, None, DiagnosticDetails::default()));
    assert_eq!(store.health().written, 2);
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(rows.events.len(), 2);
    assert_eq!(rows.events[0].level, Level::Warn);
    assert_eq!(rows.events[1].level, Level::Error);
}

#[test]
fn query_limit_reports_more_without_returning_extra_rows() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    failure(&context, Level::Error);
    failure(&context, Level::Warn);
    store.flush(WAIT).unwrap();
    let query = Query { limit: 1, ..Query::default() };
    let result = ReadOnlyDiagnostics::open(&path).unwrap().events(&query).unwrap();
    assert_eq!(result.events.len(), 1);
    assert!(result.has_more);
    assert_eq!(result.events[0].level, Level::Warn);
    assert_eq!(ReadOnlyDiagnostics::open(&path).unwrap().events(&Query { limit: 201, ..Query::default() }).unwrap_err(), Code::InvalidArguments);
    store.shutdown(WAIT).unwrap();
}

#[test]
fn expired_and_excess_rows_are_deleted_before_next_commit() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    failure(&context, Level::Error);
    store.flush(WAIT).unwrap();
    store.shutdown(WAIT).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection.execute("UPDATE events SET timestamp_ms=0", []).unwrap();
    connection.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<20005) INSERT INTO events(timestamp_ms,session_id,operation_id,level,component,event) SELECT 4102444800000,'c199f55d-99e4-4b80-8ca5-5161a0d04d55','8346908f-27a8-4629-9a7c-b3603682bc7b','info','git','completed' FROM n;").unwrap();
    drop(connection);
    let reopened = DiagnosticStore::open(&path);
    failure(&OperationContext::new(reopened.sink(), None, None), Level::Warn);
    reopened.flush(WAIT).unwrap();
    let connection = Connection::open(&path).unwrap();
    assert_eq!(connection.query_row("SELECT count(*) FROM events", [], |row| row.get::<_, i64>(0)).unwrap(), 20000);
    assert_eq!(connection.query_row("SELECT count(*) FROM events WHERE timestamp_ms=0", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.query_row("PRAGMA page_size", [], |row| row.get::<_, i64>(0)).unwrap(), 4096);
    assert!(fs::metadata(&path).unwrap().len() <= 64 * 1024 * 1024);
    reopened.shutdown(WAIT).unwrap();
}

#[test]
fn storage_failure_and_overflow_are_visible_without_database_access() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN EXCLUSIVE").unwrap();
    for _ in 0..10000 {
        context.record(Level::Error, Component::Git, Event::Failed, None, DiagnosticDetails::default());
    }
    assert!(store.health().dropped > 0);
    assert_eq!(store.health().state, HealthState::Degraded);
    assert!(store.flush(WAIT).is_err());
    assert!(store.health().last_error_code.is_some());
    connection.execute_batch("ROLLBACK").unwrap();
    let _ = store.shutdown(WAIT);
}

#[test]
fn missing_and_incompatible_stores_remain_untouched_and_degraded() {
    let temp = TempDir::new().unwrap();
    let missing = database(&temp);
    assert_eq!(ReadOnlyDiagnostics::open(&missing).err(), Some(Code::StorageUnavailable));
    assert!(!missing.exists());
    fs::create_dir(missing.parent().unwrap()).unwrap();
    let connection = Connection::open(&missing).unwrap();
    connection.execute_batch("CREATE TABLE protected(secret TEXT); INSERT INTO protected VALUES('private secret /home/user/repo'); PRAGMA user_version=99;").unwrap();
    drop(connection);
    let before = fs::read(&missing).unwrap();
    let store = DiagnosticStore::open(&missing);
    assert_eq!(store.health().state, HealthState::Degraded);
    assert_eq!(store.health().last_error_code, Some(Code::Schema));
    assert!(store.flush(WAIT).is_err());
    assert_eq!(ReadOnlyDiagnostics::open(&missing).err(), Some(Code::Schema));
    assert_eq!(before, fs::read(&missing).unwrap());
}

#[test]
fn malformed_persisted_text_is_not_returned_as_diagnostic_payload() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    failure(&OperationContext::new(store.sink(), None, None), Level::Error);
    store.shutdown(WAIT).unwrap();
    Connection::open(&path).unwrap().execute("UPDATE events SET code='token-secret /private/repository'", []).unwrap();
    assert_eq!(ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap_err(), Code::InvalidRecord);
    let output = cli(&path, &["events"]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("token-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("token-secret"));
}

#[test]
fn cli_requires_explicit_grant_and_distinguishes_missing_from_empty_without_writes() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let denied = Command::new(env!("CARGO_BIN_EXE_gitview-diagnostics")).arg("events").output().unwrap();
    assert!(!denied.status.success());
    let missing = cli(&path, &["events"]);
    assert!(!missing.status.success());
    assert!(!path.exists());
    let store = DiagnosticStore::open(&path);
    store.shutdown(WAIT).unwrap();
    let before = fs::read(&path).unwrap();
    let empty = cli(&path, &["events", "--level", "error"]);
    assert!(empty.status.success());
    let json: serde_json::Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(json["events"], serde_json::json!([]));
    assert_eq!(json["has_more"], false);
    let schema = cli(&path, &["schema"]);
    assert!(schema.status.success());
    let json: serde_json::Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(json["version"], 1);
    assert!(json["sql"].as_str().unwrap().contains("timestamp_ms"));
    assert!(!cli(&path, &["events", "--limit", "201"]).status.success());
    assert!(!cli(&path, &["events", "--event", "DROP TABLE events"]).status.success());
    assert!(!cli(&path, &["events", "--operation-id", "not-a-uuid"]).status.success());
    assert_eq!(before, fs::read(&path).unwrap());
    assert!(!path.with_extension("sqlite-journal").exists());
}

#[cfg(unix)]
#[test]
fn database_is_private_and_symlink_destinations_are_never_written() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    store.shutdown(WAIT).unwrap();
    assert_eq!(fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    let target = temp.path().canonicalize().unwrap().join("untouched");
    fs::write(&target, b"secret").unwrap();
    let linked = temp.path().canonicalize().unwrap().join("linked.sqlite");
    symlink(&target, &linked).unwrap();
    assert_eq!(DiagnosticStore::open(&linked).health().state, HealthState::Degraded);
    assert_eq!(fs::read(&target).unwrap(), b"secret");
    let linked_directory = temp.path().canonicalize().unwrap().join("linked-directory");
    symlink(path.parent().unwrap(), &linked_directory).unwrap();
    assert_eq!(DiagnosticStore::open(linked_directory.join("new.sqlite")).health().state, HealthState::Degraded);
    assert!(!path.parent().unwrap().join("new.sqlite").exists());
}

#[tokio::test]
async fn scoped_context_tracks_child_identity_without_leaking_after_completion() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let root = OperationContext::new(store.sink(), None, None);
    assert!(OperationContext::current().is_none());
    root.scope(async {
        assert_eq!(OperationContext::current().unwrap().id(), root.id());
        let child = root.child();
        assert_eq!(child.parent_id(), Some(root.id()));
        child.scope(async {
            failure(&OperationContext::current().unwrap(), Level::Warn);
        }).await;
        assert_eq!(OperationContext::current().unwrap().id(), root.id());
    }).await;
    assert!(OperationContext::current().is_none());
    store.shutdown(WAIT).unwrap();
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(rows.events[0].parent_operation_id, Some(root.id()));
}

#[test]
fn out_of_range_metadata_is_rejected_without_losing_accepted_evidence() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    failure(&context, Level::Warn);
    assert!(!context.record(Level::Error, Component::Process, Event::Failed, None, DiagnosticDetails {
        stdout_bytes: Some(u64::MAX), ..DiagnosticDetails::default()
    }));
    store.flush(WAIT).unwrap();
    assert_eq!(store.health().accepted, 1);
    assert_eq!(store.health().written, 1);
    assert_eq!(store.health().dropped, 1);
    assert_eq!(store.health().last_error_code, Some(Code::InvalidRecord));
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(rows.events.len(), 1);
    assert_eq!(rows.events[0].level, Level::Warn);
    store.shutdown(WAIT).unwrap();
}

#[test]
fn incompatible_current_version_schema_is_not_replaced() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    fs::create_dir(path.parent().unwrap()).unwrap();
    Connection::open(&path).unwrap().execute_batch("CREATE TABLE events(secret TEXT); INSERT INTO events VALUES('private payload'); PRAGMA user_version=1;").unwrap();
    let before = fs::read(&path).unwrap();
    assert_eq!(DiagnosticStore::open(&path).health().last_error_code, Some(Code::Schema));
    let output = cli(&path, &["schema"]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private payload"));
    assert_eq!(before, fs::read(&path).unwrap());
}

#[test]
fn sqlite_prefix_lookalikes_are_rejected_before_writes_or_schema_disclosure() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    store.shutdown(WAIT).unwrap();
    // `sqlite_` is reserved; `sqliteX` is user-defined and must not bypass the fixed schema.
    Connection::open(&path).unwrap().execute_batch(
        "CREATE TRIGGER sqliteXrewrite AFTER INSERT ON events BEGIN DELETE FROM events WHERE id = NEW.id; END;
         CREATE INDEX sqliteXprivate ON events(code /* private-index-token */);"
    ).unwrap();
    let before = fs::read(&path).unwrap();
    assert_eq!(ReadOnlyDiagnostics::open(&path).err(), Some(Code::Schema));
    let rejected = DiagnosticStore::open(&path);
    assert_eq!(rejected.health().last_error_code, Some(Code::Schema));
    assert_eq!(rejected.health().written, 0);
    let output = cli(&path, &["schema"]);
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-index-token"));
    assert_eq!(before, fs::read(&path).unwrap());
}

#[test]
fn shutdown_timeout_is_bounded_and_never_claims_durability() {
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None);
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("BEGIN EXCLUSIVE").unwrap();
    failure(&context, Level::Warn);
    let started = std::time::Instant::now();
    assert_eq!(store.shutdown(Duration::from_millis(1)), Err(Code::Timeout));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(store.health().state, HealthState::Degraded);
    assert!(!context.record(Level::Error, Component::Git, Event::Failed, None, DiagnosticDetails::default()));
    connection.execute_batch("ROLLBACK").unwrap();
    store.shutdown(WAIT).unwrap();
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query::default()).unwrap();
    assert_eq!(rows.events.len(), 1);
    assert_eq!(rows.events[0].level, Level::Warn);
}

#[test]
fn cli_returns_only_selected_durable_trace_with_safe_numeric_context() {
    use gitview_lib::diagnostics::OperationKind;
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    let context = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::OpenRepository);
    assert!(context.record(Level::Info, Component::Process, Event::Completed, None, DiagnosticDetails {
        duration_ms: Some(23), exit_code: Some(128), stdout_bytes: Some(0), stderr_bytes: Some(17), cleanup_failed: false
    }));
    failure(&context.child(), Level::Error);
    store.shutdown(WAIT).unwrap();
    let before = fs::read(&path).unwrap();
    let output = cli(&path, &["events", "--operation-id", &context.id().to_string(), "--level", "info", "--component", "process", "--event", "completed", "--since-ms", "0", "--limit", "1"]);
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["events"].as_array().unwrap().len(), 1);
    assert_eq!(json["events"][0]["operation_kind"], "open_repository");
    assert_eq!(json["events"][0]["exit_code"], 128);
    assert_eq!(json["events"][0]["stderr_bytes"], 17);
    assert_eq!(json["events"][0]["duration_ms"], 23);
    assert_eq!(json["events"][0]["code"], serde_json::Value::Null);
    assert_eq!(before, fs::read(&path).unwrap());
}

#[test]
fn disabled_sink_is_never_reported_healthy() {
    let sink = gitview_lib::diagnostics::DiagnosticSink::default();
    assert_eq!(sink.health().state, HealthState::Disabled);
    let context = OperationContext::new(sink.clone(), None, None);
    assert!(!context.record(Level::Warn, Component::Git, Event::Failed, None, DiagnosticDetails::default()));
    assert_eq!(sink.health().state, HealthState::Disabled);
    assert_eq!(sink.health().accepted, 0);
    assert_eq!(sink.health().dropped, 1);
}

#[cfg(unix)]
#[test]
fn preexisting_journal_symlink_cannot_redirect_writer_output() {
    use std::os::unix::fs::symlink;
    let temp = TempDir::new().unwrap();
    let path = database(&temp);
    let store = DiagnosticStore::open(&path);
    store.shutdown(WAIT).unwrap();
    let before = fs::read(&path).unwrap();
    let target = temp.path().canonicalize().unwrap().join("private-target");
    fs::write(&target, b"secret untouched bytes").unwrap();
    symlink(&target, path.with_extension("sqlite-journal")).unwrap();
    let reopened = DiagnosticStore::open(&path);
    assert_eq!(reopened.health().state, HealthState::Degraded);
    assert_eq!(fs::read(&target).unwrap(), b"secret untouched bytes");
    assert_eq!(fs::read(&path).unwrap(), before);
}
