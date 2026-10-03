/** Wraps trusted skill snippets in a real isolated native-store exercise, not a sandbox. */
export function diagnosticSource(snippets, queries) {
  return `//! Exercises the documentation against the real typed diagnostics boundary.
mod diagnostics { pub use gitview_lib::diagnostics::*; }
mod save_example {
${snippets[0]}
    pub(super) fn exercise(started: std::time::Instant) { record_save_failure(started); }
}
mod scan_example {
${snippets[1]}
    pub(super) async fn exercise(parent: &crate::diagnostics::OperationContext) -> uuid::Uuid {
        spawn_scan(parent, async {
            let child = crate::diagnostics::OperationContext::current().expect("child scope missing");
            assert!(child.record(
                crate::diagnostics::Level::Info, crate::diagnostics::Component::Application,
                crate::diagnostics::Event::Completed, None, Default::default()));
            child.id()
        }).await.expect("child task failed")
    }
}
use diagnostics::{DiagnosticStore, OperationContext, OperationKind, ReadOnlyDiagnostics, Query};
use std::time::{Duration, Instant};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let database = std::path::PathBuf::from(std::env::args_os().nth(1).expect("database required"));
    let store = DiagnosticStore::open(&database);
    let parent = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::PersistWorkspace);
    assert!(OperationContext::current().is_none());
    // No ambient scope must not invent an operation or emit a record.
    save_example::exercise(Instant::now());
    let started = Instant::now();
    let child_id = parent.scope(async {
        tokio::time::sleep(Duration::from_millis(30)).await;
        // Writing to the owned directory provokes a real I/O failure with private bytes.
        let outcome = std::fs::write(database.parent().unwrap(), b"private-doc-example-payload");
        assert!(outcome.is_err());
        save_example::exercise(started);
        let child_id = scan_example::exercise(&parent).await;
        assert_eq!(OperationContext::current().unwrap().id(), parent.id());
        child_id
    }).await;
    let maximum_duration = started.elapsed().as_millis() as u64;
    assert!(OperationContext::current().is_none());
    assert_ne!(parent.id(), child_id);
    store.flush(Duration::from_secs(5)).expect("flush failed");
    let health = store.health();
    assert_eq!(health.accepted, 2);
    assert_eq!(health.written, 2);
    assert_eq!(health.dropped, 0);
    assert!(health.last_error_code.is_none());
    store.shutdown(Duration::from_secs(5)).expect("shutdown failed");
    let reader = ReadOnlyDiagnostics::open(&database).expect("reader validation failed");
    let rows = reader.events(&Query::default()).expect("event decoding failed");
    assert!(!rows.has_more);
    assert_eq!(rows.events.len(), 2);
    let failure = rows.events.iter().find(|row| row.operation_id == parent.id()).expect("failure missing");
    assert_eq!(failure.parent_operation_id, None);
    assert_eq!(failure.operation_kind, OperationKind::PersistWorkspace);
    assert_eq!(failure.component, diagnostics::Component::Persistence);
    assert_eq!(failure.level, diagnostics::Level::Error);
    assert_eq!(failure.event, diagnostics::Event::Failed);
    assert_eq!(failure.code, Some(diagnostics::Code::SaveFailed));
    assert!((30..=maximum_duration).contains(&failure.duration_ms.expect("duration missing")));
    let child = rows.events.iter().find(|row| row.operation_id == child_id).expect("child missing");
    assert_eq!(child.parent_operation_id, Some(parent.id()));
    assert_eq!(child.operation_kind, OperationKind::ScanContext);
    assert_eq!(child.component, diagnostics::Component::Application);
    assert_eq!(child.level, diagnostics::Level::Info);
    assert_eq!(child.event, diagnostics::Event::Completed);
    assert_eq!(child.code, None);
    for row in &rows.events {
        assert_eq!(row.exit_code, None);
        assert_eq!(row.stdout_bytes, None);
        assert_eq!(row.stderr_bytes, None);
        assert!(!row.cleanup_failed);
    }
    let connection = rusqlite::Connection::open_with_flags(&database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut recent = connection.prepare(${JSON.stringify(queries.recent)}).expect("recent SQL drift");
    let recent_rows: Vec<String> = recent.query_map([], |row| row.get(3)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(recent_rows, vec![parent.id().to_string()]);
    let mut timed = connection.prepare(${JSON.stringify(queries.timed)}).expect("timed SQL drift");
    let timed_rows: Vec<String> = timed.query_map(rusqlite::named_params! {":since_ms": 0}, |row| row.get(2)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(timed_rows, vec![parent.id().to_string()]);
    let mut causal = connection.prepare(${JSON.stringify(queries.causal)}).expect("causal SQL drift");
    let causal_rows: Vec<String> = causal.query_map(rusqlite::named_params! {":operation_id": parent.id().to_string()}, |row| row.get(1)).unwrap().collect::<Result<_, _>>().unwrap();
    assert_eq!(causal_rows, vec![parent.id().to_string()]);
    println!("{}", serde_json::json!({"operationId":parent.id(),"childOperationId":child_id}));
}
`;
}
