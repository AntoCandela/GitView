//! Exercises queued diagnostic durability, control barriers and retention against isolated SQLite.

use super::*;
use crate::diagnostics::{Component, DiagnosticDetails, Event, Level, OperationKind, Query, ReadOnlyDiagnostics, StoredEvent};
use tempfile::TempDir;

fn queued_writer(path: &Path) -> (Connection, Receiver<Message>, DiagnosticSink) {
    let (connection, _) = initialize(path).unwrap();
    let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
    let shared = Arc::new(Shared { admission: Mutex::new(Admission { sender, closed: false }), health: Health::new(HealthState::Healthy), stop: AtomicBool::new(false) });
    (connection, receiver, DiagnosticSink { shared })
}

fn record(sequence: u64) -> DiagnosticRecord {
    DiagnosticRecord {
        operation_id: Uuid::new_v4(), parent_operation_id: None, operation_kind: OperationKind::Unknown,
        level: Level::Info, component: Component::Git, event: Event::Completed, code: None,
        details: DiagnosticDetails { duration_ms: Some(sequence), ..DiagnosticDetails::default() },
    }
}

fn queue_at(sink: &DiagnosticSink, sequence: u64, timestamp_ms: i64) {
    // Fixed timestamps exercise retention without changing or waiting for the system clock.
    let admission = sink.shared.admission.lock().unwrap_or_else(|poison| poison.into_inner());
    admission.sender.try_send(Message::Record { record: record(sequence), timestamp_ms }).unwrap();
    sink.shared.health.accepted.fetch_add(1, Ordering::Relaxed);
}

fn queue_control(sink: &DiagnosticSink, shutdown: bool) -> Receiver<Result<(), Code>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    let mut admission = sink.shared.admission.lock().unwrap_or_else(|poison| poison.into_inner());
    if shutdown { admission.closed = true; }
    admission.sender.try_send(if shutdown { Message::Shutdown(sender) } else { Message::Flush(sender) }).unwrap();
    receiver
}

fn run_queued(connection: Connection, receiver: Receiver<Message>, sink: &DiagnosticSink) {
    let stored_count = connection.query_row("SELECT count(*) FROM events", [], |row| row.get(0)).unwrap();
    // Start only after admission is complete, so batch membership and barriers are deterministic.
    writer_loop(connection, stored_count, receiver, sink.shared.clone());
}

fn seed_events(connection: &Connection, count: i64, timestamp: i64) {
    connection.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<?1) INSERT INTO events(timestamp_ms,session_id,operation_id,level,component,event) SELECT ?2,'c199f55d-99e4-4b80-8ca5-5161a0d04d55','8346908f-27a8-4629-9a7c-b3603682bc7b','info','git','completed' FROM n", params![count, timestamp]).unwrap();
}

fn events(path: &Path) -> Vec<StoredEvent> {
    let result = ReadOnlyDiagnostics::open(path).unwrap().events(&Query { limit: 200, ..Query::default() }).unwrap();
    assert!(!result.has_more);
    result.events
}

#[test]
fn shutdown_drains_multiple_batches_in_fifo_order_and_seals_producers() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    for sequence in 0..130 { assert!(sink.record(record(sequence))); }
    let shutdown = queue_control(&sink, true);
    assert_eq!(sink.health().accepted, 130);
    assert_eq!(sink.health().written, 0);

    run_queued(connection, receiver, &sink);

    assert_eq!(shutdown.recv().unwrap(), Ok(()));
    assert!(!sink.record(record(130)));
    let health = sink.health();
    assert_eq!(health.state, HealthState::Stopped);
    assert_eq!(health.accepted, 130);
    assert_eq!(health.written, 130);
    assert_eq!(health.dropped, 1);
    assert_eq!(health.last_error_code, None);
    let sequences: Vec<_> = events(&path).iter().map(|event| event.duration_ms.unwrap()).collect();
    assert_eq!(sequences, (0..130).rev().collect::<Vec<_>>());
}

#[test]
fn failed_batch_rolls_back_rows_and_retention_without_claiming_partial_durability() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    seed_events(&connection, 1, 0);
    connection.execute_batch("CREATE TEMP TRIGGER reject_event BEFORE INSERT ON main.events WHEN NEW.duration_ms=2 BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    queue_at(&sink, 1, MAX_AGE_MS + 1);
    queue_at(&sink, 2, MAX_AGE_MS + 2);
    queue_at(&sink, 3, MAX_AGE_MS + 3);
    let flush = queue_control(&sink, false);
    assert!(sink.record(record(4)));
    let shutdown = queue_control(&sink, true);

    run_queued(connection, receiver, &sink);

    assert_eq!(flush.recv().unwrap(), Err(Code::Storage));
    assert_eq!(shutdown.recv().unwrap(), Err(Code::Storage));
    let health = sink.health();
    assert_eq!(health.state, HealthState::Degraded);
    assert_eq!(health.accepted, 4);
    assert_eq!(health.written, 0);
    assert_eq!(health.dropped, 4);
    assert_eq!(health.last_error_code, Some(Code::Storage));
    let rows = events(&path);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].timestamp_ms, 0);
    assert_eq!(rows[0].duration_ms, None);
}

