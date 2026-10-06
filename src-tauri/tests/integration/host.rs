//! Exercises mocked IPC authorization and real Git/SQLite behind the actual host handlers.
//! Mock webviews do not prove native picker behavior or a packaged desktop launch.

use super::*;
use crate::test_support;
use crate::observation::ObservationSnapshot;
use crate::workspace::OpenOutcome;
use tokio::sync::oneshot;
use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
use crate::diagnostics::{Query, ReadOnlyDiagnostics};
use std::future::Future;
use std::time::Duration;
use tauri::test::MockRuntime;
use uuid::Uuid;

const WAIT: Duration = Duration::from_secs(5);

fn flush_diagnostics(store: &DiagnosticStore) {
    let before = store.health();
    let result = store.flush(WAIT);
    assert_diagnostic_control(store, before.accepted, before.written, result);
}

fn shutdown_diagnostics(store: &DiagnosticStore) {
    let before = store.health();
    let result = store.shutdown(WAIT);
    assert_diagnostic_control(store, before.accepted, before.written, result);
}

fn assert_diagnostic_control(store: &DiagnosticStore, accepted: u64, written: u64, result: Result<(), Code>) {
    match result {
        Ok(()) => assert!(store.health().written >= accepted, "diagnostic control must commit its accepted watermark"),
        Err(Code::Timeout) => {
            let health = store.health();
            if health.written < accepted {
                if health.written > written {
                    panic!("diagnostic control timed out while accepted records were still committing");
                }
                panic!("diagnostic control timed out with no progress toward its accepted watermark");
            }
            if health.written < health.accepted {
                panic!("diagnostic control timed out after its initial watermark committed; later accepted records remain");
            }
            panic!("diagnostic control timed out after its accepted watermark committed; acknowledgement or writer exit pending");
        },
        Err(Code::Storage) => panic!("diagnostic control storage failure"),
        Err(_) => panic!("diagnostic control failed with another fixed code"),
    }
}

fn request(window: &WebviewWindow<MockRuntime>, command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
    tauri::webview::InvokeRequest {
        cmd: command.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: window.url().unwrap(),
        body: tauri::ipc::InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    }
}

fn response(window: &WebviewWindow<MockRuntime>, command: &str, body: serde_json::Value) -> serde_json::Value {
    get_ipc_response(window, request(window, command, body)).unwrap().deserialize().unwrap()
}

fn renderer_terminal(operation: Uuid, phase: &str) -> serde_json::Value {
    serde_json::json!({ "diagnostic": {
        "operationId": operation.to_string(), "command": "refresh_entry_availability",
        "phase": phase, "durationMs": 7,
    } })
}

#[test]
fn preferred_languages_is_main_only_and_returns_only_language_tags() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::preferred_languages])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    let result = response(&main, "preferred_languages", serde_json::json!({}));
    assert_eq!(result.as_object().unwrap().len(), 1);
    for language in result["languages"].as_array().unwrap() {
        let language = language.as_str().unwrap();
        assert_eq!(languages::normalize_language(language).as_deref(), Some(language));
    }
    assert!(get_ipc_response(&secondary, request(&secondary, "preferred_languages", serde_json::json!({}))).is_err());
}

#[test]
fn picker_rejects_unknown_locale_before_opening_a_dialog() {
    let app = mock_builder()
        .manage(RepositoryService::new())
        .invoke_handler(tauri::generate_handler![commands::open_chosen_repository])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    for body in [serde_json::json!({}), serde_json::json!({"locale": "fr-FR"}), serde_json::json!({"locale": "Arbitrary title"})] {
        assert!(get_ipc_response(&main, request(&main, "open_chosen_repository", body)).is_err());
    }
}

#[test]
fn observation_command_is_allowed_only_from_main_webview() {
    let app = mock_builder()
        .manage(RepositoryService::new())
        .invoke_handler(tauri::generate_handler![commands::observe_selected_context])
        .build(app_context())
        .unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    let request = || tauri::webview::InvokeRequest {
        cmd: "observe_selected_context".into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: main.url().unwrap(),
        body: tauri::ipc::InvokeBody::Json(serde_json::json!({ "entryId": "unknown" })),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    };
    let response = get_ipc_response(&main, request()).unwrap().deserialize::<serde_json::Value>().unwrap();
    assert_eq!(response, serde_json::json!({
        "kind": "unavailable", "entryId": "unknown", "observationRevision": 0,
        "errorCode": "inaccessible"
    }));
    assert!(get_ipc_response(&secondary, request()).is_err());
}

#[test]
fn repository_mutations_are_restricted_to_the_main_webview() {
    let app = mock_builder()
        .manage(RepositoryService::new())
        .invoke_handler(tauri::generate_handler![commands::rename_repository, commands::remove_repository])
        .build(app_context())
        .unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    for command in ["rename_repository", "remove_repository"] {
        let request = || tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: main.url().unwrap(),
            body: tauri::ipc::InvokeBody::Json(serde_json::json!({
                "entryId": "unknown", "displayName": "Local label",
            })),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.into(),
        };
        let response = get_ipc_response(&main, request()).unwrap().deserialize::<serde_json::Value>().unwrap();
        assert_eq!(response["kind"], "not_found");
        assert!(get_ipc_response(&secondary, request()).is_err());
    }
}

#[test]
fn every_host_command_denies_secondary_webviews_including_diagnostics() {
    let app = mock_builder()
        .manage(RepositoryService::new())
        .manage(DiagnosticStore::unavailable(Code::StorageUnavailable))
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot, commands::open_chosen_repository, commands::select_context, commands::rename_repository,
            commands::remove_repository, commands::refresh_entry_availability, commands::observe_selected_context,
            commands::record_renderer_diagnostic, commands::diagnostic_health,
        ])
        .build(app_context()).unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    for command in [
        "workspace_snapshot", "open_chosen_repository", "select_context", "rename_repository",
        "remove_repository", "refresh_entry_availability", "observe_selected_context",
        "record_renderer_diagnostic", "diagnostic_health",
    ] {
        let mut body = renderer_terminal(Uuid::new_v4(), "completed");
        body["entryId"] = serde_json::json!("unknown");
        body["displayName"] = serde_json::json!("Local label");
        body["locale"] = serde_json::json!("en-US");
        assert!(get_ipc_response(&secondary, request(&secondary, command, body)).is_err(), "{command}");
    }
}

