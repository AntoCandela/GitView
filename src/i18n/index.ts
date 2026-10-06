/** Exposes one locale snapshot to app, features and generic UI without platform dependencies. */
import { createContext, createElement, useCallback, useContext, useLayoutEffect, useMemo, useSyncExternalStore, type ReactNode } from "react";
import { createLocaleStore, type LocaleStore } from "./preference";
import { translate, type MessageKey, type MessageParams } from "./messages";
import type { Locale, LocaleChoice, LocaleSnapshot, PreferredLanguageSource } from "./locale";

export { locales, resolveLocale, type Locale, type LocaleChoice, type LocaleSnapshot, type PreferredLanguageSource } from "./locale";
export { translate, formatNumber, type MessageKey, type MessageParams } from "./messages";
export { createLocaleStore, type LocaleStore, type LocaleStoreOptions } from "./preference";

let store = createLocaleStore({
  storage: () => window.localStorage,
  document: typeof document === "undefined" ? undefined : document,
  subscribeStorage(listener) {
    const onStorage = (event: StorageEvent) => listener(event);
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  },
});

const ReadOnlyLocaleContext = createContext<LocaleSnapshot | null>(null);
const noSubscription = () => () => {};

/** Uses only an authoritative host snapshot; it never initializes or subscribes to browser storage. */
export function ReadOnlyLocaleProvider({ locale, persistenceError, children }: {
  locale: Locale; persistenceError: boolean; children: ReactNode;
}) {
  const snapshot = useMemo<LocaleSnapshot>(() => ({ choice: locale, locale, pendingChoice: null, persistenceError }), [locale, persistenceError]);
  useLayoutEffect(() => {
    document.documentElement.lang = locale;
    document.title = translate(locale, "app.documentTitle");
    document.querySelector('meta[name="description"]')?.setAttribute("content", translate(locale, "app.documentDescription"));
  }, [locale]);
  return createElement(ReadOnlyLocaleContext.Provider, { value: snapshot }, children);
}

/** Bootstrap awaits this bounded operation before mounting the workspace. */
export function initializeLocale(source: PreferredLanguageSource): Promise<void> {
  return store.initialize(source);
}

export function setLocaleChoice(choice: LocaleChoice): Promise<void> {
  return store.setChoice(choice);
}

export function getLocaleSnapshot() {
  return store.getSnapshot();
}

export function useLocale() {
  const readOnly = useContext(ReadOnlyLocaleContext);
  return useSyncExternalStore(readOnly ? noSubscription : store.subscribe,
    readOnly ? () => readOnly : store.getSnapshot, readOnly ? () => readOnly : store.getSnapshot);
}

/** The initial default is not publishable until locale discovery or explicit selection finishes. */
export function useLocaleReady() {
  const readOnly = useContext(ReadOnlyLocaleContext);
  return useSyncExternalStore(readOnly ? noSubscription : store.subscribe,
    readOnly ? () => true : store.isReady, readOnly ? () => true : store.isReady);
}

export function useTranslation() {
  const { locale } = useLocale();
  const t = useCallback((key: MessageKey, params?: MessageParams) => translate(locale, key, params), [locale]);
  return { locale, t };
}

/** Install before mounting test consumers; restore after unmounting to isolate all browser ports. */
export function installLocaleStoreForTests(isolatedStore: LocaleStore): () => void {
  const previous = store;
  store = isolatedStore;
  return () => {
    isolatedStore.dispose();
    store = previous;
  };
}
