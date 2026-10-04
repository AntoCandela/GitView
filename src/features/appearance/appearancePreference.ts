/** Shares the browser-local interface palette across toolbar and both review surfaces. */
import { createContext, createElement, useContext, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { isAppearanceTheme, type AppearanceTheme } from "./appearanceThemes";

const storageKey = "gitview.app-theme";
const listeners = new Set<() => void>();
interface AppearanceChoice {
  theme: AppearanceTheme;
  persistenceError: boolean;
  setTheme: (theme: AppearanceTheme) => void;
}
let current: AppearanceChoice | null = null;
const ReadOnlyAppearanceContext = createContext<AppearanceChoice | null>(null);
const noSubscription = () => () => {};

/** Compact consumers receive host choices without consulting or writing browser preferences. */
export function ReadOnlyAppearanceProvider({ theme, persistenceError, children }: {
  theme: AppearanceTheme; persistenceError: boolean; children: ReactNode;
}) {
  const value = useMemo<AppearanceChoice>(() => ({ theme, persistenceError,
    setTheme() { throw new Error("Read-only appearance cannot change preferences"); } }), [theme, persistenceError]);
  return createElement(ReadOnlyAppearanceContext.Provider, { value }, children);
}

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
  const readOnly = useContext(ReadOnlyAppearanceContext);
  return useSyncExternalStore(readOnly ? noSubscription : subscribe,
    readOnly ? () => readOnly : getSnapshot, readOnly ? () => readOnly : getSnapshot);
}