#[test]
fn malformed_operation_metadata_cannot_mutate_workspace_or_enter_capture() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&path);
    let service = RepositoryService::with_diagnostics(store.sink());
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot, commands::open_chosen_repository, commands::select_context, commands::rename_repository,
            commands::remove_repository, commands::refresh_entry_availability, commands::observe_selected_context,
        ])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    for metadata in [
        serde_json::json!("private/path/not-an-id"),
        serde_json::json!("A199F55D-99E4-4B80-8CA5-5161A0D04D55"),
        serde_json::json!("c199f55d99e44b808ca55161a0d04d55"),
        serde_json::json!("c199f55d-99e4-1b80-8ca5-5161a0d04d55"),
        serde_json::json!("c199f55d-99e4-4b80-0ca5-5161a0d04d55"),
        serde_json::json!(42),
    ] {
        let body = serde_json::json!({
            "operationId": metadata, "entryId": "unknown", "displayName": "Private label", "locale": "en-US",
        });
        for command in [
            "workspace_snapshot", "open_chosen_repository", "select_context", "rename_repository",
            "remove_repository", "refresh_entry_availability", "observe_selected_context",
        ] {
            assert!(get_ipc_response(&main, request(&main, command, body.clone())).is_err(), "{command}");
        }
    }
    let store = app.state::<DiagnosticStore>();
    assert_eq!(store.health().accepted, 0);
    assert!(tauri::async_runtime::block_on(app.state::<RepositoryService>().snapshot()).entries.is_empty());
    shutdown_diagnostics(&store);
}

#[test]
fn renderer_ingestion_rejects_unknown_payloads_and_bounded_metadata_violations() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let app = mock_builder().manage(DiagnosticStore::open(&path))
        .invoke_handler(tauri::generate_handler![commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    for (field, value) in [
        ("operationId", serde_json::json!("not-an-id")),
        ("command", serde_json::json!("arbitrary_command")),
        ("phase", serde_json::json!("started")),
        ("durationMs", serde_json::json!(-1)),
        ("durationMs", serde_json::json!(86400001)),
        ("durationMs", serde_json::json!(1.5)),
        ("payload", serde_json::json!("private path and output")),
    ] {
        let mut body = renderer_terminal(Uuid::new_v4(), "completed");
        body["diagnostic"][field] = value;
        assert!(get_ipc_response(&main, request(&main, "record_renderer_diagnostic", body)).is_err());
    }
    assert_eq!(app.state::<DiagnosticStore>().health().accepted, 0);
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
}

#[test]
fn renderer_transport_failure_is_error_evidence_not_success() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let app = mock_builder().manage(DiagnosticStore::open(&path))
        .invoke_handler(tauri::generate_handler![commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let operation = Uuid::new_v4();
    response(&main, "record_renderer_diagnostic", renderer_terminal(operation, "transport_failed"));
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query { operation_id: Some(operation), ..Default::default() }).unwrap();
    assert_eq!(rows.events.len(), 1);
    assert_eq!(rows.events[0].component, Component::Renderer);
    assert_eq!(rows.events[0].event, Event::Failed);
    assert_eq!(rows.events[0].level, Level::Error);
    assert_eq!(rows.events[0].code, Some(Code::Transport));
    assert_eq!(rows.events[0].duration_ms, Some(7));
}

#[test]
fn actual_mocked_ipc_refresh_correlates_real_git_and_sqlite_across_layers() {
    let (temp, root) = test_support::working_tree();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&path);
    let service = RepositoryService::with_diagnostics(store.sink());
    let OpenOutcome::Opened { entry_id: entry, .. } = tauri::async_runtime::block_on(service.open_chosen(&root)) else {
        panic!("real fixture must be admitted");
    };
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::refresh_entry_availability, commands::record_renderer_diagnostic, commands::diagnostic_health])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let operation = Uuid::new_v4();
    let refreshed = response(&main, "refresh_entry_availability", serde_json::json!({
        "entryId": entry, "operationId": operation.to_string(),
    }));
    assert_eq!(refreshed["entries"][0]["availability"], "available");
    response(&main, "record_renderer_diagnostic", renderer_terminal(operation, "completed"));
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    let health = response(&main, "diagnostic_health", serde_json::json!({}));
    assert_eq!(health["state"], "healthy");
    assert!(health["written"].as_u64().unwrap() > 0);
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query {
        operation_id: Some(operation), limit: 200, ..Default::default()
    }).unwrap();
    assert!(!rows.has_more);
    let session = rows.events[0].session_id;
    assert!(rows.events.iter().all(|row| row.operation_id == operation && row.session_id == session));
    assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::RefreshAvailability));
    for component in [Component::Renderer, Component::Ipc, Component::Application, Component::Git, Component::Process] {
        assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Started), "{component:?}");
        assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Completed), "{component:?}");
    }
    assert!(rows.events.iter().any(|row| row.component == Component::Process
        && row.duration_ms.is_some() && row.exit_code.is_some()
        && row.stdout_bytes.is_some() && row.stderr_bytes.is_some()));
}

#[test]
fn native_fallback_id_is_generated_when_existing_callers_omit_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&path);
    let app = mock_builder().manage(RepositoryService::with_diagnostics(store.sink())).manage(store)
        .invoke_handler(tauri::generate_handler![commands::workspace_snapshot])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let snapshot = response(&main, "workspace_snapshot", serde_json::json!({}));
    assert_eq!(snapshot["entries"], serde_json::json!([]));
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query { limit: 200, ..Default::default() }).unwrap();
    let started = rows.events.iter().find(|row| row.component == Component::Ipc && row.event == Event::Started).unwrap();
    assert_eq!(started.operation_id.get_version(), Some(uuid::Version::Random));
    assert!(rows.events.iter().all(|row| row.operation_id == started.operation_id && row.component != Component::Renderer));
}

