/** Owns one mounted review's open/refresh lifecycle; native code owns account and cache authority. */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PrFailure, PrSnapshot, PullRequestClient } from "../../contracts/pullRequests";

export type PrReviewFailure = PrFailure | { kind: "transport_unavailable"; code?: never };
interface ReviewScope { client: PullRequestClient; entryId: string; generation: number; prId: string | null }
interface ReviewState { scope: ReviewScope; snapshot: PrSnapshot | null; loading: boolean; error: PrReviewFailure | null }
interface ReviewLifetime { scope: ReviewScope; refresh: () => void }

/** The caller advances generation on native account/context invalidation, clearing presentation synchronously.
 * Close releases native session authority; no renderer account identity authorizes reads.
 * Failed refreshes retain the identified old snapshot only when the failure permits stale presentation.
 */
export function usePullRequest(client: PullRequestClient, entryId: string, generation: number, prId: string | null) {
  const scope = useMemo(() => ({ client, entryId, generation, prId }), [client, entryId, generation, prId]);
  const desired = useRef(scope);
  desired.current = scope;
  const lifetime = useRef<ReviewLifetime | null>(null);
  const [state, setState] = useState<ReviewState | null>(null);

  useEffect(() => {
    let alive = true;
    let pending = false;
    let snapshot: PrSnapshot | null = null;
    let sessionId: string | null = null;
    let released = false;
    const current = () => alive && desired.current === scope;
    const release = (id: string) => {
      if (sessionId === id && released) return;
      if (sessionId === id) released = true;
      void client.release(entryId, id).catch(() => {
        // Native context revocation and bounded session storage remain authoritative if IPC is unavailable.
      });
    };
    const publish = (loading: boolean, error: PrReviewFailure | null) => {
      if (current()) setState({ scope, snapshot, loading, error });
    };
    const read = async () => {
      if (!current() || pending || prId === null) return;
      pending = true;
      publish(true, null);
      let error: PrReviewFailure | null = null;
      try {
        const result = sessionId && !released ? await client.refresh(entryId, sessionId) : await client.open(entryId, prId);
        if (!current()) {
          if (result.kind === "snapshot") release(result.sessionId);
          return;
        }
        if (result.kind === "snapshot") {
          if (result.prId !== prId || (sessionId && !released && (result.sessionId !== sessionId || result.revision < (snapshot?.revision ?? 0)))) {
            if (result.sessionId !== sessionId) release(result.sessionId);
            error = { kind: "error", code: "invalid_output" };
          } else {
            sessionId = result.sessionId;
            released = false;
            snapshot = result;
          }
        } else error = result;
      } catch {
        error = { kind: "transport_unavailable" };
      } finally {
        pending = false;
      }
      if (!current()) return;
      if (error && clearsPrivateContent(error)) {
        snapshot = null;
        if (sessionId) release(sessionId);
      } else if (error && snapshot) {
        snapshot = { ...snapshot, freshness: "stale", availability: error.kind === "transport_unavailable" ? snapshot.availability : error };
      }
      publish(false, error);
    };
    const owner = { scope, refresh: () => { void read(); } };
    lifetime.current = owner;
    void read();
    return () => {
      alive = false;
      if (lifetime.current === owner) lifetime.current = null;
      if (sessionId) release(sessionId);
    };
  }, [scope, client, entryId, prId]);

  const refresh = useCallback(() => {
    if (desired.current === scope && lifetime.current?.scope === scope) lifetime.current.refresh();
  }, [scope]);
  const visible = state?.scope === scope ? state : null;
  return { snapshot: visible?.snapshot ?? null, loading: visible?.loading ?? prId !== null, error: visible?.error ?? null, refresh };
}

function clearsPrivateContent(error: PrReviewFailure): boolean {
  return error.kind !== "transport_unavailable" && ["auth_required", "auth_unavailable", "access_denied", "repository_unavailable", "stale_context"].includes(error.code);
}
