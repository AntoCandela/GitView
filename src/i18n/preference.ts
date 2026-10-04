/** Owns locale intents and browser persistence; native preference discovery is an injected port. */
import { isLocale, resolveLocale, type LocaleChoice, type LocaleSnapshot, type PreferredLanguageSource } from "./locale";
import { translate } from "./messages";

export const localeStorageKey = "gitview.locale";
const sourceTimeoutMs = 2_000;

type LocaleStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
export interface LocaleStorageEvent {
  key: string | null;
  newValue: string | null;
  storageArea?: LocaleStorage | null;
}
export interface LocaleStoreOptions {
  storage: () => LocaleStorage;
  document?: Pick<Document, "documentElement" | "title" | "querySelector">;
  subscribeStorage?: (listener: (event: LocaleStorageEvent) => void) => () => void;
  sourceTimeoutMs?: number;
}
export interface LocaleStore {
  getSnapshot: () => LocaleSnapshot;
  subscribe: (listener: () => void) => () => void;
  initialize: (source: PreferredLanguageSource) => Promise<void>;
  setChoice: (choice: LocaleChoice) => Promise<void>;
  dispose: () => void;
}

function parseChoice(saved: string | null): LocaleChoice | null {
  if (saved === null) return "system";
  try {
    const value: unknown = JSON.parse(saved);
    if (typeof value !== "object" || value === null || !("version" in value) || value.version !== 1 || !("choice" in value)) return null;
    return value.choice === "system" || isLocale(value.choice) ? value.choice : null;
  } catch {
    return null;
  }
}

async function preferredLocale(source: PreferredLanguageSource, timeoutMs: number) {
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    const languages = await Promise.race([
      Promise.resolve().then(source),
      new Promise<readonly string[]>((resolve) => { timeout = setTimeout(() => resolve([]), timeoutMs); }),
    ]);
    return resolveLocale(Array.isArray(languages) ? languages : []);
  } catch {
    // Native availability is independent of preference persistence and uses the ordinary fallback.
    return "en-US" as const;
  } finally {
    clearTimeout(timeout);
  }
}

/** Each instance owns its listeners, intent generation and injected browser state, including in tests. */
export function createLocaleStore(options: LocaleStoreOptions): LocaleStore {
  const listeners = new Set<() => void>();
  let current: LocaleSnapshot = { choice: "system", locale: "en-US", pendingChoice: null, persistenceError: false };
  let generation = 0;
  let source: PreferredLanguageSource = async () => [];
  let unsubscribeStorage: (() => void) | undefined;
  let disposed = false;

  function publish(snapshot: LocaleSnapshot) {
    current = snapshot;
    if (options.document) {
      options.document.documentElement.lang = snapshot.locale;
      options.document.title = translate(snapshot.locale, "app.documentTitle");
      options.document.querySelector('meta[name="description"]')?.setAttribute("content", translate(snapshot.locale, "app.documentDescription"));
    }
    for (const listener of listeners) listener();
  }

  function persist(choice: LocaleChoice, intent: number) {
    if (disposed || intent !== generation) return;
    let persistenceError = false;
    try {
      if (choice === "system") options.storage().removeItem(localeStorageKey);
      else options.storage().setItem(localeStorageKey, JSON.stringify({ version: 1, choice }));
    } catch {
      persistenceError = true;
    }
    if (!disposed && intent === generation && current.persistenceError !== persistenceError) publish({ ...current, persistenceError });
  }

  async function applySystem(intent: number, save: boolean, persistenceError: boolean) {
    publish({ ...current, pendingChoice: "system" });
    const locale = await preferredLocale(source, options.sourceTimeoutMs ?? sourceTimeoutMs);
    // Both failed and successful obsolete requests are forbidden from writing any observable state.
    if (disposed || intent !== generation) return;
    publish({ choice: "system", locale, pendingChoice: null, persistenceError });
    if (save) persist("system", intent);
  }

  function onStorage(event: LocaleStorageEvent) {
    if (disposed || (event.key !== localeStorageKey && event.key !== null)) return;
    if (event.storageArea) {
      try {
        if (event.storageArea !== options.storage()) return;
      } catch {
        publish({ ...current, persistenceError: true });
        return;
      }
    }
    const choice = parseChoice(event.key === null ? null : event.newValue);
    if (choice === null) return;
    const intent = ++generation;
    if (choice === "system") void applySystem(intent, false, false);
    else publish({ choice, locale: choice, pendingChoice: null, persistenceError: false });
  }

  return {
    getSnapshot: () => current,
    subscribe(listener) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    async initialize(preferredLanguages) {
      if (disposed) return;
      source = preferredLanguages;
      const intent = ++generation;
      if (!unsubscribeStorage && options.subscribeStorage) unsubscribeStorage = options.subscribeStorage(onStorage);
      let choice: LocaleChoice = "system";
      let persistenceError = false;
      try { choice = parseChoice(options.storage().getItem(localeStorageKey)) ?? "system"; }
      catch { persistenceError = true; }
      if (choice === "system") await applySystem(intent, false, persistenceError);
      else publish({ choice, locale: choice, pendingChoice: null, persistenceError });
    },
    async setChoice(choice) {
      if (disposed) return;
      const intent = ++generation;
      if (choice === "system") {
        await applySystem(intent, true, current.persistenceError);
      } else {
        publish({ choice, locale: choice, pendingChoice: null, persistenceError: current.persistenceError });
        persist(choice, intent);
      }
    },
    dispose() {
      disposed = true;
      ++generation;
      unsubscribeStorage?.();
      listeners.clear();
    },
  };
}