#[test]
fn unavailable_capture_preserves_real_repository_results_and_reports_host_health() {
    let (_temp, root) = test_support::working_tree();
    let store = DiagnosticStore::unavailable(Code::StorageUnavailable);
    let service = RepositoryService::with_diagnostics(store.sink());
    let OpenOutcome::Opened { entry_id: entry, .. } = tauri::async_runtime::block_on(service.open_chosen(&root)) else {
        panic!("capture failure must not prevent admission");
    };
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::refresh_entry_availability, commands::diagnostic_health])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let refreshed = response(&main, "refresh_entry_availability", serde_json::json!({
        "entryId": entry, "operationId": Uuid::new_v4().to_string(),
    }));
    assert_eq!(refreshed["entries"][0]["availability"], "available");
    let health = response(&main, "diagnostic_health", serde_json::json!({}));
    assert_eq!(health["state"], "degraded");
    assert_eq!(health["last_error_code"], "storage_unavailable");
    assert_eq!(health["written"], 0);
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
}

#[test]
fn health_reads_the_host_store_without_repository_service_or_database_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let app = mock_builder().manage(DiagnosticStore::open(&path))
        .invoke_handler(tauri::generate_handler![commands::diagnostic_health])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let healthy = response(&main, "diagnostic_health", serde_json::json!({}));
    assert_eq!(healthy["state"], "healthy");
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    std::fs::remove_file(&path).unwrap();
    let stopped = response(&main, "diagnostic_health", serde_json::json!({}));
    assert_eq!(stopped["state"], "stopped");
    assert!(!path.exists());
}

#[test]
fn failed_real_git_refresh_keeps_domain_result_and_correlated_failure_evidence() {
    let (temp, root) = test_support::working_tree();
    let path = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&path);
    let service = RepositoryService::with_diagnostics(store.sink());
    let OpenOutcome::Opened { entry_id: entry, .. } = tauri::async_runtime::block_on(service.open_chosen(&root)) else {
        panic!("real fixture must be admitted");
    };
    std::fs::remove_dir_all(root.join(".git")).unwrap();
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::refresh_entry_availability, commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let operation = Uuid::new_v4();
    let refreshed = response(&main, "refresh_entry_availability", serde_json::json!({
        "entryId": entry, "operationId": operation.to_string(),
    }));
    assert_eq!(refreshed["entries"][0]["availability"], "unavailable");
    // Resolved transport is not domain success: Git classifies the real probe failure.
    response(&main, "record_renderer_diagnostic", renderer_terminal(operation, "completed"));
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let rows = ReadOnlyDiagnostics::open(&path).unwrap().events(&Query {
        operation_id: Some(operation), limit: 200, ..Default::default()
    }).unwrap();
    assert!(!rows.has_more);
    let session = rows.events[0].session_id;
    assert!(rows.events.iter().all(|row| row.session_id == session && row.operation_id == operation));
    for component in [Component::Renderer, Component::Ipc, Component::Application, Component::Git, Component::Process] {
        assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Started), "{component:?}");
    }
    assert!(rows.events.iter().any(|row| row.component == Component::Git
        && row.event == Event::Failed && row.code == Some(Code::NotRepository)));
    assert!(rows.events.iter().any(|row| row.component == Component::Process
        && row.event == Event::Completed && row.exit_code.is_some_and(|code| code != 0)));
}

#[test]
fn review_ipc_is_restricted_and_correlates_success_and_failure_without_private_payload() {
    let (temp, root) = test_support::working_tree();
    std::fs::write(root.join("private-review-path"), b"private review source\n").unwrap();
    std::fs::write(root.join("private-binary-path"), b"private\0binary").unwrap();
    std::fs::write(root.join("private-staged-path"), b"private review source\n").unwrap();
    test_support::git(&root, &["add", "--", "private-staged-path"]);
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = RepositoryService::with_diagnostics(store.sink());
    let (entry, revision, files) = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture must be admitted") };
        service.select(&entry_id).await;
        let snapshot = tokio::time::timeout(WAIT, async {
            loop {
                if let ObservationSnapshot::Ready { observation_revision, files, .. } = service.observe_selected_context(&entry_id).await {
                    break (observation_revision, files);
                }
                // Readiness polling must not outrun the durable diagnostic writer.
                let sink = store.sink();
                let accepted = sink.health().accepted;
                loop {
                    let health = sink.health();
                    // Best-effort admission can drop unrelated records on try_lock contention.
                    assert!(matches!(health.last_error_code, None | Some(Code::Overflow)));
                    if health.written >= accepted { break; }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.unwrap();
        (entry_id, snapshot.0, snapshot.1)
    });
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::review_file, commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    let working_text = if cfg!(unix) {
        ("text", Event::Completed, None)
    } else {
        ("unsupported", Event::Failed, Some(Code::ReviewUnsupported))
    };
    let operations = [
        ("private-review-path", "untracked", working_text.0, working_text.1, working_text.2),
        ("private-binary-path", "untracked", "unsupported", Event::Failed, Some(Code::ReviewUnsupported)),
        ("private-staged-path", "staged", "text", Event::Completed, None),
    ].map(|(name, category, expected_kind, expected_event, expected_code)| {
        let id = Uuid::new_v4();
        let body = serde_json::json!({
            "entryId": entry, "observationRevision": revision,
            "pathId": files.iter().find(|file| file.display_path == name).unwrap().path_id,
            "category": category, "operationId": id.to_string(),
        });
        assert!(get_ipc_response(&secondary, request(&secondary, "review_file", body.clone())).is_err());
        let result = response(&main, "review_file", body);
        assert_eq!(result["kind"], expected_kind);
        if expected_kind == "text" {
            assert_eq!(result["from"], if category == "staged" { "HEAD" } else { "absent" });
            assert_eq!(result["to"], if category == "staged" { "index" } else { "working_files" });
            assert_eq!(result["fromAbsent"], true);
            assert_eq!(result["hunks"][0]["lines"][0]["text"], "private review source");
        }
        if expected_kind == "unsupported" && !cfg!(unix) {
            assert_eq!(result["reason"], "other");
        }
        response(&main, "record_renderer_diagnostic", serde_json::json!({ "diagnostic": {
            "operationId": id.to_string(), "command": "review_file", "phase": "completed", "durationMs": 1,
        }}));
        (id, expected_event, expected_code)
    });
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    for (id, expected_event, expected_code) in operations {
        let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(id), limit: 200, ..Default::default() }).unwrap();
        assert!(!rows.has_more);
        for component in [Component::Ipc, Component::Application, Component::Git] {
            assert!(rows.events.iter().any(|row| row.component == component && row.event == expected_event && row.code == expected_code));
        }
        assert!(rows.events.iter().any(|row| row.component == Component::Process));
        assert!(rows.events.iter().any(|row| row.component == Component::Renderer && row.event == Event::Completed));
        assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::ReviewFile));
    }
    let bytes = std::fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-review-path".len()).any(|bytes| bytes == b"private-review-path"));
    assert!(!bytes.windows(b"private-staged-path".len()).any(|bytes| bytes == b"private-staged-path"));
    assert!(!bytes.windows(b"private review source".len()).any(|bytes| bytes == b"private review source"));
}

