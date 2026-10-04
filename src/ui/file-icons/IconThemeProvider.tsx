/** Owns the browser-local presentation preference shared by working and committed file trees. */
import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { isIconTheme, type IconTheme } from "./iconThemes";
export type { IconTheme } from "./iconThemes";
const storageKey = "gitview.icon-theme";
const IconThemeContext = createContext<{
  theme: IconTheme;
  setTheme: (theme: IconTheme) => void;
  persistenceError: string | null;
} | null>(null);

export function IconThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setPreference] = useState<{ theme: IconTheme; persistenceError: string | null }>(() => {
    try {
      const savedTheme = localStorage.getItem(storageKey);
      return { theme: isIconTheme(savedTheme) ? savedTheme : "classic", persistenceError: null };
    } catch {
      return { theme: "classic", persistenceError: "Icon preference storage is unavailable." };
    }
  });

  function setTheme(theme: IconTheme) {
    let persistenceError: string | null = null;
    try {
      localStorage.setItem(storageKey, theme);
    } catch {
      // The in-memory choice still works; do not imply that it will survive a restart.
      persistenceError = "Icon theme changed for this session only; the preference could not be saved.";
    }
    setPreference({ theme, persistenceError });
  }

  return <IconThemeContext.Provider value={{ ...preference, setTheme }}>{children}</IconThemeContext.Provider>;
}

/** Requires an explicit authoritative icon choice; it never reads or persists a default. */
export function ReadOnlyIconThemeProvider({ theme, persistenceError, children }: {
  theme: IconTheme; persistenceError: boolean; children: ReactNode;
}) {
  const value = useMemo(() => ({ theme, persistenceError: persistenceError ? "session_only" : null,
    setTheme() { throw new Error("Read-only icons cannot change preferences"); } }), [theme, persistenceError]);
  return <IconThemeContext.Provider value={value}>{children}</IconThemeContext.Provider>;
}

export function useIconTheme() {
  return useContext(IconThemeContext);
}
