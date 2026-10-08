//! Exposes the finite PR command surface only to the local main webview.
use tauri::WebviewWindow;
use super::{require_main_window, ipc_context, traced_ipc};
use crate::{application::RepositoryService, diagnostics::OperationKind};
use crate::github::model::{PrRequest, PrResult, CollectionKind, ComparisonSelection};

#[tauri::command]
pub(super) async fn pr_status<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrStatus, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Status)).await
}

#[tauri::command]
pub(super) async fn pr_associations<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    branch: Option<String>,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrAssociations, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Associations { branch })).await
}

#[tauri::command]
pub(super) async fn pr_map_head<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    association_id: String,
    owner: String,
    repository: String,
    head_ref: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrMapHead, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::MapHead { association_id, owner, repository, head_ref })).await
}

#[tauri::command]
pub(super) async fn pr_choose<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    association_id: String,
    candidate_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrChoose, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Choose { association_id, candidate_id })).await
}

#[tauri::command]
pub(super) async fn pr_open<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    pr_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrOpen, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Open { pr_id })).await
}

#[tauri::command]
pub(super) async fn pr_page<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    collection: CollectionKind,
    cursor: Option<String>,
    thread_id: Option<String>,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrPage, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Page { session_id, collection, cursor, thread_id })).await
}

#[tauri::command]
pub(super) async fn pr_refresh<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrRefresh, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Refresh { session_id })).await
}

#[tauri::command]
pub(super) async fn pr_compare<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    selection: ComparisonSelection,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrCompare, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Compare { session_id, selection })).await
}

#[tauri::command]
pub(super) async fn pr_files_page<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    comparison_id: String,
    cursor: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrFilesPage, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::FilesPage { comparison_id, cursor })).await
}

#[tauri::command]
pub(super) async fn pr_resolve_anchor<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    anchor_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrResolveAnchor, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::ResolveAnchor { session_id, anchor_id })).await
}

#[tauri::command]
pub(super) async fn pr_file<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    comparison_id: String,
    file_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrFile, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::File { comparison_id, file_id })).await
}

#[tauri::command]
pub(super) async fn pr_release<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrRelease, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::Release { session_id })).await
}

#[tauri::command]
pub(super) async fn pr_open_link<R: tauri::Runtime>(
    window: WebviewWindow<R>, service: tauri::State<'_, RepositoryService>, entry_id: String,
    session_id: String,
    link_id: String,
    operation_id: Option<String>,
) -> Result<PrResult, &'static str> {
    require_main_window(&window)?;
    let context = ipc_context(&service, OperationKind::PrOpenLink, operation_id.as_deref())?;
    traced_ipc(&service, context, service.pull_requests.execute(&service, &entry_id, PrRequest::OpenLink { session_id, link_id })).await
}

#[cfg(test)]
#[path = "../../tests/integration/github_host.rs"]
mod integration_tests;