#[test]
fn history_ipc_restricts_windows_and_correlates_real_topology_without_private_payloads() {
    let (temp, root) = test_support::working_tree();
    test_support::git(&root, &["branch", "private-history-ref"]);
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = RepositoryService::with_diagnostics(store.sink());
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture must be admitted") };
        service.select(&entry_id).await;
        entry_id
    });
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::history_page, commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    let operation = Uuid::new_v4();
    let body = serde_json::json!({ "entryId": entry, "cursor": null, "operationId": operation.to_string() });
    assert!(get_ipc_response(&secondary, request(&secondary, "history_page", body.clone())).is_err());
    let result = response(&main, "history_page", body);
    assert_eq!(result["kind"], "page");
    assert_eq!(result["page"]["head"]["state"], "attached");
    assert_eq!(result["page"]["head"]["scope"], "worktree");
    assert_eq!(result["page"]["commits"][0]["root"], true);
    assert_eq!(result["page"]["commits"][0]["parents"], serde_json::json!([]));
    assert!(result["page"]["refs"].as_array().unwrap().iter().any(|reference| reference["name"] == "private-history-ref"));
    response(&main, "record_renderer_diagnostic", serde_json::json!({ "diagnostic": {
        "operationId": operation.to_string(), "command": "history_page", "phase": "completed", "durationMs": 1,
    }}));
    let rejected = Uuid::new_v4();
    let result = response(&main, "history_page", serde_json::json!({
        "entryId": entry, "cursor": "--all", "operationId": rejected.to_string(),
    }));
    assert_eq!(result["kind"], "unavailable");
    assert_eq!(result["code"], "stale_cursor");
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(operation), limit: 200, ..Default::default() }).unwrap();
    assert!(!rows.has_more);
    for component in [Component::Ipc, Component::Application, Component::Git, Component::Process, Component::Renderer] {
        assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Completed));
    }
    assert!(rows.events.iter().all(|row| row.operation_kind == OperationKind::HistoryPage));
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(rejected), limit: 200, ..Default::default() }).unwrap();
    assert!(!rows.has_more);
    assert!(rows.events.iter().any(|row| row.component == Component::Application && row.event == Event::Superseded && row.code == Some(Code::StaleCursor)));
    let bytes = std::fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-history-ref".len()).any(|bytes| bytes == b"private-history-ref"));
}

