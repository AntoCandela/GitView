/** Carries native-owned surface epochs and bounded read-only handoff authority, never filesystem paths. */
import type { Locale } from "../i18n";
import type { ObservationSnapshot } from "./changes";
import type { ReviewCategory, ReviewSelection } from "./diff";
import type { RepositoryClient, WorkspaceSnapshot } from "./repositories";

export type ReviewSurface = "main" | "companion";
export type CompanionCode = "disabled" | "not_visible" | "stale_surface" | "stale_context" |
  "unavailable" | "window_unavailable" | "delivery_timeout" | "busy" | "invalid_request";
export interface CompanionState {
  supported: boolean;
  enabled: boolean;
  available: boolean;
  visible: boolean;
  revision: number;
  persistenceError: null | "load_failed" | "unsupported_version" | "save_failed" | "storage_unavailable";
  nativeError: null | "tray_failed" | "panel_failed" | "show_failed" | "focus_failed" | "main_unavailable";
}
export interface PresentationInput {
  locale: Locale;
  appearanceTheme: "cream" | "paper" | "mist" | "stone" | "graphite" | "midnight";
  iconTheme: "classic" | "material" | "catppuccin";
  review: {
    mode: "changes" | "full";
    theme: "plain" | "github-light" | "catppuccin-latte" | "solarized-light" | "github-dark" | "match";
    lineMode: "scroll" | "wrap";
  };
  persistenceError: boolean;
  menuLabels: { openGitView: string; quit: string };
}
export interface PresentationSnapshot extends PresentationInput { revision: number }
export interface ReviewSurfaceSnapshot {
  workspace: WorkspaceSnapshot;
  visible: boolean;
  openEpoch: string;
  observation: ObservationSnapshot | null;
  presentation: PresentationSnapshot | null;
  handoff: { revision: number; pendingRequestId: string | null };
}
export type SurfaceNotice =
  | { kind: "invalidate"; workspaceRevision: number; contextEpoch: string; openEpoch: string }
  | { kind: "visibility"; visible: boolean; openEpoch: string }
  | { kind: "presentation"; revision: number }
  | { kind: "handoff"; revision: number; requestId: string | null };
export type BeginCompanionReviewResult =
  | { kind: "ready"; surface: ReviewSurfaceSnapshot }
  | { kind: "unavailable" | "stale"; code: CompanionCode };
export interface HandoffSelection {
  entryId: string;
  stablePathId: string;
  observationRevision: number;
  pathId: string;
  category: ReviewCategory;
}
export interface RequestReviewHandoff {
  openEpoch: string;
  contextEpoch: string;
  selection: HandoffSelection | null;
}
export type HandoffOutcome = "applied" | "changed" | "unavailable";
export type ReviewHandoffResult =
  | { kind: HandoffOutcome; requestId: string }
  | { kind: "failed"; code: CompanionCode };
export type ReviewHandoffTarget =
  | { kind: "live"; entryId: string; selection: ReviewSelection; pathId: string; observationRevision: number }
  | { kind: "no_remaining"; entryId: string; selection: ReviewSelection }
  | { kind: "unavailable"; entryId: string; code: CompanionCode }
  | null;
export interface PendingReviewHandoff {
  requestId: string;
  contextEpoch: string;
  entryId: string | null;
  target: ReviewHandoffTarget;
  sourceOpenEpoch: string;
  phase: "pending" | "claimed";
}
export interface PendingReviewHandoffSnapshot {
  revision: number;
  pending: PendingReviewHandoff | null;
}
export type ClaimReviewHandoffResult =
  | { kind: "claimed"; requestId: string; contextEpoch: string; handoffRevision: number; target: ReviewHandoffTarget; remainingMs: number }
  | { kind: "stale" | "busy"; code: CompanionCode };
export type AckReviewHandoffResult = { kind: "applied" } | { kind: "stale"; code: CompanionCode };
export interface ReviewSurfaceClient {
  bootstrap(): Promise<ReviewSurface>;
  snapshot(): Promise<ReviewSurfaceSnapshot>;
  /** Register before reading a snapshot; notices invalidate rather than replace authoritative state. */
  subscribe(listener: (notice: SurfaceNotice) => void): Promise<() => void>;
}
/** Only admitted-context review is exposed; management/discovery/native-window authority is absent. */
export interface CompanionClient extends Pick<RepositoryClient, "selectContext" | "observeSelectedContext" | "reviewFile"> {
  begin(openEpoch: string): Promise<BeginCompanionReviewResult>;
  dismiss(): Promise<void>;
  requestHandoff(request: RequestReviewHandoff): Promise<ReviewHandoffResult>;
  quit(): Promise<void>;
}
export interface CompanionSettingsClient {
  state(): Promise<CompanionState>;
  setEnabled(enabled: boolean): Promise<{ kind: "applied" | "unavailable"; state: CompanionState }>;
  publishPresentation(presentation: PresentationInput): Promise<PresentationSnapshot>;
}
export interface ReviewHandoffClient {
  pending(): Promise<PendingReviewHandoffSnapshot>;
  claim(requestId: string, contextEpoch: string): Promise<ClaimReviewHandoffResult>;
  ack(requestId: string, contextEpoch: string, outcome: HandoffOutcome): Promise<AckReviewHandoffResult>;
}
