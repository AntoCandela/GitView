/** Owns compact reading intent; native open/context epochs alone authorize current review. */
import { useEffect, useRef, useState } from "react";
import type { CompanionClient, CompanionCode, HandoffSelection, ReviewSurfaceClient, ReviewSurfaceSnapshot } from "../../contracts/companion";
import type { ReviewSelection } from "../../contracts/diff";
import type { ObservationSnapshot } from "../../contracts/changes";
import { hasReviewCategory, type LiveReviewAuthority } from "../../features/diff";
import { useReviewSurface } from "./useReviewSurface";


export interface CompanionReview {
  snapshot: ReviewSurfaceSnapshot | null;
  visible: boolean;
  ready: boolean;
  observation: ObservationSnapshot | null;
  selection: ReviewSelection | null;
  generation: number;
  selecting: boolean;
  handingOff: boolean;
  error: CompanionCode | null;
  errorSource: "context" | "handoff" | "surface";
  selectFile(selection: ReviewSelection): void;
  recordAuthority(authority: LiveReviewAuthority): void;
  selectContext(entryId: string): void;
  openInGitView(): Promise<void>;
  lifecycleAction(action: "dismiss" | "quit"): Promise<void>;
}
export function useCompanionReview(client: CompanionClient, surfaceClient: ReviewSurfaceClient): CompanionReview {
  const surface = useReviewSurface(surfaceClient);
  const snapshot = surface.snapshot;
  const [opened, setOpened] = useState<{ generation: number; snapshot: ReviewSurfaceSnapshot } | null>(null);
  const [openingError, setOpeningError] = useState<CompanionCode | null>(null);
  const [openingAttempt, setOpeningAttempt] = useState(0);
  const failedObservationRevision = useRef(-1);
  const [actionError, setActionError] = useState<CompanionCode | null>(null);
  const [errorSource, setErrorSource] = useState<CompanionReview["errorSource"]>("surface");
  const [choice, setChoice] = useState<{ contextEpoch: string; selection: ReviewSelection; provenance: HandoffSelection | null } | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [handingOff, setHandingOff] = useState(false);
  const selectionQueue = useRef<Promise<void>>(Promise.resolve());
  const selectionIntent = useRef(0);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; ++selectionIntent.current; }; }, []);
  const visible = snapshot?.visible ?? false;
  const contextEpoch = snapshot?.workspace.contextEpoch;
  const openEpoch = snapshot?.openEpoch;
  const generation = surface.generation;

  useEffect(() => {
    if (!visible || !openEpoch) return;
    let active = true;
    // Recovery can overtake this reply; fence against the attempted observation, not its successor.
    const attemptedObservationRevision = surface.connection?.getSnapshot().snapshot?.observation?.observationRevision ?? -1;
    setOpeningError(null);
    setActionError(null);
    setErrorSource("surface");
    void client.begin(openEpoch).then((result) => {
      const current = surface.connection?.getSnapshot();
      if (!active || !current?.snapshot?.visible || current.generation !== generation) return;
      if (result.kind !== "ready") {
        failedObservationRevision.current = attemptedObservationRevision;
        setOpeningError(result.code);
        return;
      }
      if (result.surface.openEpoch !== openEpoch || result.surface.workspace.contextEpoch !== current.snapshot.workspace.contextEpoch) {
        surface.connection?.refresh();
        return;
      }
      if (!surface.connection?.acceptOpening(result.surface, generation)) return;
      setOpened({ generation, snapshot: result.surface });
      surface.connection?.refresh();
    }).catch(() => {
      if (active) {
        failedObservationRevision.current = attemptedObservationRevision;
        setOpeningError("unavailable");
      }
    });
    return () => { active = false; };
  }, [client, surface.connection, visible, openEpoch, contextEpoch, generation, openingAttempt]);

  const observedKind = snapshot?.observation?.kind;
  const observedRevision = snapshot?.observation?.observationRevision ?? -1;
  useEffect(() => {
    if (visible && openingError && (observedKind === "ready" || observedKind === "bare") &&
      observedRevision > failedObservationRevision.current) {
      failedObservationRevision.current = observedRevision;
      setOpeningAttempt((attempt) => attempt + 1);
    }
  }, [visible, openingError, observedKind, observedRevision]);

  const ready = !!(visible && !selecting && opened?.generation === generation &&
    opened.snapshot.openEpoch === openEpoch && opened.snapshot.workspace.contextEpoch === contextEpoch);
  const current = snapshot;
  const observation = ready ? current?.observation ?? null : null;
  const selection = choice && choice.contextEpoch === contextEpoch ? choice.selection : null;
  const entryId = current?.workspace.activeContextId ?? null;

  function selectFile(selection: ReviewSelection) {
    if (!ready || !contextEpoch || !entryId) return;
    const file = observation?.kind === "ready" ? observation.files.find((file) => file.stablePathId === selection.stablePathId) : null;
    setChoice({ contextEpoch, selection, provenance: file && observation && hasReviewCategory(file, selection) ? {
      entryId, stablePathId: file.stablePathId, pathId: file.pathId, observationRevision: observation.observationRevision, category: selection.category,
    } : choice?.contextEpoch === contextEpoch && choice.selection.stablePathId === selection.stablePathId && choice.selection.category === selection.category ? choice.provenance : null });
  }

  function recordAuthority(authority: LiveReviewAuthority) {
    setChoice((current) => {
      if (!current || current.contextEpoch !== contextEpoch || current.selection.stablePathId !== authority.stablePathId ||
        current.selection.category !== authority.category || authority.entryId !== entryId) return current;
      if (current.provenance?.observationRevision === authority.observationRevision && current.provenance.pathId === authority.pathId) return current;
      return { ...current, provenance: authority };
    });
  }

  function selectContext(entryId: string) {
    const intent = ++selectionIntent.current;
    setSelecting(true);
    setActionError(null);
    setErrorSource("context");
    selectionQueue.current = selectionQueue.current.catch(() => undefined).then(async () => {
      if (!mounted.current) return;
      try {
        const result = await client.selectContext(entryId);
        if (mounted.current) surface.connection?.acceptWorkspace(result.snapshot);
        if (mounted.current && intent === selectionIntent.current && result.kind !== "selected") setActionError("unavailable");
      } catch {
        if (mounted.current && intent === selectionIntent.current) setActionError("unavailable");
      } finally {
        if (mounted.current && intent === selectionIntent.current) { setSelecting(false); surface.connection?.refresh(); }
      }
    });
  }

  async function openInGitView() {
    if (!snapshot || !visible || handingOff) return;
    setHandingOff(true);
    setActionError(null);
    setErrorSource("handoff");
    const requestedGeneration = generation;
    const file = observation?.kind === "ready" && selection ? observation.files.find((file) => file.stablePathId === selection.stablePathId) : null;
    const categoryPresent = file && selection && hasReviewCategory(file, selection);
    const provenance = categoryPresent && file && selection && entryId && observation ? {
      entryId, stablePathId: file.stablePathId, pathId: file.pathId, observationRevision: observation.observationRevision, category: selection.category,
    } : selection ? choice?.provenance ?? null : null;
    try {
      const result = await client.requestHandoff({ openEpoch: snapshot.openEpoch, contextEpoch: snapshot.workspace.contextEpoch, selection: provenance });
      if (mounted.current && surface.connection?.getSnapshot().generation === requestedGeneration && result.kind === "failed") setActionError(result.code);
    } catch {
      if (mounted.current && surface.connection?.getSnapshot().generation === requestedGeneration) setActionError("unavailable");
    } finally {
      if (mounted.current) setHandingOff(false);
    }
  }

  async function lifecycleAction(action: "dismiss" | "quit") {
    setErrorSource("surface");
    try { await client[action](); }
    catch { if (mounted.current) setActionError("unavailable"); }
  }
  return { snapshot: current, visible, ready, observation, selection, generation, selecting, handingOff, errorSource,
    error: actionError ?? openingError ?? (surface.unavailable ? "unavailable" : null),
    selectFile, recordAuthority, selectContext, openInGitView, lifecycleAction };
}