#[test]
fn inspection_ipc_selects_discovered_worktrees_and_records_only_fixed_diagnostic_kinds() {
    let (temp, root) = test_support::working_tree();
    let linked = temp.path().join("private-linked");
    test_support::git(&root, &["worktree", "add", "-b", "private-topic", linked.to_str().unwrap()]);
    test_support::git(&root, &["branch", "upstream"]);
    test_support::git(&root, &["config", "branch.main.remote", "."]);
    test_support::git(&root, &["config", "branch.main.merge", "refs/heads/upstream"]);
    std::fs::write(root.join("private-commit-file"), "private content").unwrap();
    test_support::commit(&root);
    let oid = String::from_utf8(test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout).unwrap().trim_end().to_owned();
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = RepositoryService::with_diagnostics(store.sink());
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture opens") };
        service.select(&entry_id).await;
        entry_id
    });
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::list_contexts, commands::select_worktree, commands::commit_files, commands::upstream_files, commands::history_page, commands::review_commit_file, commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    for command in ["list_contexts", "select_worktree", "commit_files", "upstream_files", "review_commit_file"] {
        let body = serde_json::json!({ "entryId": entry, "worktreeId": "unknown", "commitOid": oid, "parentOid": null, "fileId": "forged", "token": "forged" });
        assert!(get_ipc_response(&secondary, request(&secondary, command, body)).is_err());
    }
    let list_operation = Uuid::new_v4();
    let options = response(&main, "list_contexts", serde_json::json!({ "entryId": entry, "operationId": list_operation.to_string() }));
    assert_eq!(options["kind"], "options");
    let target = options["worktrees"].as_array().unwrap().iter().find(|worktree| worktree["current"] == false).unwrap()["id"].as_str().unwrap();
    let files_operation = Uuid::new_v4();
    let files = response(&main, "commit_files", serde_json::json!({ "entryId": entry, "commitOid": oid, "parentOid": null, "operationId": files_operation.to_string() }));
    assert_eq!(files["kind"], "files");
    assert_eq!(files["files"][0]["displayPath"], "private-commit-file");
    assert_eq!(files["files"][0]["kind"], "added");
    std::fs::write(root.join("private-commit-file"), "different working bytes").unwrap();
    let graph = response(&main, "history_page", serde_json::json!({ "entryId": entry, "cursor": null, "branch": null }));
    let token = graph["page"]["upstream"]["outgoing"]["token"].as_str().unwrap();
    let upstream_operation = Uuid::new_v4();
    let aggregate = response(&main, "upstream_files", serde_json::json!({ "entryId": entry, "token": token, "operationId": upstream_operation.to_string() }));
    assert_eq!(aggregate["kind"], "files");
    assert_eq!(aggregate["files"][0]["displayPath"], "private-commit-file");
    let review_operation = Uuid::new_v4();
    let review_body = serde_json::json!({
        "entryId": entry, "commitOid": oid, "parentOid": files["parentOid"],
        "fileId": files["files"][0]["id"], "operationId": review_operation.to_string(),
    });
    let reviewed = response(&main, "review_commit_file", review_body.clone());
    assert_eq!(reviewed["kind"], "text");
    assert_eq!(reviewed["fromAbsent"], true);
    assert_eq!(reviewed["toAbsent"], false);
    assert_eq!(reviewed["parentOid"], files["parentOid"]);
    assert_eq!(reviewed["hunks"][0]["lines"][0]["text"], "private content");
    assert!(get_ipc_response(&secondary, request(&secondary, "review_commit_file", review_body.clone())).is_err());
    let select_operation = Uuid::new_v4();
    let selected = response(&main, "select_worktree", serde_json::json!({ "entryId": entry, "worktreeId": target, "operationId": select_operation.to_string() }));
    assert_eq!(selected["kind"], "updated");
    assert_ne!(selected["snapshot"]["activeContextId"], entry);
    assert_eq!(selected["snapshot"]["entries"][1]["head"]["name"], "private-topic");
    let stale_operation = Uuid::new_v4();
    let mut stale_body = review_body;
    stale_body["operationId"] = serde_json::json!(stale_operation.to_string());
    assert_eq!(response(&main, "review_commit_file", stale_body), serde_json::json!({ "kind": "stale_selection" }));
    let operations = [
        (list_operation, "list_contexts", OperationKind::ListContexts),
        (files_operation, "commit_files", OperationKind::CommitFiles),
        (upstream_operation, "upstream_files", OperationKind::UpstreamFiles),
        (review_operation, "review_commit_file", OperationKind::ReviewCommitFile),
        (select_operation, "select_worktree", OperationKind::SelectWorktree),
    ];
    for (operation, command, _) in operations {
        response(&main, "record_renderer_diagnostic", serde_json::json!({ "diagnostic": {
            "operationId": operation.to_string(), "command": command, "phase": "completed", "durationMs": 1,
        }}));
    }
    // All commands and renderer facts are complete; seal capture before read-only validation.
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let stale_rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(stale_operation), limit: 200, ..Default::default() }).unwrap();
    assert!(!stale_rows.has_more);
    for component in [Component::Ipc, Component::Application] {
        assert!(stale_rows.events.iter().any(|row| row.component == component && row.event == Event::Superseded));
    }
    for (operation, _, kind) in operations {
        let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(operation), limit: 200, ..Default::default() }).unwrap();
        assert!(!rows.has_more);
        assert!(rows.events.iter().all(|row| row.operation_kind == kind || row.operation_kind == OperationKind::PersistWorkspace));
        for component in [Component::Renderer, Component::Ipc, Component::Application, Component::Process] {
            assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Completed));
        }
    }
    let bytes = std::fs::read(database).unwrap();
    for private in [b"private-commit-file".as_slice(), b"private content", b"private-topic", b"private-linked"] {
        assert!(!bytes.windows(private.len()).any(|window| window == private));
    }
}

/// Only the OS folder callback is replaced; command dispatch, admission, Git and capture are real.
struct AdmissionFixture(std::path::PathBuf);

#[tauri::command(rename = "open_chosen_repository")]
async fn admit_native_fixture<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    folder: tauri::State<'_, AdmissionFixture>,
    operation_id: String,
) -> Result<OpenOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::OpenRepository, Some(&operation_id))?;
    traced_ipc(&service, context, async {
        let (send, receive) = oneshot::channel();
        send.send(folder.0.clone()).unwrap();
        let selected = receive.await.map_err(|_| "Native fixture callback failed.").unwrap();
        service.open_chosen(&selected).await
    }).await
}

