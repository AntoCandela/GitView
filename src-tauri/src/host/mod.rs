//! Composes the native host and restricts commands to the main window.
//!
//! Only the picker and Git discovery supply paths; renderer commands use opaque IDs.

mod commands;
mod languages;
mod renderer_diagnostics;

use std::{future::Future, time::Duration};
use tauri::{Manager, WebviewWindow};
use uuid::Uuid;

use crate::application::RepositoryService;
use crate::diagnostic_operation::{DiagnosticOutcome, OperationTrace};
use crate::diagnostics::{Code, Component, DiagnosticDetails, DiagnosticStore, Event, Level, OperationContext, OperationKind};

const INVALID_DIAGNOSTIC_METADATA: &str = "Invalid diagnostic metadata.";

fn operation_id(value: &str) -> Result<Uuid, &'static str> {
    if value.len() != 36 { return Err(INVALID_DIAGNOSTIC_METADATA); }
    let id = Uuid::parse_str(value).map_err(|_| INVALID_DIAGNOSTIC_METADATA)?;
    let mut buffer = Uuid::encode_buffer();
    if id.get_version() != Some(uuid::Version::Random)
        || id.get_variant() != uuid::Variant::RFC4122
        || id.hyphenated().encode_lower(&mut buffer) != value {
        return Err(INVALID_DIAGNOSTIC_METADATA);
    }
    Ok(id)
}

fn ipc_context(service: &RepositoryService, kind: OperationKind, metadata: Option<&str>) -> Result<OperationContext, &'static str> {
    let id = metadata.map(operation_id).transpose()?;
    let context = OperationContext::new(service.diagnostic_sink(), id, None).with_kind(kind);
    if id.is_some() {
        context.record(Level::Info, Component::Renderer, Event::Started, None, DiagnosticDetails::default());
    }
    Ok(context)
}

fn traced_ipc<T: DiagnosticOutcome>(context: OperationContext, future: impl Future<Output = T>) -> impl Future<Output = T> {
    // Box before constructing the async state machine; an async fn would retain the
    // concrete future in its arguments and multiply it through nested tracing/IPC frames.
    let future = Box::pin(future);
    async move {
        context.scope(async {
            let mut trace = OperationTrace::new(context.clone(), Component::Ipc);
            let outcome = future.await;
            trace.outcome(&outcome);
            outcome
        }).await
    }
}

// Keep every command behind the same window boundary, including read-only snapshots.
fn require_main_window<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<(), &'static str> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("This command is only available in the main window.")
    }
}

// Expand the generated context once: on macOS it embeds a single exported Info.plist symbol.
fn app_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

/// Starts the desktop host with one authoritative repository service.
///
/// # Panics
/// Panics if Tauri cannot start the configured application.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (store, service) = tauri::async_runtime::block_on(async {
                match app.path().app_data_dir() {
                    Ok(directory) => {
                        let store = DiagnosticStore::open(directory.join("diagnostics").join("diagnostics.sqlite"));
                        let service = RepositoryService::with_workspace_file_and_diagnostics(
                            directory.join("workspace.json"), store.sink(),
                        ).await;
                        (store, service)
                    },
                    Err(_) => {
                        let store = DiagnosticStore::unavailable(Code::StorageUnavailable);
                        let service = RepositoryService::storage_unavailable(store.sink()).await;
                        (store, service)
                    },
                }
            });
            app.manage(store);
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot,
            commands::preferred_languages,
            commands::open_chosen_repository,
            commands::select_context,
            commands::rename_repository,
            commands::remove_repository,
            commands::refresh_entry_availability,
            commands::observe_selected_context,
            commands::review_file,
            commands::history_page,
            commands::list_contexts,
            commands::select_worktree,
            commands::commit_files,
            commands::upstream_files,
            commands::review_commit_file,
            commands::list_repository_files,
            commands::review_repository_file,
            commands::record_renderer_diagnostic,
            commands::diagnostic_health,
        ])
        .build(app_context())
        .expect("failed to start GitView");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            tauri::async_runtime::block_on(app.state::<RepositoryService>().shutdown());
            if let Err(code) = app.state::<DiagnosticStore>().shutdown(Duration::from_secs(2)) {
                eprintln!("gitview diagnostics shutdown incomplete: {}", code.as_str());
            }
        }
    });
}

#[cfg(test)]
#[path = "../../tests/integration/host.rs"]
mod integration_tests;
