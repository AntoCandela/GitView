/** Subscribes before cached reads and reconciles one visible surface without hidden periodic IPC. */
import { useEffect, useMemo, useSyncExternalStore } from "react";
import type { ReviewSurfaceClient, ReviewSurfaceSnapshot, SurfaceNotice } from "../../contracts/companion";
import type { WorkspaceSnapshot } from "../../contracts/repositories";

export interface SurfaceState { snapshot: ReviewSurfaceSnapshot | null; generation: number; noticeRevision: number; unavailable: boolean; reconciling: boolean }

export class ReviewSurfaceConnection {
  private state: SurfaceState = { snapshot: null, generation: 0, noticeRevision: 0, unavailable: false, reconciling: true };
  private listeners = new Set<() => void>();
  private active = false;
  private lifetime = 0;
  private reading = false;
  private dirty = false;
  private timer: number | undefined;
  private unsubscribe: (() => void) | undefined;
  private bootstrapRetries = 2;
  constructor(private readonly client: ReviewSurfaceClient) {}
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; };
  private publish(state: SurfaceState) { this.state = state; this.listeners.forEach((listener) => listener()); }

  start() {
    this.active = true;
    const lifetime = ++this.lifetime;
    this.bootstrapRetries = 2;
    const reveal = () => {
      // DOM focus/visibility are read triggers only; host snapshots remain visibility authority.
      if (this.active && lifetime === this.lifetime) this.refresh();
    };
    const visibility = () => { if (document.visibilityState === "visible") reveal(); };
    window.addEventListener("focus", reveal);
    document.addEventListener("visibilitychange", visibility);
    void this.client.subscribe((notice) => { if (this.active && lifetime === this.lifetime) this.invalidate(notice); }).then((dispose) => {
      if (!this.active || lifetime !== this.lifetime) { dispose(); return; }
      this.unsubscribe = dispose;
      this.refresh();
    }).catch(() => {
      if (this.active && lifetime === this.lifetime) this.publish({ ...this.state, unavailable: true });
    });
    return () => {
      this.active = false;
      ++this.lifetime;
      clearTimeout(this.timer);
      this.unsubscribe?.();
      this.unsubscribe = undefined;
      window.removeEventListener("focus", reveal);
      document.removeEventListener("visibilitychange", visibility);
    };
  }

  private invalidate(notice: SurfaceNotice) {
    const current = this.state.snapshot;
    let snapshot = current;
    let generation = this.state.generation;
    if (current && notice.kind === "visibility") {
      if (current.openEpoch !== notice.openEpoch || current.visible !== notice.visible) ++generation;
      snapshot = { ...current, visible: notice.visible, openEpoch: notice.openEpoch, observation: null };
    } else if (current && notice.kind === "invalidate" &&
      (notice.contextEpoch !== current.workspace.contextEpoch || notice.openEpoch !== current.openEpoch)) {
      ++generation;
      snapshot = { ...current, observation: null, openEpoch: notice.openEpoch,
        workspace: { ...current.workspace, contextEpoch: notice.contextEpoch } };
    } else if (current && notice.kind === "handoff" && notice.revision > current.handoff.revision) {
      snapshot = { ...current, handoff: { revision: notice.revision, pendingRequestId: notice.requestId } };
    }
    const changedScope = notice.kind === "invalidate" && (!current ||
      notice.contextEpoch !== current.workspace.contextEpoch || notice.openEpoch !== current.openEpoch);
    this.publish({ snapshot, generation, noticeRevision: this.state.noticeRevision + 1, unavailable: false, reconciling: changedScope || this.state.reconciling });
    clearTimeout(this.timer);
    // A single hide reconciliation is allowed; only visible state schedules periodic recovery.
    this.refresh();
  }

  refresh = () => {
    if (!this.active) return;
    this.dirty = true;
    clearTimeout(this.timer);
    if (this.unsubscribe && !this.reading) void this.read();
  };

  acceptOpening(snapshot: ReviewSurfaceSnapshot, generation: number) {
    const current = this.state.snapshot;
    if (!this.active || this.state.generation !== generation || !current?.visible ||
      current.openEpoch !== snapshot.openEpoch || current.workspace.contextEpoch !== snapshot.workspace.contextEpoch) return false;
    // The ticket certifies freshness for this scope even if a rename/presentation/handoff overtook its reply.
    // Merge authorities independently: accepting that certificate must not roll any cached revision back.
    const observation = snapshot.observation && (!current.observation ||
      snapshot.observation.observationRevision >= current.observation.observationRevision) ? snapshot.observation : current.observation;
    const presentation = snapshot.presentation && (!current.presentation ||
      snapshot.presentation.revision >= current.presentation.revision) ? snapshot.presentation : current.presentation;
    const next = { ...current, observation, presentation,
      workspace: snapshot.workspace.revision >= current.workspace.revision ? snapshot.workspace : current.workspace,
      handoff: snapshot.handoff.revision >= current.handoff.revision ? snapshot.handoff : current.handoff };
    this.publish({ ...this.state, snapshot: next, reconciling: false, unavailable: false });
    return true;
  }

  acceptWorkspace(workspace: WorkspaceSnapshot) {
    const current = this.state.snapshot;
    if (!this.active || !current || workspace.revision < current.workspace.revision) return;
    const changed = workspace.contextEpoch !== current.workspace.contextEpoch;
    this.publish({ ...this.state, snapshot: { ...current, workspace, observation: changed ? null : current.observation },
      generation: this.state.generation + (changed ? 1 : 0), reconciling: false });
    this.refresh();
  }

  private async read() {
    this.reading = true;
    const lifetime = this.lifetime;
    try {
      while (this.active && lifetime === this.lifetime && this.dirty) {
        this.dirty = false;
        const generation = this.state.generation;
        const noticeRevision = this.state.noticeRevision;
        try {
          const incoming = await this.client.snapshot();
          if (!this.active || lifetime !== this.lifetime) return;
          if (generation !== this.state.generation || noticeRevision !== this.state.noticeRevision) { this.dirty = true; continue; }
          const old = this.state.snapshot;
          if (old && (incoming.workspace.revision < old.workspace.revision || incoming.handoff.revision < old.handoff.revision)) continue;
          const changed = old && (old.workspace.contextEpoch !== incoming.workspace.contextEpoch || old.openEpoch !== incoming.openEpoch || old.visible !== incoming.visible);
          if (!changed && old?.observation && incoming.observation &&
            incoming.observation.observationRevision < old.observation.observationRevision) continue;
          this.publish({ snapshot: incoming, generation: generation + (changed ? 1 : 0), noticeRevision: this.state.noticeRevision, unavailable: false, reconciling: false });
        } catch {
          if (this.active && lifetime === this.lifetime) this.publish({ ...this.state, unavailable: true });
        }
      }
    } finally {
      this.reading = false;
      if (this.active) {
        if (this.dirty) this.refresh();
        else if (this.state.snapshot?.visible) this.timer = window.setTimeout(this.refresh, 1000);
        else if (!this.state.snapshot && this.bootstrapRetries > 0) {
          --this.bootstrapRetries;
          this.timer = window.setTimeout(this.refresh, 1000);
        }
      }
    }
  }
}

export function useReviewSurface(client: ReviewSurfaceClient | null) {
  const connection = useMemo(() => client ? new ReviewSurfaceConnection(client) : null, [client]);
  const state = useSyncExternalStore(connection?.subscribe ?? subscribeAbsent, connection?.getSnapshot ?? absentSnapshot);
  useEffect(() => connection?.start(), [connection]);
  return { ...state, connection };
}
const absent: SurfaceState = { snapshot: null, generation: 0, noticeRevision: 0, unavailable: false, reconciling: false };
const absentSnapshot = () => absent;
const subscribeAbsent = () => () => undefined;