#[test]
fn traced_native_admission_persists_choices_and_correlated_git_completion() {
    // A stack overflow aborts rather than unwinds; isolate the real IPC worker from the suite.
    const CHILD: &str = "GITVIEW_ADMISSION_REGRESSION_CHILD";
    const COMPLETION: &str = "GITVIEW_ADMISSION_REGRESSION_COMPLETION";
    if std::env::var_os(CHILD).is_none() {
        let completion_directory = tempfile::tempdir().unwrap();
        let completion = completion_directory.path().join("completed");
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "host::integration_tests::traced_native_admission_persists_choices_and_correlated_git_completion", "--nocapture"])
            .env(CHILD, "1")
            .env(COMPLETION, &completion)
            .output().unwrap();
        assert!(child.status.success(), "native admission worker must complete without aborting:\n{}\n{}",
            String::from_utf8_lossy(&child.stdout), String::from_utf8_lossy(&child.stderr));
        assert_eq!(std::fs::read(&completion).unwrap(), b"completed",
            "native admission worker must execute the exact regression through its final assertions");
        return;
    }

    let (temp, root) = test_support::working_tree();
    let root = root.canonicalize().unwrap();
    let directory = temp.path().canonicalize().unwrap();
    let database = directory.join("diagnostics.sqlite");
    let choices = directory.join("workspace.json");
    let before_head = test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout;
    let before_index = std::fs::read(root.join(".git/index")).unwrap();
    let before_refs = test_support::git_output(&root, &["show-ref"]).stdout;
    let store = DiagnosticStore::open(&database);
    let service = tauri::async_runtime::block_on(
        RepositoryService::with_workspace_file_and_diagnostics(choices.clone(), store.sink()),
    );
    let app = mock_builder().manage(service).manage(store).manage(AdmissionFixture(root.clone()))
        .invoke_handler(tauri::generate_handler![admit_native_fixture])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let operation = Uuid::new_v4();
    let admitted = response(&main, "open_chosen_repository", serde_json::json!({
        "operationId": operation.to_string(),
    }));
    assert_eq!(admitted["kind"], "opened");
    assert_eq!(admitted["snapshot"]["entries"][0]["availability"], "available");
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&choices).unwrap()).unwrap();
    assert_eq!(saved["repositories"][0]["root"], root.to_str().unwrap());
    assert_eq!(test_support::git_output(&root, &["rev-parse", "HEAD"]).stdout, before_head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), before_index);
    assert_eq!(test_support::git_output(&root, &["show-ref"]).stdout, before_refs);

    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    let records = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query {
        operation_id: Some(operation), limit: 200, ..Default::default()
    }).unwrap();
    assert!(!records.has_more);
    for component in [Component::Ipc, Component::Application, Component::Git, Component::Process] {
        assert!(records.events.iter().any(|row| row.component == component && row.event == Event::Completed));
        assert!(!records.events.iter().any(|row| row.component == component
            && matches!(row.event, Event::Failed | Event::Cancelled)));
    }
    std::fs::write(std::env::var_os(COMPLETION).expect("parent-owned completion marker"), b"completed").unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn browsing_ipc_reads_issued_working_files_only_from_main_and_records_safe_command_kinds() {
    let (temp, root) = test_support::working_tree();
    std::fs::write(root.join("private-browsing-file"), "private browsing bytes\n").unwrap();
    let database = temp.path().canonicalize().unwrap().join("capture/diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = RepositoryService::with_diagnostics(store.sink());
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture opens") };
        service.select(&entry_id).await;
        entry_id
    });
    let app = mock_builder().manage(service).manage(store)
        .invoke_handler(tauri::generate_handler![commands::list_repository_files, commands::review_repository_file, commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let secondary = tauri::WebviewWindowBuilder::new(&app, "secondary", Default::default()).build().unwrap();
    for command in ["list_repository_files", "review_repository_file"] {
        let body = serde_json::json!({ "entryId": entry, "listingId": "forged", "fileId": "../../outside",
            "request": { "listingId": null, "directoryId": null, "cursor": null } });
        assert!(get_ipc_response(&secondary, request(&secondary, command, body)).is_err());
    }
    let list_operation = Uuid::new_v4();
    let listed = response(&main, "list_repository_files", serde_json::json!({
        "entryId": entry, "operationId": list_operation.to_string(),
        "request": { "listingId": null, "directoryId": null, "cursor": null },
    }));
    assert_eq!(listed["kind"], "files");
    assert_eq!(listed["files"][0]["displayPath"], "private-browsing-file");
    let review_operation = Uuid::new_v4();
    let reviewed = response(&main, "review_repository_file", serde_json::json!({
        "entryId": entry, "listingId": listed["listingId"], "fileId": listed["files"][0]["id"],
        "operationId": review_operation.to_string(),
    }));
    assert_eq!(reviewed["kind"], "text");
    assert_eq!(reviewed["content"], "private browsing bytes\n");
    let forged_operation = Uuid::new_v4();
    assert_eq!(response(&main, "review_repository_file", serde_json::json!({
        "entryId": entry, "listingId": listed["listingId"], "fileId": "../../outside",
        "operationId": forged_operation.to_string(),
    })), serde_json::json!({ "kind": "stale_selection" }));
    let operations = [
        (list_operation, "list_repository_files", OperationKind::ListRepositoryFiles),
        (review_operation, "review_repository_file", OperationKind::ReviewRepositoryFile),
    ];
    for (operation, command, _) in operations {
        response(&main, "record_renderer_diagnostic", serde_json::json!({ "diagnostic": {
            "operationId": operation.to_string(), "command": command, "phase": "completed", "durationMs": 1,
        }}));
    }
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    flush_diagnostics(&app.state::<DiagnosticStore>());
    shutdown_diagnostics(&app.state::<DiagnosticStore>());
    for (operation, _, kind) in operations {
        let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(operation), limit: 200, ..Default::default() }).unwrap();
        assert!(!rows.has_more);
        assert!(rows.events.iter().all(|row| row.operation_kind == kind));
        for component in [Component::Renderer, Component::Ipc, Component::Application] {
            assert!(rows.events.iter().any(|row| row.component == component && row.event == Event::Completed));
        }
    }
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query { operation_id: Some(forged_operation), limit: 200, ..Default::default() }).unwrap();
    assert!(!rows.has_more);
    assert!(rows.events.iter().any(|row| row.component == Component::Ipc && row.event == Event::Superseded));
    let bytes = std::fs::read(database).unwrap();
    assert!(!bytes.windows(b"private-browsing-file".len()).any(|window| window == b"private-browsing-file"));
    assert!(!bytes.windows(b"private browsing bytes".len()).any(|window| window == b"private browsing bytes"));
}

#[test]
fn shutdown_rejects_new_repository_commands_without_mutating_workspace() {
    let app = mock_builder().manage(RepositoryService::new())
        .invoke_handler(tauri::generate_handler![commands::workspace_snapshot, commands::rename_repository])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    assert!(get_ipc_response(&main, request(&main, "workspace_snapshot", serde_json::json!({}))).is_err());
    assert!(get_ipc_response(&main, request(&main, "rename_repository", serde_json::json!({
        "entryId": "unknown", "displayName": "Must not be saved",
    }))).is_err());
    assert!(tauri::async_runtime::block_on(app.state::<RepositoryService>().snapshot()).entries.is_empty());
}

