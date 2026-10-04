//! Decodes authorized workspace commands and owns the native folder picker.

use tauri::{Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::{ipc_context, require_main_window, require_review_window, require_companion_window, traced_ipc, traced_surface_ipc};
use super::languages::{self, PreferredLanguages};
use crate::locale::Locale;
use super::renderer_diagnostics::RendererDiagnostic;
use crate::application::RepositoryService;
use crate::browsing::{RepositoryFileResult, RepositoryFilesRequest, RepositoryFilesResult};
use crate::diagnostics::{DiagnosticHealth, DiagnosticStore, OperationKind};
use crate::diff::{ReviewCategory, ReviewResult};
use crate::diff::committed::CommitReviewResult;
use crate::git::GitError;
use crate::history::HistoryPageResult;
use crate::inspection::{CommitFilesResult, ContextOptionsResult};
use crate::observation::ObservationSnapshot;
use crate::workspace::{MutationOutcome, OpenOutcome, SelectOutcome, WorkspaceSnapshot};

#[tauri::command]
pub(super) fn preferred_languages<R: tauri::Runtime>(
    window: WebviewWindow<R>,
) -> Result<PreferredLanguages, &'static str> {
    require_main_window(&window)?;
    Ok(languages::preferred_languages())
}

#[tauri::command]
pub(super) async fn workspace_snapshot<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    operation_id: Option<String>,
) -> Result<WorkspaceSnapshot, &'static str> {
    let caller = require_review_window(&window)?;
    let scope = service.capture_surface_scope(caller).map_err(|code| code.as_str())?;
    let context = ipc_context(&service, OperationKind::WorkspaceSnapshot, operation_id.as_deref())?;
    traced_surface_ipc(&service, context, service.run_surface_request(&scope, service.snapshot())).await
}

#[tauri::command]
pub(super) async fn open_chosen_repository<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    locale: Locale,
    operation_id: Option<String>,
) -> Result<OpenOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::OpenRepository, operation_id.as_deref())?;
    traced_ipc(&service, context, async {
        let (send, receive) = oneshot::channel();
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title(locale.picker_title())
            .pick_folder(move |path| {
                // A closed receiver means the invoking future is gone; no path or state needs updating.
                let _ = send.send(path);
            });
        let chosen = match receive.await {
            Ok(Some(chosen)) => chosen,
            Ok(None) => return service.cancelled().await,
            Err(_) => return service.rejected(GitError::Unavailable).await,
        };
        let selected = match chosen.into_path() {
            Ok(path) => path,
            Err(_) => return service.rejected(GitError::UnsupportedPathEncoding).await,
        };
        service.open_chosen(&selected).await
    }).await
}

#[tauri::command]
pub(super) async fn select_context<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<SelectOutcome, &'static str> {
    let caller = require_review_window(&window)?;
    let context = ipc_context(&service, OperationKind::SelectContext, operation_id.as_deref())?;
    traced_surface_ipc(&service, context, service.select_context_for_surface(caller, &entry_id)).await
}

#[tauri::command]
pub(super) async fn rename_repository<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    display_name: String,
    operation_id: Option<String>,
) -> Result<MutationOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::RenameRepository, operation_id.as_deref())?;
    traced_ipc(&service, context, service.rename(&entry_id, &display_name)).await
}

#[tauri::command]
pub(super) async fn remove_repository<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<MutationOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::RemoveRepository, operation_id.as_deref())?;
    traced_ipc(&service, context, service.remove(&entry_id)).await
}

#[tauri::command]
pub(super) async fn refresh_entry_availability<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<WorkspaceSnapshot, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::RefreshAvailability, operation_id.as_deref())?;
    traced_ipc(&service, context, service.refresh(&entry_id)).await
}

#[tauri::command]
pub(super) async fn observe_selected_context<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<ObservationSnapshot, &'static str> {
    let caller = require_review_window(&window)?;
    let scope = service.capture_surface_scope(caller).map_err(|code| code.as_str())?;
    let context = ipc_context(&service, OperationKind::ObserveContext, operation_id.as_deref())?;
    traced_surface_ipc(&service, context,
        service.run_surface_request(&scope, service.observe_selected_context(&entry_id))).await
}

#[tauri::command]
pub(super) async fn review_file<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    observation_revision: u64,
    path_id: String,
    category: ReviewCategory,
    operation_id: Option<String>,
) -> Result<ReviewResult, &'static str> {
    let caller = require_review_window(&window)?;
    let scope = service.capture_surface_scope(caller).map_err(|code| code.as_str())?;
    let context = ipc_context(&service, OperationKind::ReviewFile, operation_id.as_deref())?;
    traced_surface_ipc(&service, context,
        service.review_file_for_surface(&scope, &entry_id, observation_revision, &path_id, category)).await
}

