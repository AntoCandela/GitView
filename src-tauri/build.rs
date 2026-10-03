//! Declares the native command permission surface for Tauri's build-time manifest.

fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "workspace_snapshot",
            "open_chosen_repository",
            "select_context",
            "rename_repository",
            "remove_repository",
            "refresh_entry_availability",
            "observe_selected_context",
            "review_file",
            "history_page",
            "list_contexts",
            "select_worktree",
            "commit_files",
            "review_commit_file",
            "list_repository_files",
            "review_repository_file",
            "record_renderer_diagnostic",
            "diagnostic_health",
        ]),
    ))
    .expect("failed to build GitView host");
}