#[cfg(unix)]
#[test]
fn shutdown_cancels_live_ipc_reaps_git_and_drains_an_accepted_save() {
    let (temp, root) = test_support::working_tree();
    let choices = temp.path().join("workspace.json");
    let executable = test_support::executable(temp.path(), &format!(r#"
case "$*" in *" rev-list "*)
    git "$@" || exit $?
    printf '%s\n' "$$" > {}
    : > {}
    exec sleep 30 ;;
esac
exec git "$@"
"#, test_support::quote(&temp.path().join(".gitview-child-pid")),
        test_support::quote(&temp.path().join(".gitview-entered"))));
    let service = tauri::async_runtime::block_on(RepositoryService::with_workspace_file(choices.clone()))
        .with_inspection_executable(&executable);
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture opens") };
        service.select(&entry_id).await;
        entry_id
    });
    let app = mock_builder().manage(service).build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let handle = app.handle().clone();
    let window = main.clone();
    let history_entry = entry.clone();
    let history = tauri::async_runtime::spawn(async move {
        commands::history_page(window, handle.state(), history_entry, None, None, None).await
    });
    tauri::async_runtime::block_on(test_support::wait_for_probe(temp.path()));
    let mut writer = test_support::WorkspaceWriteGate::new(&choices);
    let handle = app.handle().clone();
    let rename = tauri::async_runtime::spawn(async move {
        commands::rename_repository(main, handle.state(), entry, "Accepted choice".into(), None).await
    });
    let (child_alive, history_result, rename_result) = tauri::async_runtime::block_on(async {
        writer.entered().await;
        let service = app.state::<RepositoryService>();
        let shutdown = service.shutdown();
        tokio::pin!(shutdown);
        assert!(std::future::poll_fn(|context| {
            std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
        }).await, "an accepted blocked save must keep shutdown pending");
        writer.release();
        writer.completed().await;
        tokio::time::timeout(WAIT, shutdown).await.unwrap();
        let pid = std::fs::read_to_string(temp.path().join(".gitview-child-pid")).unwrap();
        let alive = std::process::Command::new("kill").args(["-0", pid.trim()]).output().unwrap().status.success();
        // Keep the failing-before fixture from leaving its live child behind.
        if alive { history.abort(); }
        let history_result = history.await;
        test_support::wait_for_reaped_child(temp.path()).await;
        let rename_result = rename.await;
        service.shutdown().await;
        (alive, history_result, rename_result)
    });
    assert!(!child_alive, "shutdown returned before the foreground Git child was reaped");
    assert!(history_result.unwrap().is_err(), "cancelled review must not publish a late success");
    assert!(rename_result.unwrap().is_err(), "cancelled requester must not wait for the accepted save");
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(choices).unwrap()).unwrap();
    assert_eq!(saved["repositories"][0]["displayName"], "Accepted choice");
}

#[tokio::test]
async fn shutdown_waits_for_blocking_review_cleanup_after_a_cancelled_shutdown_waiter() {
    let temp = tempfile::tempdir().unwrap();
    let completed = temp.path().join("completed");
    let service = std::sync::Arc::new(RepositoryService::new());
    let (entered, entering) = oneshot::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let child = std::sync::Arc::clone(&service);
    let written = completed.clone();
    let request = tokio::spawn(async move {
        child.admit_native_request().unwrap().run(async move {
            crate::native_work::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = gate.recv();
                std::fs::write(written, b"finished").unwrap();
            }).await.unwrap();
        }).await
    });
    entering.await.unwrap();
    {
        let shutdown = service.shutdown();
        tokio::pin!(shutdown);
        assert!(std::future::poll_fn(|context| {
            std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
        }).await);
    }
    assert!(service.admit_native_request().is_err());
    let shutdown = service.shutdown();
    tokio::pin!(shutdown);
    assert!(std::future::poll_fn(|context| {
        std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
    }).await, "cancelling a shutdown waiter must not abandon blocking cleanup");
    release.send(()).unwrap();
    tokio::time::timeout(WAIT, shutdown).await.unwrap();
    assert!(request.await.unwrap().is_err());
    assert_eq!(std::fs::read(completed).unwrap(), b"finished");
}

#[tokio::test]
async fn shutdown_is_local_to_one_service_and_does_not_stop_other_native_work() {
    let (_temp, root) = test_support::working_tree();
    let first = RepositoryService::new();
    let other = RepositoryService::new();
    first.shutdown().await;
    assert!(matches!(other.admit_native_request().unwrap().run(other.open_chosen(&root)).await,
        Ok(OpenOutcome::Opened { .. })));
    assert!(first.snapshot().await.entries.is_empty());
    other.shutdown().await;
}

#[tokio::test]
async fn shutdown_during_a_final_ipc_poll_rejects_the_response_without_completed_trace() {
    let temp = tempfile::tempdir().unwrap();
    let database = temp.path().canonicalize().unwrap().join("diagnostics.sqlite");
    let store = DiagnosticStore::open(&database);
    let service = RepositoryService::with_diagnostics(store.sink());
    let context = ipc_context(&service, OperationKind::WorkspaceSnapshot, None).unwrap();
    let operation = context.id();
    let result = traced_ipc(&service, context, async {
        service.begin_shutdown();
        service.snapshot().await
    }).await;
    service.shutdown().await;
    shutdown_diagnostics(&store);
    assert!(result.is_err(), "closing during the final poll must win over its successful result");
    let rows = ReadOnlyDiagnostics::open(&database).unwrap().events(&Query {
        operation_id: Some(operation), ..Default::default()
    }).unwrap();
    assert!(rows.events.iter().any(|row| row.component == Component::Ipc && row.event == Event::Cancelled));
    assert!(!rows.events.iter().any(|row| row.component == Component::Ipc && row.event == Event::Completed));
}

struct GatedReviewCleanup {
    entered: Option<oneshot::Sender<()>>,
    release: std::sync::mpsc::Receiver<()>,
}

impl Drop for GatedReviewCleanup {
    fn drop(&mut self) {
        if let Some(entered) = self.entered.take() { let _ = entered.send(()); }
        let _ = self.release.recv();
    }
}

#[tokio::test]
async fn shutdown_waits_until_an_abandoned_blocking_result_finishes_cleanup() {
    let service = std::sync::Arc::new(RepositoryService::new());
    let (entered, entering) = oneshot::channel();
    let (release_work, work_gate) = std::sync::mpsc::channel();
    let (cleanup_entered, cleanup_entering) = oneshot::channel();
    let (release_cleanup, cleanup_gate) = std::sync::mpsc::channel();
    let child = std::sync::Arc::clone(&service);
    let request = tokio::spawn(async move {
        child.admit_native_request().unwrap().run(async move {
            crate::native_work::spawn_blocking(move || {
                entered.send(()).unwrap();
                let _ = work_gate.recv();
                GatedReviewCleanup { entered: Some(cleanup_entered), release: cleanup_gate }
            }).await.unwrap()
        }).await
    });
    entering.await.unwrap();
    service.begin_shutdown();
    assert!(request.await.unwrap().is_err());
    release_work.send(()).unwrap();
    cleanup_entering.await.unwrap();
    let shutdown = service.shutdown();
    tokio::pin!(shutdown);
    assert!(std::future::poll_fn(|context| {
        std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
    }).await, "an abandoned review result must finish destruction before shutdown returns");
    release_cleanup.send(()).unwrap();
    tokio::time::timeout(WAIT, shutdown).await.unwrap();
}

