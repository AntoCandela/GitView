/** Shares the browser-local interface palette across toolbar and both review surfaces. */
import { useSyncExternalStore } from "react";
import { isAppearanceTheme, type AppearanceTheme } from "./appearanceThemes";

const storageKey = "gitview.app-theme";
const listeners = new Set<() => void>();
interface AppearanceChoice {
  theme: AppearanceTheme;
  persistenceError: boolean;
  setTheme: (theme: AppearanceTheme) => void;
}
let current: AppearanceChoice | null = null;

function readTheme(): AppearanceTheme {
  try {
    const saved = localStorage.getItem(storageKey);
    return isAppearanceTheme(saved) ? saved : "cream";
  } catch { return "cream"; }
}
function emit() { for (const listener of listeners) listener(); }
function setTheme(theme: AppearanceTheme) {
  let persistenceError = false;
  try { localStorage.setItem(storageKey, theme); }
  catch { persistenceError = true; }
  current = { theme, persistenceError, setTheme };
  emit();
}
function getSnapshot(): AppearanceChoice {
  return current ??= { theme: readTheme(), persistenceError: false, setTheme };
}
function onStorage(event: StorageEvent) {
  if (event.key !== storageKey && event.key !== null) return;
  current = { theme: readTheme(), persistenceError: false, setTheme };
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
export function useAppearanceTheme(): AppearanceChoice {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}
