/** Owns explicit ancestry reads and pinned pagination; superseded scopes and requests cannot publish. */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { HistoryPage } from "../../contracts/history";
import type { RepositoryClient } from "../../contracts/repositories";
import type { HistoryReadFailure } from "./historyMessages";

interface HistoryScope {
  client: RepositoryClient;
  entryId: string;
  selectionGeneration: number;
  branch: string | null;
}

export type HistoryFailure = HistoryReadFailure | { kind: "transport_unavailable" };
interface HistoryState {
  scope: HistoryScope;
  page: HistoryPage | null;
  loading: "initial" | "more" | null;
  error: HistoryFailure | null;
}

/** Null-cursor refresh replaces the snapshot; continuation retains the first page's refs and HEAD. */
export function useHistory(client: RepositoryClient, entryId: string, selectionGeneration: number, branch: string | null = null) {
  const scope = useMemo(() => ({ client, entryId, selectionGeneration, branch }), [client, entryId, selectionGeneration, branch]);
  const desired = useRef(scope);
  desired.current = scope;
  const mounted = useRef(false);
  const pending = useRef<{ scope: HistoryScope } | null>(null);
  const [state, setState] = useState<HistoryState | null>(null);
  const current = useRef(state);
  current.current = state;

  const request = useCallback((previous: HistoryPage | null) => {
    const operation = { scope };
    pending.current = operation;
    const next: HistoryState = { scope, page: previous, loading: previous ? "more" : "initial", error: null };
    current.current = next;
    setState(next);
    void (async () => {
      let page = previous;
      let error: HistoryFailure | null = null;
      try {
        const result = branch === null
          ? await client.historyPage(entryId, previous?.cursor ?? null)
          : await client.historyPage(entryId, previous?.cursor ?? null, branch);
        if (result.kind !== "page") {
          error = { kind: result.kind, code: result.code };
        } else if (result.page.entryId !== entryId) {
          error = { kind: "error", code: "invalid_output" };
        } else if (previous) {
          const known = new Set(previous.commits.map((commit) => commit.oid));
          page = {
            ...previous,
            commits: [...previous.commits, ...result.page.commits.filter((commit) => {
              if (known.has(commit.oid)) return false;
              known.add(commit.oid);
              return true;
            })],
            cursor: result.page.cursor,
            hasMore: result.page.hasMore,
            completeness: previous.completeness === "shallow_or_missing" ? previous.completeness : result.page.completeness,
          };
        } else {
          page = result.page;
        }
      } catch {
        error = { kind: "transport_unavailable" };
      }
      // The render-time scope guard covers the interval before effect cleanup, including same-entry reselection.
      if (!mounted.current || desired.current !== scope || pending.current !== operation) return;
      pending.current = null;
      const completed: HistoryState = { scope, page, loading: null, error };
      current.current = completed;
      setState(completed);
    })();
  }, [client, entryId, branch, scope]);

  useEffect(() => {
    mounted.current = true;
    if (pending.current?.scope !== scope && current.current?.scope !== scope) request(null);
    return () => { mounted.current = false; };
  }, [scope, request]);

  const refresh = useCallback(() => {
    if (desired.current === scope && mounted.current) request(null);
  }, [scope, request]);
  const loadMore = useCallback(() => {
    const active = current.current;
    if (desired.current !== scope || !mounted.current || pending.current?.scope === scope
      || active?.scope !== scope || !active.page?.hasMore || active.page.cursor === null) return;
    request(active.page);
  }, [scope, request]);

  const visible = state?.scope === scope ? state : null;
  return {
    page: visible?.page ?? null,
    loading: visible ? visible.loading : "initial" as const,
    error: visible?.error ?? null,
    refresh,
    loadMore,
  };
}