#[tokio::test]
async fn shutdown_keeps_an_accepted_save_owned_when_its_first_waiter_is_cancelled() {
    let (temp, root) = test_support::working_tree();
    let choices = temp.path().join("workspace.json");
    let service = std::sync::Arc::new(RepositoryService::with_workspace_file(choices.clone()).await);
    let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture opens") };
    let mut writer = test_support::WorkspaceWriteGate::new(&choices);
    let child = std::sync::Arc::clone(&service);
    let rename = tokio::spawn(async move {
        child.admit_native_request().unwrap().run(child.rename(&entry_id, "Durable after cancellation")).await
    });
    writer.entered().await;
    service.begin_shutdown();
    assert!(rename.await.unwrap().is_err());
    {
        let shutdown = service.shutdown();
        tokio::pin!(shutdown);
        assert!(std::future::poll_fn(|context| {
            std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
        }).await);
    }
    let shutdown = service.shutdown();
    tokio::pin!(shutdown);
    assert!(std::future::poll_fn(|context| {
        std::task::Poll::Ready(shutdown.as_mut().poll(context).is_pending())
    }).await, "a second shutdown must still own the accepted persistence tail");
    writer.release();
    writer.completed().await;
    tokio::time::timeout(WAIT, shutdown).await.unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(choices).unwrap()).unwrap();
    assert_eq!(saved["repositories"][0]["displayName"], "Durable after cancellation");
}

#[test]
fn review_surface_bootstrap_uses_native_caller_identity_and_rejects_remote_content() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::review_surface_bootstrap])
        .build(app_context()).unwrap();
    let main = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build().unwrap();
    let unrelated = tauri::WebviewWindowBuilder::new(&app, "unrelated", Default::default()).build().unwrap();
    assert_eq!(response(&main, "review_surface_bootstrap", serde_json::json!({})), "main");
    assert!(get_ipc_response(&unrelated, request(&unrelated, "review_surface_bootstrap",
        serde_json::json!({"surface": "main"}))).is_err());

    let remote_app = mock_builder()
        .invoke_handler(tauri::generate_handler![commands::review_surface_bootstrap])
        .build(app_context()).unwrap();
    let remote = tauri::WebviewWindowBuilder::new(&remote_app, "main",
        tauri::WebviewUrl::External("https://example.invalid/".parse().unwrap())).build().unwrap();
    assert!(get_ipc_response(&remote, request(&remote, "review_surface_bootstrap", serde_json::json!({}))).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn companion_cannot_admit_worktrees_manage_preferences_or_read_diagnostic_health() {
    let app = mock_builder().manage(RepositoryService::new())
        .manage(DiagnosticStore::unavailable(Code::StorageUnavailable))
        .invoke_handler(tauri::generate_handler![
            commands::review_surface_bootstrap, commands::select_worktree,
            commands::diagnostic_health, commands::set_companion_enabled,
            commands::pending_review_handoff, commands::claim_review_handoff,
            commands::ack_review_handoff,
        ]).build(app_context()).unwrap();
    let companion = tauri::WebviewWindowBuilder::new(&app, "companion", Default::default()).build().unwrap();
    assert_eq!(response(&companion, "review_surface_bootstrap", serde_json::json!({})), "companion");
    for command in ["select_worktree", "diagnostic_health", "set_companion_enabled",
        "pending_review_handoff", "claim_review_handoff", "ack_review_handoff"] {
        assert!(get_ipc_response(&companion, request(&companion, command, serde_json::json!({
            "entryId": "forged", "worktreeId": "forged", "enabled": true,
            "requestId": "forged", "contextEpoch": "forged", "outcome": "applied",
        }))).is_err(), "{command}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn companion_diagnostics_reject_forbidden_command_identities_without_capture() {
    let app = mock_builder().manage(DiagnosticStore::unavailable(Code::StorageUnavailable))
        .invoke_handler(tauri::generate_handler![commands::record_renderer_diagnostic])
        .build(app_context()).unwrap();
    let companion = tauri::WebviewWindowBuilder::new(&app, "companion", Default::default()).build().unwrap();
    let mut body = renderer_terminal(Uuid::new_v4(), "completed");
    body["diagnostic"]["command"] = serde_json::json!("select_worktree");
    assert!(get_ipc_response(&companion, request(&companion, "record_renderer_diagnostic", body)).is_err());
    assert_eq!(app.state::<DiagnosticStore>().health().accepted, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn visible_companion_can_read_admitted_context_but_hidden_companion_cannot() {
    let (temp, root) = test_support::working_tree();
    std::fs::write(root.join("changed.txt"), "read-only fixture").unwrap();
    let service = RepositoryService::new();
    let entry = tauri::async_runtime::block_on(async {
        let OpenOutcome::Opened { entry_id, .. } = service.open_chosen(&root).await else { panic!("fixture opens") };
        service.select(&entry_id).await;
        entry_id
    });
    service.set_companion_available(true);
    service.set_surface_visibility(ReviewCaller::Companion, true);
    let app = mock_builder().manage(service)
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot, commands::select_context, commands::observe_selected_context,
        ]).build(app_context()).unwrap();
    let companion = tauri::WebviewWindowBuilder::new(&app, "companion", Default::default()).build().unwrap();
    let snapshot = response(&companion, "workspace_snapshot", serde_json::json!({}));
    assert_eq!(snapshot["activeContextId"], entry);
    let rejected = response(&companion, "select_context", serde_json::json!({"entryId": "unadmitted"}));
    assert_eq!(rejected["kind"], "not_found");
    assert_eq!(rejected["snapshot"]["activeContextId"], entry);
    app.state::<RepositoryService>().set_surface_visibility(ReviewCaller::Companion, false);
    assert!(get_ipc_response(&companion, request(&companion, "workspace_snapshot", serde_json::json!({}))).is_err());
    assert!(get_ipc_response(&companion, request(&companion, "observe_selected_context",
        serde_json::json!({"entryId": entry}))).is_err());
    assert_eq!(std::fs::read_to_string(root.join("changed.txt")).unwrap(), "read-only fixture");
    tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
    drop(temp);
}