#[tauri::command]
pub(super) async fn history_page<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    cursor: Option<String>,
    branch: Option<String>,
    operation_id: Option<String>,
) -> Result<HistoryPageResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::HistoryPage, operation_id.as_deref())?;
    traced_ipc(&service, context, service.history_page(&entry_id, cursor.as_deref(), branch.as_deref())).await
}

#[tauri::command]
pub(super) async fn list_contexts<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, operation_id: Option<String>,
) -> Result<ContextOptionsResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ListContexts, operation_id.as_deref())?;
    traced_ipc(&service, context, service.list_contexts(&entry_id)).await
}

#[tauri::command]
pub(super) async fn select_worktree<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, worktree_id: String, operation_id: Option<String>,
) -> Result<MutationOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::SelectWorktree, operation_id.as_deref())?;
    traced_ipc(&service, context, service.select_worktree(&entry_id, &worktree_id)).await
}

#[tauri::command]
pub(super) async fn commit_files<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, commit_oid: String, parent_oid: Option<String>, operation_id: Option<String>,
) -> Result<CommitFilesResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::CommitFiles, operation_id.as_deref())?;
    traced_ipc(&service, context, service.commit_files(&entry_id, &commit_oid, parent_oid.as_deref())).await
}

#[tauri::command]
pub(super) async fn review_commit_file<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, commit_oid: String, parent_oid: Option<String>, file_id: String,
    operation_id: Option<String>,
) -> Result<CommitReviewResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ReviewCommitFile, operation_id.as_deref())?;
    traced_ipc(&service, context, service.review_commit_file(&entry_id, &commit_oid, parent_oid.as_deref(), &file_id)).await
}

#[tauri::command]
pub(super) async fn list_repository_files<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, request: RepositoryFilesRequest, operation_id: Option<String>,
) -> Result<RepositoryFilesResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ListRepositoryFiles, operation_id.as_deref())?;
    traced_ipc(&service, context, service.list_repository_files(&entry_id, request)).await
}

#[tauri::command]
pub(super) async fn review_repository_file<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, listing_id: String, file_id: String, operation_id: Option<String>,
) -> Result<RepositoryFileResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ReviewRepositoryFile, operation_id.as_deref())?;
    traced_ipc(&service, context, service.review_repository_file(&entry_id, &listing_id, &file_id)).await
}

#[tauri::command]
pub(super) fn record_renderer_diagnostic<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    store: tauri::State<'_, DiagnosticStore>,
    diagnostic: RendererDiagnostic,
) -> Result<(), &'static str> {
    let caller = require_review_window(&window)?;
    if matches!(caller, crate::companion::ReviewCaller::Companion) {
        diagnostic.require_companion_command()?;
    }
    diagnostic.record(&store)
}

#[tauri::command]
pub(super) fn diagnostic_health<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    store: tauri::State<'_, DiagnosticStore>,
) -> Result<DiagnosticHealth, &'static str> {
    require_main_window(&window)?;
    Ok(store.health())
}

#[tauri::command]
pub(super) fn review_surface_bootstrap<R: tauri::Runtime>(
    window: WebviewWindow<R>,
) -> Result<crate::companion::ReviewCaller, &'static str> {
    require_review_window(&window)
}

#[tauri::command]
pub(super) async fn review_surface_snapshot<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
) -> Result<crate::companion::ReviewSurfaceSnapshot, &'static str> {
    let caller = require_review_window(&window)?;
    service.admit_native_request()?.run(service.review_surface_snapshot(caller)).await
}

#[tauri::command]
pub(super) fn subscribe_review_surface<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    channel: tauri::ipc::Channel<crate::companion::SurfaceNotice>,
) -> Result<(), &'static str> {
    let caller = require_review_window(&window)?;
    let _request = service.admit_native_request()?;
    service.subscribe_review_surface(caller, std::sync::Arc::new(move |notice| {
        // Disconnection never changes native state; destruction/replacement retires the registration.
        let _ = channel.send(notice);
    }));
    Ok(())
}

fn companion_controller<R: tauri::Runtime>(
    window: &WebviewWindow<R>,
) -> Result<super::companion::CompanionController, &'static str> {
    window.try_state::<super::companion::CompanionController>()
        .map(|controller| controller.inner().clone()).ok_or("unavailable")
}

