/** Shares renderer-local presentation choices across toolbar and reviews; native comparison authority is unchanged. */
import { useSyncExternalStore } from "react";
import { isCodeTheme, type CodeTheme } from "./codeThemes";

export type ReviewMode = "changes" | "full";
export type LineMode = "scroll" | "wrap";
interface Choices { mode: ReviewMode; theme: CodeTheme | "match"; lineMode: LineMode }
export interface ReviewChoices extends Choices {
  persistenceError: boolean;
  change: (next: Partial<Choices>) => void;
}
const storageKey = "gitview.code-review";
const listeners = new Set<() => void>();
let current: ReviewChoices | null = null;

function readChoices(): Choices {
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? "null");
    return { mode: saved?.mode === "full" ? "full" : "changes",
      theme: saved?.theme === "match" || isCodeTheme(saved?.theme) ? saved.theme : "match",
      lineMode: saved?.lineMode === "wrap" ? "wrap" : "scroll" };
  } catch { return { mode: "changes", theme: "match", lineMode: "scroll" }; }
}
function emit() { for (const listener of listeners) listener(); }
function change(update: Partial<Choices>) {
  const previous = getSnapshot();
  const next: Choices = { mode: update.mode ?? previous.mode, theme: update.theme ?? previous.theme,
    lineMode: update.lineMode ?? previous.lineMode };
  let persistenceError = false;
  try { localStorage.setItem(storageKey, JSON.stringify(next)); }
  catch { persistenceError = true; }
  // Retain session-only changes even while every review unmounts or storage is inaccessible.
  current = { ...next, persistenceError, change };
  emit();
}
function getSnapshot(): ReviewChoices {
  return current ??= { ...readChoices(), persistenceError: false, change };
}
function onStorage(event: StorageEvent) {
  if (event.key !== storageKey && event.key !== null) return;
  current = { ...readChoices(), persistenceError: false, change };
  emit();
}
function subscribe(listener: () => void) {
  if (listeners.size === 0) window.addEventListener("storage", onStorage);
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) window.removeEventListener("storage", onStorage);
  };
}
export function useReviewChoices(): ReviewChoices {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
