/**
 * Coordinates requests and recoverable feedback for the mounted workspace.
 * Rust snapshots are authoritative; revision filtering orders state, while
 * selection generations prevent superseded requests from updating this view.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  RepositoryClient,
  WorkspaceSnapshot,
} from "../../contracts/repositories";
import { useLocale } from "../../i18n";
import type { WorkspaceError } from "./workspaceError";

const emptyWorkspace: WorkspaceSnapshot = {
  revision: 0,
  contextEpoch: "",
  entries: [],
  activeContextId: null,
  restoring: false,
  persistenceError: null,
};

/**
 * Keeps native selections in user-intent order without delaying pending UI feedback.
 * A pending ID is presentation state, not a confirmed host selection.
 */
export function useWorkspace(client: RepositoryClient, enabled = true) {
  const { locale } = useLocale();
  const [snapshot, setSnapshot] = useState<WorkspaceSnapshot>(emptyWorkspace);
  const [loading, setLoading] = useState(true);
  const [opening, setOpening] = useState(false);
  const [mutating, setMutating] = useState(false);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState<WorkspaceError | null>(null);
  const [observationGeneration, setObservationGeneration] = useState(0);
  const selectionGeneration = useRef(0);
  const selectionQueue = useRef<Promise<void>>(Promise.resolve());
  const latestRevision = useRef(0);
  const contextEpoch = useRef("");
  const lifecycle = useRef(0);
  const selectionPending = useRef(false);
  const snapshotQueue = useRef<Promise<void>>(Promise.resolve());
  const snapshotSource = useRef(client);

  const readSnapshot = useCallback((source: RepositoryClient) => {
    // A replacement service must not wait for an abandoned client's pending read.
    if (snapshotSource.current !== source) {
      snapshotSource.current = source;
      snapshotQueue.current = Promise.resolve();
    }
    // Bootstrap, polling and transport recovery share one serialized read boundary.
    const request = snapshotQueue.current.then(() => source.snapshot());
    snapshotQueue.current = request.then(() => undefined, () => undefined);
    return request;
  }, []);

  const applySnapshot = useCallback((incomingSnapshot: WorkspaceSnapshot) => {
    // Replies from independent requests can arrive after newer host state.
    if (incomingSnapshot.revision < latestRevision.current) return;
    latestRevision.current = incomingSnapshot.revision;
    contextEpoch.current = incomingSnapshot.contextEpoch;
    setSnapshot(incomingSnapshot);
  }, []);

  const acceptExternalSnapshot = useCallback((incoming: WorkspaceSnapshot) => {
    if (incoming.revision < latestRevision.current) return;
    if (incoming.contextEpoch !== contextEpoch.current) {
      setObservationGeneration(++selectionGeneration.current);
      selectionPending.current = false;
      setPendingId(null);
    }
    applySnapshot(incoming);
  }, [applySnapshot]);

  useEffect(() => {
    // StrictMode and client replacement must invalidate all prior request completions.
    const requestedLifecycle = ++lifecycle.current;
    const requestedGeneration = ++selectionGeneration.current;
    latestRevision.current = 0;
    selectionPending.current = false;
    selectionQueue.current = Promise.resolve();
    setSnapshot(emptyWorkspace);
    setLoading(true);
    setOpening(false);
    setMutating(false);
    setPendingId(null);
    setError(null);
    setObservationGeneration(requestedGeneration);
    readSnapshot(client)
      .then((initial) => {
        if (
          lifecycle.current === requestedLifecycle &&
          selectionGeneration.current === requestedGeneration &&
          !selectionPending.current
        ) applySnapshot(initial);
      })
      .catch(() => {
        if (
          lifecycle.current === requestedLifecycle &&
          selectionGeneration.current === requestedGeneration
        )
          setError({ domain: "workspace", code: "desktop_unavailable" });
      })
      .finally(() => {
        if (lifecycle.current === requestedLifecycle) setLoading(false);
      });
    return () => {
      if (lifecycle.current === requestedLifecycle) ++lifecycle.current;
    };
  }, [client, applySnapshot, readSnapshot]);

  const needsSnapshotPolling = snapshot.restoring || snapshot.entries.some(
    (entry) => entry.id === snapshot.activeContextId && entry.kind === "unknown",
  );

  useEffect(() => {
    if (loading || !enabled || !needsSnapshotPolling) return;
    let active = true;
    let timer: number | undefined;
    const requestedLifecycle = lifecycle.current;

    async function poll() {
      const requestedGeneration = selectionGeneration.current;
      // A read started during a queued selection must not publish its intermediate state.
      const startedDuringSelection = selectionPending.current;
      try {
        const incoming = await readSnapshot(client);
        if (
          active &&
          lifecycle.current === requestedLifecycle &&
          requestedGeneration === selectionGeneration.current &&
          !startedDuringSelection &&
          !selectionPending.current
        ) {
          applySnapshot(incoming);
          setError((current) => current?.domain === "workspace" && current.code === "restoration_connection" ? null : current);
        }
      } catch {
        if (
          active &&
          lifecycle.current === requestedLifecycle &&
          requestedGeneration === selectionGeneration.current &&
          !startedDuringSelection &&
          !selectionPending.current
        )
          setError({ domain: "workspace", code: "restoration_connection" });
      } finally {
        // Completion pacing bounds native reads even when the desktop service is slow.
        if (active) timer = window.setTimeout(() => void poll(), 1000);
      }
    }

    timer = window.setTimeout(() => void poll(), 1000);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [client, loading, enabled, needsSnapshotPolling, applySnapshot, readSnapshot]);

  async function reconcileOpenAfterSelection(requestedLifecycle: number) {
    // Admission survives newer intents; read only after their queued host transitions settle.
    while (lifecycle.current === requestedLifecycle) {
      await selectionQueue.current.catch(() => undefined);
      if (lifecycle.current !== requestedLifecycle) return;
      if (selectionPending.current) continue;
      const requestedGeneration = selectionGeneration.current;
      const recovered = await readSnapshot(client);
      if (lifecycle.current !== requestedLifecycle) return;
      // An intent that overtakes this read gets a fresh reconciliation after its own completion.
      if (
        selectionGeneration.current !== requestedGeneration ||
        selectionPending.current
      ) continue;
      applySnapshot(recovered);
      return;
    }
  }

  async function openRepository() {
    const requestedLifecycle = lifecycle.current;
    const requestedGeneration = selectionGeneration.current;
    setOpening(true);
    setError(null);
    try {
      const outcome = await client.openChosenRepository(locale);
      if (lifecycle.current !== requestedLifecycle) return;
      if (selectionGeneration.current === requestedGeneration && !selectionPending.current)
        applySnapshot(outcome.snapshot);
      else if (outcome.kind === "opened" || outcome.kind === "reused")
        await reconcileOpenAfterSelection(requestedLifecycle);
      if (outcome.kind === "rejected") setError({ domain: "rejection", code: outcome.code });
    } catch {
      if (lifecycle.current !== requestedLifecycle) return;
      setError({ domain: "workspace", code: "open_connection" });
      // A lost reply may follow a committed host change; reconcile before retrying.
      try {
        await reconcileOpenAfterSelection(requestedLifecycle);
      } catch {
        // Keep the last accepted snapshot and the transport error if recovery also fails.
      }
    } finally {
      if (lifecycle.current === requestedLifecycle) setOpening(false);
    }
  }

  function refreshForSelection(entryId: string, requestedGeneration: number) {
    const requestedLifecycle = lifecycle.current;
    void client
      .refreshEntryAvailability(entryId)
      .then((next) => {
        // Availability belongs to the intent that launched it, not a later selection.
        if (
          lifecycle.current === requestedLifecycle &&
          selectionGeneration.current === requestedGeneration
        )
          applySnapshot(next);
      })
      .catch(() => {
        if (
          lifecycle.current === requestedLifecycle &&
          selectionGeneration.current === requestedGeneration
        )
          setError({ domain: "workspace", code: "check_connection" });
      });
  }

  function selectRepository(entryId: string, worktreeId?: string) {
    const requestedLifecycle = lifecycle.current;
    const requestedGeneration = ++selectionGeneration.current;
    selectionPending.current = true;
    setObservationGeneration(requestedGeneration);
    setPendingId(entryId);
    setError(null);
    // Serialize host mutations in click order, even when earlier IPC replies are slow.
    selectionQueue.current = selectionQueue.current
      // A failed queue item must not prevent the user's next selection.
      .catch(() => undefined)
      .then(async () => {
        if (lifecycle.current !== requestedLifecycle) return;
        try {
          const outcome = worktreeId === undefined
            ? await client.selectContext(entryId)
            : await client.selectWorktree(entryId, worktreeId);
          // Queued older intents still run, but only the latest may update the view.
          if (
            lifecycle.current !== requestedLifecycle ||
            requestedGeneration !== selectionGeneration.current
          ) return;
          applySnapshot(outcome.snapshot);
          selectionPending.current = false;
          setPendingId(null);
          if (outcome.kind === "rejected") {
            setError({ domain: "rejection", code: outcome.code });
            return;
          }
          if (outcome.kind === "not_found") {
            setError({ domain: "workspace", code: "not_found" });
            return;
          }
        } catch {
          if (
            lifecycle.current !== requestedLifecycle ||
            requestedGeneration !== selectionGeneration.current
          ) return;
          selectionPending.current = false;
          setPendingId(null);
          setError({ domain: "workspace", code: "switch_connection" });
          try {
            // Failure does not prove the host transition failed; recover its actual state.
            const recovered = await readSnapshot(client);
            if (
              lifecycle.current === requestedLifecycle &&
              requestedGeneration === selectionGeneration.current
            ) applySnapshot(recovered);
          } catch {
            // Preserve the last accepted snapshot; the switch error remains actionable.
          }
        }
      });
  }

  /**
   * Sidebar mutations share the selection queue: older replies cannot restore a
   * removed context or override a selection made while the mutation was pending.
   */
  function mutateRepository(
    entryId: string,
    displayName?: string,
  ): Promise<WorkspaceError | null> {
    const requestedLifecycle = lifecycle.current;
    const requestedGeneration = ++selectionGeneration.current;
    selectionPending.current = true;
    setMutating(true);
    setError(null);
    const isRemoval = displayName === undefined;
    const currentIntent = () =>
      lifecycle.current === requestedLifecycle &&
      selectionGeneration.current === requestedGeneration;
    const request = selectionQueue.current
      .catch(() => undefined)
      .then(async (): Promise<WorkspaceError | null> => {
        if (lifecycle.current !== requestedLifecycle)
          return { domain: "workspace", code: "connection_changed" };
        try {
          const outcome = isRemoval
            ? await client.removeRepository(entryId)
            : await client.renameRepository(entryId, displayName);
          if (currentIntent()) {
            applySnapshot(outcome.snapshot);
            selectionPending.current = false;
            setPendingId(null);
            if (isRemoval && outcome.snapshot.activeContextId !== snapshot.activeContextId)
              setObservationGeneration(requestedGeneration);
          }
          if (outcome.kind === "rejected") return { domain: "rejection", code: outcome.code };
          if (outcome.kind === "not_found")
            return { domain: "workspace", code: "not_found" };
          return null;
        } catch {
          // The host may have committed before the reply was lost; recover its state.
          try {
            const recovered = await readSnapshot(client);
            if (currentIntent()) {
              applySnapshot(recovered);
              if (isRemoval && recovered.activeContextId !== snapshot.activeContextId)
                setObservationGeneration(requestedGeneration);
            }
          } catch {
            // Preserve the last accepted snapshot and its independent persistence warning.
          }
          return { domain: "workspace", code: isRemoval ? "remove_connection" : "rename_connection" };
        } finally {
          if (lifecycle.current === requestedLifecycle) setMutating(false);
          if (currentIntent()) {
            selectionPending.current = false;
            setPendingId(null);
          }
        }
      });
    selectionQueue.current = request.then(() => undefined);
    return request;
  }

  async function removeRepository(entryId: string) {
    const requestedLifecycle = lifecycle.current;
    const request = mutateRepository(entryId);
    const requestedGeneration = selectionGeneration.current;
    const result = await request;
    if (
      lifecycle.current === requestedLifecycle &&
      selectionGeneration.current === requestedGeneration &&
      result !== null
    )
      setError(result);
  }

  return {
    snapshot,
    acceptExternalSnapshot,
    loading,
    opening,
    mutating,
    pendingId,
    selectionGeneration: observationGeneration,
    error,
    openRepository,
    selectRepository,
    selectWorktree: (entryId: string, worktreeId: string) => selectRepository(entryId, worktreeId),
    renameRepository: (entryId: string, displayName: string) =>
      mutateRepository(entryId, displayName),
    removeRepository,
    refreshAvailability: (entryId: string) =>
      refreshForSelection(entryId, selectionGeneration.current),
    dismissError: () => setError(null),
  };
}
