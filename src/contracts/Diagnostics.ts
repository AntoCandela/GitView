/** Defines closed renderer diagnostic metadata and native in-memory health without payloads or paths. */

/** Diagnostic identity never authorizes a new native command. */
export type RepositoryCommand =
  | "workspace_snapshot"
  | "open_chosen_repository"
  | "select_context"
  | "refresh_entry_availability"
  | "observe_selected_context"
  | "review_file"
  | "history_page"
  | "list_contexts"
  | "select_worktree"
  | "commit_files"
  | "upstream_files"
  | "review_commit_file"
  | "list_repository_files"
  | "review_repository_file"
  | "rename_repository"
  | "remove_repository";

/** Terminal transport facts do not reinterpret native domain outcomes. */
export type RendererDiagnosticPhase = "completed" | "transport_failed";

export const MAX_RENDERER_DIAGNOSTIC_DURATION_MS = 86_400_000;

/** Exact record_renderer_diagnostic payload; the host rejects unknown fields. */
export interface RendererDiagnostic {
  /** Lowercase canonical UUID v4, shared with the original invocation. */
  operationId: string;
  command: RepositoryCommand | import("./pullRequests").PullRequestCommand;
  phase: RendererDiagnosticPhase;
  /** Monotonic elapsed milliseconds, floored and clamped to 0..86_400_000. */
  durationMs: number;
}

/** Mirrors native safe codes; unknown error strings never enter this contract. */
export type DiagnosticCode =
  | "integration_unavailable" | "pr_unavailable" | "pr_stale_context"
  | "git_unavailable"
  | "not_repository"
  | "inaccessible"
  | "unsafe_repository"
  | "probe_timeout"
  | "repository_changed"
  | "unsupported_path_encoding"
  | "repository_unavailable"
  | "invalid_status"
  | "unsupported_configuration"
  | "review_unsupported"
  | "invalid_output"
  | "changed_during_read"
  | "resource_limit"
  | "stale_selection"
  | "stale_cursor"
  | "missing_objects"
  | "status_unavailable"
  | "load_failed"
  | "unsupported_version"
  | "save_failed"
  | "storage_unavailable"
  | "process_start"
  | "process_io"
  | "output_limit"
  | "deadline"
  | "cleanup"
  | "transport"
  | "overflow"
  | "storage"
  | "schema"
  | "invalid_record"
  | "invalid_arguments"
  | "shutdown"
  | "timeout"
  | "disabled";

/** Native in-memory counters; accepted submissions are not persistence acknowledgements. */
export interface DiagnosticHealth {
  state: "healthy" | "degraded" | "disabled" | "stopped";
  accepted: number;
  written: number;
  dropped: number;
  last_error_code: DiagnosticCode | null;
}

/** Health reads are direct and cannot recursively generate repository diagnostics. */
export interface DiagnosticHealthClient {
  diagnosticHealth(): Promise<DiagnosticHealth>;
}
