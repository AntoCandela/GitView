/** Reconciles native context and claim-authorized handoffs into the existing mounted workspace. */
import { useEffect, useRef } from "react";
import { flushSync } from "react-dom";
import type { ReviewHandoffClient, ReviewHandoffTarget } from "../../contracts/companion";
import type { WorkspaceSnapshot } from "../../contracts/repositories";
import { ReviewHandoffReceiver } from "./ReviewHandoffReceiver";
import type { ReviewSurfaceConnection } from "./useReviewSurface";

export interface AppliedReviewHandoff {
  requestId: string;
  contextEpoch: string;
  target: ReviewHandoffTarget;
  observationRevision: number;
}

export function useMainReviewSurface(connection: ReviewSurfaceConnection | null, handoffClient: ReviewHandoffClient,
  acceptWorkspace: (snapshot: WorkspaceSnapshot) => void, applyHandoff: (handoff: AppliedReviewHandoff) => void) {
  const callbacks = useRef({ acceptWorkspace, applyHandoff });
  callbacks.current = { acceptWorkspace, applyHandoff };
  useEffect(() => {
    if (!connection) return;
    const receiver = new ReviewHandoffReceiver(handoffClient, (target, contextEpoch, requestId) => {
      const current = connection.getSnapshot();
      const snapshot = current.snapshot;
      if (!snapshot || current.reconciling || snapshot.workspace.contextEpoch !== contextEpoch ||
        (target && target.entryId !== snapshot.workspace.activeContextId)) throw new Error("stale_context");
      flushSync(() => {
        callbacks.current.acceptWorkspace(snapshot.workspace);
        callbacks.current.applyHandoff({ requestId, contextEpoch, target, observationRevision: snapshot.observation?.observationRevision ?? -1 });
      });
    });
    const reconcile = () => {
      const current = connection.getSnapshot();
      if (!current.snapshot) return;
      if (!current.reconciling) callbacks.current.acceptWorkspace(current.snapshot.workspace);
      receiver.reconcile(current.snapshot.workspace.contextEpoch, current.snapshot.handoff.revision,
        current.snapshot.handoff.pendingRequestId, !current.reconciling);
    };
    const unsubscribe = connection.subscribe(reconcile);
    reconcile();
    return () => { unsubscribe(); receiver.dispose(); };
  }, [connection, handoffClient]);
}
