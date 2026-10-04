//! Composes the native host and restricts commands to the main window.
//!
//! Only the picker and Git discovery supply paths; renderer commands use opaque IDs.

mod commands;
mod languages;
mod renderer_diagnostics;
mod companion;

use std::{future::Future, time::Duration};
use tauri::{Manager, WebviewWindow};
use uuid::Uuid;

use crate::application::RepositoryService;
use crate::companion::ReviewCaller;
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
    if service.is_shutting_down() { return Err(crate::native_work::SHUTTING_DOWN); }
    let id = metadata.map(operation_id).transpose()?;
    let context = OperationContext::new(service.diagnostic_sink(), id, None).with_kind(kind);
    if id.is_some() {
        context.record(Level::Info, Component::Renderer, Event::Started, None, DiagnosticDetails::default());
    }
    Ok(context)
}

fn traced_ipc<'a, T: DiagnosticOutcome + 'a>(
    service: &'a RepositoryService, context: OperationContext, future: impl Future<Output = T> + 'a,
) -> impl Future<Output = Result<T, &'static str>> + 'a {
    // Box before constructing the async state machine; an async fn would retain the
    // concrete future in its arguments and multiply it through nested tracing/IPC frames.
    let future = Box::pin(future);
    async move {
        let request = service.admit_native_request()?;
        context.scope(async {
            let mut trace = OperationTrace::new(context.clone(), Component::Ipc);
            let result = request.run(future).await;
            if let Ok(outcome) = &result { trace.outcome(outcome); }
            result
        }).await
    }
}

fn traced_surface_ipc<'a, T: DiagnosticOutcome + 'a>(
    service: &'a RepositoryService, context: OperationContext,
    future: impl Future<Output = Result<T, crate::companion::CompanionCode>> + 'a,
) -> impl Future<Output = Result<T, &'static str>> + 'a {
    let future = Box::pin(future);
    async move {
        let request = service.admit_native_request()?;
        context.scope(async {
            let mut trace = OperationTrace::new(context.clone(), Component::Ipc);
            let result = request.run(future).await?.map_err(|code| code.as_str());
            if let Ok(outcome) = &result { trace.outcome(outcome); }
            result
        }).await
    }
}

fn local_surface<R: tauri::Runtime>(window: &WebviewWindow<R>) -> bool {
    let Ok(url) = window.url() else { return false; };
    if url.scheme() == "tauri" && url.host_str() == Some("localhost") { return true; }
    if matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost") { return true; }
    cfg!(debug_assertions) && window.app_handle().config().build.dev_url.as_ref()
        .is_some_and(|configured| url.origin() == configured.origin())
}

fn require_main_window<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<(), &'static str> {
    if window.label() == "main" && local_surface(window) { Ok(()) }
    else { Err("This command is only available in the main window.") }
}

fn require_review_window<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<ReviewCaller, &'static str> {
    if !local_surface(window) { return Err("This command requires a local review surface."); }
    match window.label() {
        "main" => Ok(ReviewCaller::Main),
        "companion" if cfg!(target_os = "macos") => Ok(ReviewCaller::Companion),
        _ => Err("This command requires a local review surface."),
    }
}

fn require_companion_window<R: tauri::Runtime>(window: &WebviewWindow<R>) -> Result<(), &'static str> {
    if matches!(require_review_window(window)?, ReviewCaller::Companion) { Ok(()) }
    else { Err("This command is only available in the companion.") }
}

// Expand the generated context once: on macOS it embeds a single exported Info.plist symbol.
fn app_context<R: tauri::Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

fn reconcile_surface_demand(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let service = app.state::<RepositoryService>();
        if let Ok(request) = service.admit_native_request() {
            let _ = request.run(service.reconcile_surface_demand()).await;
        }
    });
}

fn companion_callbacks(app: &tauri::AppHandle) -> companion::HostCallbacks {
    use std::sync::Arc;
    let access_app = app.clone();
    let open_app = app.clone();
    let hide_app = app.clone();
    let main_app = app.clone();
    let quit_app = app.clone();
    companion::HostCallbacks {
        access_changed: Arc::new(move |enabled, available| {
            access_app.state::<RepositoryService>().set_companion_available(enabled && available);
            reconcile_surface_demand(&access_app);
        }),
        open: Arc::new(move || {
            let service = open_app.state::<RepositoryService>();
            service.set_surface_visibility(ReviewCaller::Companion, true);
            let scope = service.capture_surface_scope(ReviewCaller::Companion).ok();
            reconcile_surface_demand(&open_app);
            scope.map(|scope| scope.open_epoch)
        }),
        hide: Arc::new(move || {
            hide_app.state::<RepositoryService>().set_surface_visibility(ReviewCaller::Companion, false);
            reconcile_surface_demand(&hide_app);
        }),
        main_visibility: Arc::new(move |visible| {
            main_app.state::<RepositoryService>().set_surface_visibility(ReviewCaller::Main, visible);
            reconcile_surface_demand(&main_app);
        }),
        quit: Arc::new(move || {
            quit_app.state::<RepositoryService>().begin_shutdown();
            quit_app.exit(0);
        }),
    }
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
            let controller = companion::initialize(app.handle(), companion_callbacks(app.handle()));
            app.manage(controller);
            Ok(())
        })
        .on_window_event(|window, event| {
            let app = window.app_handle();
            let Some(service) = app.try_state::<RepositoryService>() else { return; };
            if window.label() == "main" {
                if let Some(controller) = app.try_state::<companion::CompanionController>() {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            if controller.handle_main_close() { api.prevent_close(); }
                        },
                        tauri::WindowEvent::Focused(focused) => controller.handle_main_focus(*focused),
                        _ => {},
                    }
                    if matches!(event, tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Destroyed) {
                        controller.refresh_main_visibility();
                    }
                }
            }
            if window.label() == "companion" {
                if let Some(controller) = app.try_state::<companion::CompanionController>() {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            if controller.handle_companion_close() { api.prevent_close(); }
                        },
                        tauri::WindowEvent::Destroyed => controller.handle_companion_destroyed(),
                        _ => {},
                    }
                }
            }
            if matches!(event, tauri::WindowEvent::Destroyed) {
                let caller = match window.label() {
                    "main" => ReviewCaller::Main,
                    "companion" => ReviewCaller::Companion,
                    _ => return,
                };
                service.set_surface_visibility(caller, false);
                service.unsubscribe_review_surface(caller);
                reconcile_surface_demand(app);
            }
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
            commands::review_commit_file,
            commands::list_repository_files,
            commands::review_repository_file,
            commands::record_renderer_diagnostic,
            commands::diagnostic_health,
            commands::review_surface_bootstrap,
            commands::review_surface_snapshot,
            commands::subscribe_review_surface,
            commands::companion_state,
            commands::set_companion_enabled,
            commands::publish_companion_presentation,
            commands::begin_companion_review,
            commands::dismiss_companion,
            commands::request_review_handoff,
            commands::pending_review_handoff,
            commands::claim_review_handoff,
            commands::ack_review_handoff,
            commands::quit_companion,
        ])
        .build(app_context())
        .expect("failed to start GitView");
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
            app.state::<RepositoryService>().begin_shutdown();
            app.state::<companion::CompanionController>().quit_cleanup();
        }
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
