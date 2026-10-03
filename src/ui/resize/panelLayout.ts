/** Keeps the diff proportion across file remounts and broadcasts non-destructive layout resets. */
import { useSyncExternalStore } from "react";

const listeners = new Set<() => void>();
let current = { diffRatio: 0.5, resetVersion: 0 };
function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
const getSnapshot = () => current;

/** The old-side fraction is session-only; it is not tied to a repository or selected file. */
export function setDiffRatio(diffRatio: number) {
  if (current.diffRatio === diffRatio) return;
  current = { ...current, diffRatio };
  for (const listener of listeners) listener();
}

/** Resets sizing without remounting source panes, file trees or the history graph. */
export function resetPanelLayout() {
  current = { diffRatio: 0.5, resetVersion: current.resetVersion + 1 };
  for (const listener of listeners) listener();
}

export function usePanelLayout() {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
