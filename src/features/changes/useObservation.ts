/** Polls complete cached observations without driving native scans or overlapping IPC reads. */

import { useEffect, useState } from "react";
import type { ObservationSnapshot } from "../../contracts/changes";
import type { RepositoryClient } from "../../contracts/repositories";

/** Transport loss is presentation state, never a fabricated native error or clean result. */
export type ObservationView = ObservationSnapshot | { kind: "transport_unavailable" };
interface ScopedObservation {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  observation: ObservationView;
}

/** Same-ID selections start a new lifecycle; only matching, ordered snapshots may publish. */
export function useObservation(
  client: RepositoryClient,
  entryId: string | null,
  selectionGeneration: number,
  enabled = true,
): ObservationView | null {
  const [current, setCurrent] = useState<ScopedObservation | null>(null);
  const [inFlight] = useState(() => new Map<RepositoryClient, Promise<void>>());

  useEffect(() => {
    if (entryId === null || !enabled) return;
    const observedEntryId = entryId;
    let active = true;
    let timer: number | undefined;
    let latestRevision = -1;
    let accepted: ObservationSnapshot | null = null;
    let disconnected = false;
    const publish = (observation: ObservationView) => {
      if (active) setCurrent({ client, entryId, selectionGeneration, observation });
    };
    publish({ entryId, observationRevision: 0, kind: "checking" });

    async function readSnapshot() {
      try {
        const incoming = await client.observeSelectedContext(observedEntryId);
        if (!active || incoming.entryId !== entryId) return;
        if (incoming.observationRevision > latestRevision) {
          latestRevision = incoming.observationRevision;
          accepted = incoming;
          publish(incoming);
          disconnected = false;
        } else if (incoming.observationRevision === latestRevision && accepted && disconnected) {
          publish(accepted);
          disconnected = false;
        }
      } catch {
        if (!disconnected) publish({ kind: "transport_unavailable" });
        disconnected = true;
      }
    }

    async function poll() {
      // A replacement client must not wait for a disconnected client's abandoned IPC.
      const outstanding = inFlight.get(client);
      if (outstanding) await outstanding;
      if (!active) return;
      const request = readSnapshot();
      inFlight.set(client, request);
      await request;
      if (inFlight.get(client) === request) inFlight.delete(client);
      if (active) timer = window.setTimeout(() => void poll(), 1000);
    }
    void poll();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [client, entryId, selectionGeneration, enabled, inFlight]);

  // Render guards clear the old tree immediately, before effect cleanup runs.
  if (entryId === null || !enabled) return null;
  if (current?.client !== client || current.entryId !== entryId || current.selectionGeneration !== selectionGeneration) {
    return { entryId, observationRevision: 0, kind: "checking" };
  }
  return current.observation;
}
