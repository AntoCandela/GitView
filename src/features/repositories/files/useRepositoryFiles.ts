/** Replaces native listings serially while retaining one readable generation until expanded branches catch up. */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RepositoryClient } from "../../../contracts/repositories";
import type { RepositoryDirectory, RepositoryFile, RepositoryFilesRequest } from "../../../contracts/browsing";

interface DirectoryProgress {
  directory: RepositoryDirectory | null;
  cursor: string | null;
  complete: boolean;
}
interface ListingView {
  listingId: string | null;
  files: RepositoryFile[];
  directories: RepositoryDirectory[];
  complete: boolean;
  loading: boolean;
  error: string | null;
}
interface ListingSession {
  client: RepositoryClient;
  entryId: string;
  view: ListingView;
  retaining: boolean;
  listingId: string | null;
  files: Map<string, RepositoryFile>;
  directories: Map<string | null, DirectoryProgress>;
  expanded: Set<string>;
  busy: boolean;
  disposed: boolean;
  error: string | null;
  timer: number | undefined;
}
const emptyView: ListingView = { listingId: null, files: [], directories: [], complete: false, loading: false, error: null };

/** Expansion follows native segments across listing IDs; only native directory IDs authorize requests. */
function nextDirectory(current: ListingSession) {
  for (const entry of current.directories) {
    const progress = entry[1];
    if (!progress.complete && (progress.directory === null || current.expanded.has(JSON.stringify(progress.directory.segments)))) return entry;
  }
  return undefined;
}

export function useRepositoryFiles(client: RepositoryClient, entryId: string, enabled: boolean, revision: number | null) {
  const [attempt, setAttempt] = useState(0);
  const scope = useMemo(() => ({ client, entryId, revision, attempt }), [client, entryId, revision, attempt]);
  const desired = useRef(scope);
  desired.current = scope;
  const visible = useRef(enabled);
  visible.current = enabled;
  const session = useRef<ListingSession | null>(null);
  const [state, setState] = useState<{ scope: typeof scope; view: ListingView } | null>(null);

  const publish = useCallback((current: ListingSession, contentsChanged = false) => {
    if (current.disposed || desired.current !== scope) return;
    // Keep one displayed generation, never a historical union. Collapsed branches do not delay cutover.
    if (current.retaining && (current.listingId === null || nextDirectory(current))) {
      current.view = { ...current.view, complete: false, loading: current.busy, error: current.error };
    } else if (!contentsChanged && !current.retaining) {
      current.view = { ...current.view, loading: current.busy, error: current.error };
    } else {
      const progress = [...current.directories.values()];
      current.retaining = false;
      current.view = {
        listingId: current.listingId,
        files: [...current.files.values()],
        directories: progress.flatMap((item) => item.directory ? [item.directory] : []),
        complete: progress.every((item) => item.complete),
        loading: current.busy,
        error: current.error,
      };
    }
    setState({ scope, view: current.view });
  }, [scope]);

  const pump = useCallback(function schedule() {
    const current = session.current;
    if (!current || current.disposed || desired.current !== scope || !visible.current
      || current.busy || current.timer !== undefined || current.error) return;
    const next = nextDirectory(current);
    if (!next) return;
    current.timer = window.setTimeout(() => {
      current.timer = undefined;
      if (current.disposed || desired.current !== scope || !visible.current) return;
      const [directoryId, progress] = next;
      if (progress.directory && !current.expanded.has(JSON.stringify(progress.directory.segments))) { schedule(); return; }
      const request: RepositoryFilesRequest = { listingId: current.listingId, directoryId, cursor: progress.cursor };
      current.busy = true;
      publish(current);
      void client.listRepositoryFiles(entryId, request).then((result) => {
        if (current.disposed || desired.current !== scope) return;
        if (result.kind === "stale_selection") {
          current.error = "Repository listing expired or selection changed. Refresh the file tree.";
        } else if (result.kind === "unavailable") {
          current.error = result.message;
        } else if (result.entryId !== entryId || result.directoryId !== directoryId
          || (current.listingId !== null && result.listingId !== current.listingId)) {
          current.error = "The repository listing could not be verified.";
        } else {
          current.listingId = result.listingId;
          for (const file of result.files) current.files.set(file.id, file);
          for (const directory of result.directories) {
            if (!current.directories.has(directory.id)) {
              current.directories.set(directory.id, { directory, cursor: null, complete: false });
            }
          }
          progress.cursor = result.cursor;
          progress.complete = result.cursor === null;
        }
      }).catch(() => {
        if (!current.disposed && desired.current === scope) current.error = "Desktop connection interrupted. Refresh the file tree.";
      }).finally(() => {
        current.busy = false;
        if (current.disposed || desired.current !== scope) return;
        publish(current, true);
        schedule();
      });
    }, 16);
  }, [client, entryId, scope, publish]);

  useEffect(() => {
    const previous = session.current;
    const sameRepository = previous?.client === client && previous.entryId === entryId;
    const current: ListingSession = {
      client, entryId,
      view: sameRepository ? previous.view : emptyView,
      retaining: sameRepository && previous.view.listingId !== null,
      listingId: null, files: new Map(),
      directories: new Map([[null, { directory: null, cursor: null, complete: false }]]),
      expanded: sameRepository ? previous.expanded : new Set(), busy: false, disposed: false, error: null, timer: undefined,
    };
    session.current = current;
    publish(current);
    pump();
    return () => {
      current.disposed = true;
      clearTimeout(current.timer);
    };
  }, [client, entryId, scope, publish, pump]);

  useEffect(() => { if (enabled) pump(); }, [enabled, pump]);

  const expandDirectories = useCallback((directories: RepositoryDirectory[]) => {
    const current = session.current;
    if (!current || current.disposed || desired.current !== scope) return;
    current.expanded = new Set(directories.map((directory) => JSON.stringify(directory.segments)));
    if (current.retaining) publish(current, true);
    pump();
  }, [scope, publish, pump]);

  const refresh = useCallback(() => setAttempt((value) => value + 1), []);
  const sameRepository = state?.scope.client === client && state.scope.entryId === entryId;
  const view = state?.scope === scope ? state.view
    : sameRepository ? { ...state.view, complete: false, error: null } : emptyView;
  return { ...view, expandDirectories, refresh };
}
