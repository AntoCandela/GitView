//! Exercises real service/Git/SQLite trace ownership, classified failures and background lineage.

#[path = "../support/mod.rs"]
mod support;

use gitview_lib::{application::RepositoryService, diagnostics::{Code, Component, DiagnosticStore, Event, HealthState, OperationContext, OperationKind, Query, ReadOnlyDiagnostics}, workspace::OpenOutcome};
use std::{collections::HashSet, path::PathBuf, time::Duration};
use tempfile::TempDir;

fn diagnostic_store() -> (TempDir, PathBuf, DiagnosticStore) {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    assert!(matches!(store.health().state, HealthState::Healthy));
    (directory, database, store)
}

#[tokio::test]
async fn concurrent_real_opens_keep_failure_classification_and_process_facts_separate() {
    let (_database_directory, database, store) = diagnostic_store();
    let (_repository_directory, root) = support::unborn_working_tree();
    let missing_repository = tempfile::tempdir().unwrap();
    let service = RepositoryService::with_diagnostics(store.sink());
    let success = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::OpenRepository);
    let failure = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::OpenRepository);
    let (opened, rejected) = tokio::join!(
        success.scope(service.open_chosen(&root)),
        failure.scope(service.open_chosen(missing_repository.path())),
    );
    assert!(matches!(opened, OpenOutcome::Opened { .. }));
    assert!(matches!(rejected, OpenOutcome::Rejected { code: "not_repository", .. }));
    store.flush(Duration::from_secs(2)).unwrap();
    let reader = ReadOnlyDiagnostics::open(&database).unwrap();
    let good = reader.events(&Query { operation_id: Some(success.id()), limit: 200, ..Default::default() }).unwrap().events;
    let bad = reader.events(&Query { operation_id: Some(failure.id()), limit: 200, ..Default::default() }).unwrap().events;
    for component in [Component::Application, Component::Git, Component::Process] {
        assert!(good.iter().any(|row| row.component == component && row.event == Event::Started));
        assert!(bad.iter().any(|row| row.component == component && row.event == Event::Started));
    }
    assert!(good.iter().any(|row| row.component == Component::Application && row.event == Event::Completed));
    assert!(!good.iter().any(|row| row.event == Event::Failed));
    assert!(good.iter().any(|row| row.component == Component::Process && row.exit_code.is_some_and(|code| code != 0) && row.event == Event::Completed));
    assert!(bad.iter().any(|row| row.component == Component::Application && row.event == Event::Failed && row.code == Some(Code::NotRepository)));
    assert!(bad.iter().any(|row| row.component == Component::Git && row.code == Some(Code::NotRepository)));
    assert!(good.iter().all(|row| row.operation_id == success.id() && row.operation_kind == OperationKind::OpenRepository));
    assert!(bad.iter().all(|row| row.operation_id == failure.id() && row.operation_kind == OperationKind::OpenRepository));
    service.shutdown().await;
    store.shutdown(Duration::from_secs(2)).unwrap();
}

#[tokio::test]
async fn each_completed_scan_has_its_own_identity_under_selection_not_cached_polling() {
    let (_database_directory, database, store) = diagnostic_store();
    let (_repository_directory, root) = support::working_tree();
    let service = RepositoryService::with_diagnostics(store.sink());
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("real fixture must open") };
    let selection = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::SelectContext);
    selection.scope(service.select(&entry_id)).await;
    let polling = OperationContext::new(store.sink(), None, None).with_kind(OperationKind::ObserveContext);
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let _ = polling.scope(service.observe_selected_context(&entry_id)).await;
            store.flush(Duration::from_secs(2)).unwrap();
            let rows = ReadOnlyDiagnostics::open(&database).unwrap()
                .events(&Query { component: Some(Component::Observation), limit: 200, ..Default::default() }).unwrap().events;
            if rows.iter().filter(|row| row.event == Event::Completed).count() >= 2 { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.expect("two real scans must complete");
    service.shutdown().await;
    store.flush(Duration::from_secs(2)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { component: Some(Component::Observation), limit: 200, ..Default::default() }).unwrap().events;
    let completed: Vec<_> = rows.iter().filter(|row| row.event == Event::Completed).collect();
    let identities: HashSet<_> = completed.iter().map(|row| row.operation_id).collect();
    assert!(identities.len() >= 2, "scan identity must not be reused across ticks");
    for row in completed {
        assert_eq!(row.operation_kind, OperationKind::ScanContext);
        assert_eq!(row.parent_operation_id, Some(selection.id()));
        assert_ne!(row.operation_id, polling.id());
        assert!(rows.iter().any(|start| start.operation_id == row.operation_id && start.event == Event::Started));
    }
    store.shutdown(Duration::from_secs(2)).unwrap();
}

#[tokio::test]
async fn corrupt_workspace_preserves_bytes_and_records_safe_persistence_failure() {
    let (_database_directory, database, store) = diagnostic_store();
    let workspace_directory = tempfile::tempdir().unwrap();
    let workspace_file = workspace_directory.path().join("workspace.json");
    let private_payload = b"not-json:private-path-and-secret-token";
    std::fs::write(&workspace_file, private_payload).unwrap();
    let service = RepositoryService::with_workspace_file_and_diagnostics(workspace_file.clone(), store.sink()).await;
    assert_eq!(service.snapshot().await.persistence_error.unwrap().code, gitview_lib::workspace::persistence::PersistenceErrorCode::LoadFailed);
    assert_eq!(std::fs::read(&workspace_file).unwrap(), private_payload);
    service.shutdown().await;
    store.flush(Duration::from_secs(2)).unwrap();
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { component: Some(Component::Persistence), limit: 200, ..Default::default() }).unwrap().events;
    assert!(rows.iter().any(|row| row.event == Event::Failed && row.code == Some(Code::LoadFailed)));
    assert!(!serde_json::to_string(&rows).unwrap().contains("private-path-and-secret-token"));
    store.shutdown(Duration::from_secs(2)).unwrap();
}

#[tokio::test]
async fn unavailable_capture_keeps_real_repository_operations_usable_and_health_degraded() {
    let store = DiagnosticStore::unavailable(Code::StorageUnavailable);
    let service = RepositoryService::with_diagnostics(store.sink());
    let (_repository_directory, root) = support::working_tree();
    assert!(matches!(service.open_chosen(&root).await, OpenOutcome::Opened { .. }));
    assert!(matches!(store.health().state, HealthState::Degraded));
    assert_eq!(store.health().last_error_code, Some(Code::StorageUnavailable));
    service.shutdown().await;
    assert_eq!(store.shutdown(Duration::from_secs(2)), Err(Code::Storage));
}