#[test]
fn flush_barrier_preserves_earlier_durability_when_later_records_fail() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    connection.execute_batch("CREATE TEMP TRIGGER reject_event BEFORE INSERT ON main.events WHEN NEW.duration_ms=4 BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    assert!(sink.record(record(1)));
    assert!(sink.record(record(2)));
    let first_flush = queue_control(&sink, false);
    assert!(sink.record(record(3)));
    assert!(sink.record(record(4)));
    let second_flush = queue_control(&sink, false);
    let shutdown = queue_control(&sink, true);

    run_queued(connection, receiver, &sink);

    assert_eq!(first_flush.recv().unwrap(), Ok(()));
    assert_eq!(second_flush.recv().unwrap(), Err(Code::Storage));
    assert_eq!(shutdown.recv().unwrap(), Err(Code::Storage));
    assert_eq!(sink.health().accepted, 4);
    assert_eq!(sink.health().written, 2);
    assert_eq!(sink.health().dropped, 2);
    let sequences: Vec<_> = events(&path).iter().map(|event| event.duration_ms.unwrap()).collect();
    assert_eq!(sequences, [2, 1]);
}

#[test]
fn later_batch_failure_preserves_earlier_committed_records() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    connection.execute_batch("CREATE TEMP TRIGGER reject_event BEFORE INSERT ON main.events WHEN NEW.duration_ms=66 BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    for sequence in 1..=70 { assert!(sink.record(record(sequence))); }
    let flush = queue_control(&sink, false);
    let shutdown = queue_control(&sink, true);

    run_queued(connection, receiver, &sink);

    assert_eq!(flush.recv().unwrap(), Err(Code::Storage));
    assert_eq!(shutdown.recv().unwrap(), Err(Code::Storage));
    assert_eq!(sink.health().accepted, 70);
    assert_eq!(sink.health().written, 64);
    assert_eq!(sink.health().dropped, 6);
    let sequences: Vec<_> = events(&path).iter().map(|event| event.duration_ms.unwrap()).collect();
    assert_eq!(sequences, (1..=64).rev().collect::<Vec<_>>());
}

#[test]
fn commit_failure_never_counts_inserted_rows_as_written() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    reader.execute_batch("BEGIN").unwrap();
    assert_eq!(reader.query_row("SELECT count(*) FROM events", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert!(sink.record(record(1)));
    assert!(sink.record(record(2)));
    let flush = queue_control(&sink, false);
    let shutdown = queue_control(&sink, true);

    // The held shared lock allows inserts but deterministically refuses the DELETE-journal commit.
    run_queued(connection, receiver, &sink);
    reader.execute_batch("ROLLBACK").unwrap();

    assert_eq!(flush.recv().unwrap(), Err(Code::Storage));
    assert_eq!(shutdown.recv().unwrap(), Err(Code::Storage));
    let health = sink.health();
    assert_eq!(health.state, HealthState::Degraded);
    assert_eq!(health.accepted, 2);
    assert_eq!(health.written, 0);
    assert_eq!(health.dropped, 2);
    assert_eq!(health.last_error_code, Some(Code::Storage));
    assert!(events(&path).is_empty());
}

#[test]
fn batched_retention_keeps_the_newest_twenty_thousand_records() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    seed_events(&connection, 20000, i64::MAX);
    queue_at(&sink, 1, 10);
    queue_at(&sink, 2, 11);
    queue_at(&sink, 3, 12);
    let shutdown = queue_control(&sink, true);

    run_queued(connection, receiver, &sink);

    assert_eq!(shutdown.recv().unwrap(), Ok(()));
    assert_eq!(sink.health().accepted, 3);
    assert_eq!(sink.health().written, 3);
    assert_eq!(sink.health().dropped, 0);
    let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(reader.query_row("SELECT count(*), min(id), max(id) FROM events", [], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))).unwrap(), (20000, 4, 20003));
    let result = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query { limit: 3, ..Query::default() }).unwrap();
    assert!(result.has_more);
    let timestamps: Vec<_> = result.events.iter().map(|event| event.timestamp_ms).collect();
    assert_eq!(timestamps, [12, 11, 10]);
}

#[test]
fn batched_age_retention_preserves_cutoff_and_each_records_clock_order() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let (connection, receiver, sink) = queued_writer(&path);
    seed_events(&connection, 1, 99);
    seed_events(&connection, 1, 100);
    seed_events(&connection, 1, 101);
    queue_at(&sink, 1, 100);
    queue_at(&sink, 2, MAX_AGE_MS + 101);
    queue_at(&sink, 3, 100);
    let shutdown = queue_control(&sink, true);

    run_queued(connection, receiver, &sink);

    assert_eq!(shutdown.recv().unwrap(), Ok(()));
    assert_eq!(sink.health().accepted, 3);
    assert_eq!(sink.health().written, 3);
    assert_eq!(sink.health().dropped, 0);
    let retained: Vec<_> = events(&path).iter().map(|event| (event.timestamp_ms, event.duration_ms)).collect();
    assert_eq!(retained, [(100, Some(3)), (MAX_AGE_MS + 101, Some(2)), (101, None)]);
}
