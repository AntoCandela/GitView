//! Decodes authorized workspace commands and owns the native folder picker.

use tauri::WebviewWindow;
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use super::{ipc_context, require_main_window, traced_ipc};
use super::languages::{self, Locale, PreferredLanguages};
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
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::WorkspaceSnapshot, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.snapshot()).await)
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
    Ok(traced_ipc(context, async {
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
    }).await)
}

#[tauri::command]
pub(super) async fn select_context<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<SelectOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::SelectContext, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.select(&entry_id)).await)
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
    Ok(traced_ipc(context, service.rename(&entry_id, &display_name)).await)
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
    Ok(traced_ipc(context, service.remove(&entry_id)).await)
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
    Ok(traced_ipc(context, service.refresh(&entry_id)).await)
}

#[tauri::command]
pub(super) async fn observe_selected_context<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    service: tauri::State<'_, RepositoryService>,
    entry_id: String,
    operation_id: Option<String>,
) -> Result<ObservationSnapshot, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ObserveContext, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.observe_selected_context(&entry_id)).await)
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
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ReviewFile, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.review_file(&entry_id, observation_revision, &path_id, category)).await)
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
    Ok(traced_ipc(context, service.history_page(&entry_id, cursor.as_deref(), branch.as_deref())).await)
}

#[tauri::command]
pub(super) async fn list_contexts<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, operation_id: Option<String>,
) -> Result<ContextOptionsResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ListContexts, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.list_contexts(&entry_id)).await)
}

#[tauri::command]
pub(super) async fn select_worktree<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, worktree_id: String, operation_id: Option<String>,
) -> Result<MutationOutcome, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::SelectWorktree, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.select_worktree(&entry_id, &worktree_id)).await)
}

#[tauri::command]
pub(super) async fn commit_files<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, commit_oid: String, parent_oid: Option<String>, operation_id: Option<String>,
) -> Result<CommitFilesResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::CommitFiles, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.commit_files(&entry_id, &commit_oid, parent_oid.as_deref())).await)
}

#[tauri::command]
pub(super) async fn review_commit_file<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, commit_oid: String, parent_oid: Option<String>, file_id: String,
    operation_id: Option<String>,
) -> Result<CommitReviewResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ReviewCommitFile, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.review_commit_file(&entry_id, &commit_oid, parent_oid.as_deref(), &file_id)).await)
}

#[tauri::command]
pub(super) async fn list_repository_files<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, request: RepositoryFilesRequest, operation_id: Option<String>,
) -> Result<RepositoryFilesResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ListRepositoryFiles, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.list_repository_files(&entry_id, request)).await)
}

#[tauri::command]
pub(super) async fn review_repository_file<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>,
    entry_id: String, listing_id: String, file_id: String, operation_id: Option<String>,
) -> Result<RepositoryFileResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::ReviewRepositoryFile, operation_id.as_deref())?;
    Ok(traced_ipc(context, service.review_repository_file(&entry_id, &listing_id, &file_id)).await)
}

#[tauri::command]
pub(super) fn record_renderer_diagnostic<R: tauri::Runtime>(
    window: WebviewWindow<R>,
    store: tauri::State<'_, DiagnosticStore>,
    diagnostic: RendererDiagnostic,
) -> Result<(), &'static str> {
    require_main_window(&window)?;
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