#[tauri::command]
pub(super) fn companion_state<R: tauri::Runtime>(
    window: WebviewWindow<R>,
) -> Result<super::companion::CompanionState, &'static str> {
    require_main_window(&window)?;
    Ok(companion_controller(&window)?.state())
}

#[tauri::command]
pub(super) async fn set_companion_enabled<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, enabled: bool,
) -> Result<super::companion::EnableResult, &'static str> {
    require_main_window(&window)?;
    let controller = companion_controller(&window)?;
    service.admit_native_request()?.run(controller.set_enabled(enabled)).await
}

#[tauri::command]
pub(super) async fn publish_companion_presentation<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    presentation: crate::companion::PresentationInput,
) -> Result<crate::companion::PresentationSnapshot, &'static str> {
    require_main_window(&window)?;
    let controller = companion_controller(&window)?;
    service.admit_native_request()?.run(async {
        let published = service.publish_companion_presentation(presentation).map_err(|code| code.as_str())?;
        let labels = super::companion::MenuLabels {
            open_git_view: published.menu_labels.open_git_view.clone(),
            quit: published.menu_labels.quit.clone(),
        };
        controller.publish_menu_labels(labels).await.map_err(|_| "unavailable")?;
        Ok(published)
    }).await?
}

#[tauri::command]
pub(super) async fn begin_companion_review<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, open_epoch: String,
) -> Result<crate::companion::BeginCompanionReviewResult, &'static str> {
    require_companion_window(&window)?;
    service.admit_native_request()?.run(service.begin_companion_review(&open_epoch)).await
}

#[tauri::command]
pub(super) async fn dismiss_companion<R: tauri::Runtime>(
    window: WebviewWindow<R>,
) -> Result<(), &'static str> {
    require_companion_window(&window)?;
    companion_controller(&window)?.dismiss().await.map_err(|_| "unavailable")
}

#[tauri::command]
pub(super) async fn request_review_handoff<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    request: crate::companion::RequestReviewHandoff,
) -> Result<crate::companion::ReviewHandoffResult, &'static str> {
    require_companion_window(&window)?;
    let controller = companion_controller(&window)?;
    let source_epoch = request.open_epoch.clone();
    let issued_id = std::sync::Arc::new(parking_lot::Mutex::new(None::<String>));
    let reveal_id = issued_id.clone();
    let reveal_controller = controller.clone();
    let result = service.admit_native_request()?.run(service.request_review_handoff(
        request, move |request_id, epoch| async move {
            *reveal_id.lock() = Some(request_id.clone());
            if !reveal_controller.begin_focus_transfer(&request_id, &epoch) {
                return Err(crate::companion::CompanionCode::StaleSurface);
            }
            reveal_controller.reveal_main().await.map_err(|_| crate::companion::CompanionCode::WindowUnavailable)
        },
    )).await?;
    let acknowledged = !matches!(&result, crate::companion::ReviewHandoffResult::Failed { .. });
    let request_id = issued_id.lock().clone();
    if let Some(request_id) = request_id {
        let _ = controller.finish_focus_transfer(&request_id, &source_epoch, acknowledged).await;
    }
    Ok(result)
}

#[tauri::command]
pub(super) fn pending_review_handoff<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
) -> Result<crate::companion::PendingReviewHandoffSnapshot, &'static str> {
    require_main_window(&window)?;
    Ok(service.pending_review_handoff())
}

#[tauri::command]
pub(super) fn claim_review_handoff<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    request_id: String, context_epoch: String,
) -> Result<crate::companion::ClaimReviewHandoffResult, &'static str> {
    require_main_window(&window)?;
    Ok(service.claim_review_handoff(&request_id, &context_epoch))
}

#[tauri::command]
pub(super) fn ack_review_handoff<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    request_id: String, context_epoch: String, outcome: crate::companion::HandoffOutcome,
) -> Result<crate::companion::AckReviewHandoffResult, &'static str> {
    require_main_window(&window)?;
    Ok(service.ack_review_handoff(&request_id, &context_epoch, outcome))
}

#[tauri::command]
pub(super) fn quit_companion<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
) -> Result<(), &'static str> {
    require_companion_window(&window)?;
    service.capture_surface_scope(crate::companion::ReviewCaller::Companion).map_err(|code| code.as_str())?;
    service.begin_shutdown();
    companion_controller(&window)?.quit_cleanup();
    window.app_handle().exit(0);
    Ok(())
}
