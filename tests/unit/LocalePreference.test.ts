/** Exercises ordered locale resolution and the real preference store with isolated browser ports. */
import { afterEach, describe, expect, test, vi } from "vitest";
import { createLocaleStore, localeStorageKey, type LocaleStorageEvent, type LocaleStore } from "../../src/i18n/preference";
import { resolveLocale } from "../../src/i18n/locale";
import { catalogs, createTranslator, formatNumber, translate } from "../../src/i18n/messages";
import { deferred } from "../support/deferred";

const stores: LocaleStore[] = [];
afterEach(() => {
  for (const store of stores.splice(0)) store.dispose();
  vi.useRealTimers();
});

function fixture(saved: string | null = null, sourceTimeoutMs = 100) {
  const values = new Map<string, string>();
  if (saved !== null) values.set(localeStorageKey, saved);
  const storage = {
    failRead: false,
    failWrite: false,
    writes: 0,
    getItem(key: string) {
      if (this.failRead) throw new Error("Storage unavailable");
      return values.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      this.writes++;
      if (this.failWrite) throw new Error("Storage unavailable");
      values.set(key, value);
    },
    removeItem(key: string) {
      this.writes++;
      if (this.failWrite) throw new Error("Storage unavailable");
      values.delete(key);
    },
  };
  const document = window.document.implementation.createHTMLDocument();
  const description = document.createElement("meta");
  description.name = "description";
  document.head.append(description);
  let onStorage: ((event: LocaleStorageEvent) => void) | undefined;
  const store = createLocaleStore({
    storage: () => storage,
    document,
    sourceTimeoutMs,
    subscribeStorage(listener) {
      onStorage = listener;
      return () => { onStorage = undefined; };
    },
  });
  stores.push(store);
  return {
    store, storage, values, document, description,
    external(choice: string | null, key: string | null = localeStorageKey) {
      if (choice === null) values.delete(localeStorageKey);
      else values.set(localeStorageKey, choice);
      onStorage?.({ key, newValue: choice });
    },
  };
}

function savedChoice(choice: string) { return JSON.stringify({ version: 1, choice }); }

describe("ordered system locale matching", () => {
  test.each([
    [["it-IT"], "it"],
    [["es-MX"], "es"],
    [["pt"], "pt-BR"],
    [["pt-AO"], "pt-BR"],
    [["pT-pT"], "pt-PT"],
    [["pt-BR"], "pt-BR"],
    [["en"], "en-US"],
    [["en-AU"], "en-US"],
    [["EN-gb"], "en-GB"],
    [["en-US"], "en-US"],
    [["fr-FR", "es-MX"], "es"],
    [["pt-AO", "pt-PT"], "pt-BR"],
    [["en-GB-u-nu-latn"], "en-GB"],
    [["fr-FR", "it-CH", "en-GB"], "it"],
    [["C", "POSIX", "en_US", "not a tag", "es-MX"], "es"],
    [["pt--BR", "", "en-", "x-private"], "en-US"],
    [[], "en-US"],
    [["de-DE"], "en-US"],
  ])("resolves %j to %s", (preferences, expected) => {
    expect(resolveLocale(preferences as string[])).toBe(expected);
  });
});

test("a saved explicit choice restores without consulting native preferences", async () => {
  const { store, document, storage } = fixture(savedChoice("en-GB"));
  const source = vi.fn(async () => ["it-IT"]);
  await store.initialize(source);
  expect(store.getSnapshot()).toEqual({ choice: "en-GB", locale: "en-GB", pendingChoice: null, persistenceError: false });
  expect(source).not.toHaveBeenCalled();
  expect(document.documentElement.lang).toBe("en-GB");
  expect(document.title).toBe(translate("en-GB", "app.documentTitle"));
  expect(storage.writes).toBe(0);
});

test.each([null, "broken JSON", "{}", '{"version":2,"choice":"it"}', savedChoice("fr-FR"), savedChoice("system")])(
  "missing or invalid saved state uses System without rewriting storage: %s", async (saved) => {
    const { store, storage } = fixture(saved);
    await store.initialize(async () => ["es-MX"]);
    expect(store.getSnapshot()).toEqual({ choice: "system", locale: "es", pendingChoice: null, persistenceError: false });
    expect(storage.writes).toBe(0);
  },
);

test("manual choice immediately publishes coherent locale and document metadata and survives restart", async () => {
  const { store, storage, values, document, description } = fixture();
  await store.initialize(async () => ["it-IT"]);
  const observed: string[] = [];
  store.subscribe(() => {
    const { locale } = store.getSnapshot();
    expect(document.documentElement.lang).toBe(locale);
    expect(document.title).toBe(translate(locale, "app.documentTitle"));
    expect(description.content).toBe(translate(locale, "app.documentDescription"));
    observed.push(locale);
  });
  const selection = store.setChoice("pt-PT");
  expect(store.getSnapshot().locale).toBe("pt-PT");
  expect(observed).toEqual(["pt-PT"]);
  await selection;
  expect(storage.writes).toBe(1);
  const restarted = fixture(values.get(localeStorageKey));
  await restarted.store.initialize(async () => ["it"]);
  expect(restarted.store.getSnapshot().choice).toBe("pt-PT");
});

test("System remains pending without saving until native resolution commits in the same session", async () => {
  const { store, storage, values, document } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  const selecting = store.setChoice("system");
  expect(store.getSnapshot()).toEqual({ choice: "en-GB", locale: "en-GB", pendingChoice: "system", persistenceError: false });
  expect(document.documentElement.lang).toBe("en-GB");
  expect(storage.writes).toBe(0);
  expect(values.get(localeStorageKey)).toBe(savedChoice("en-GB"));
  languages.resolve(["it-IT"]);
  await selecting;
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "it", pendingChoice: null, persistenceError: false });
  expect(document.documentElement.lang).toBe("it");
  expect(values.has(localeStorageKey)).toBe(false);
  expect(storage.writes).toBe(1);
});

test("storage read failure resolves a usable session and reports that persistence is unavailable", async () => {
  const { store, storage } = fixture();
  storage.failRead = true;
  await store.initialize(async () => ["pt-PT"]);
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "pt-PT", pendingChoice: null, persistenceError: true });
});

test("storage accessor failure cannot prevent initialization or immediate manual choices", async () => {
  const store = createLocaleStore({ storage() { throw new Error("Denied"); } });
  stores.push(store);
  await store.initialize(async () => ["it"]);
  expect(store.getSnapshot().persistenceError).toBe(true);
  await store.setChoice("en-GB");
  expect(store.getSnapshot()).toEqual({ choice: "en-GB", locale: "en-GB", pendingChoice: null, persistenceError: true });
});

test("a failed manual write retains the session choice and a later successful save clears its warning", async () => {
  const { store, storage, values } = fixture(savedChoice("en-US"));
  await store.initialize(async () => []);
  storage.failWrite = true;
  await store.setChoice("pt-BR");
  expect(store.getSnapshot()).toEqual({ choice: "pt-BR", locale: "pt-BR", pendingChoice: null, persistenceError: true });
  expect(values.get(localeStorageKey)).toBe(savedChoice("en-US"));
  storage.failWrite = false;
  await store.setChoice("it");
  expect(store.getSnapshot().persistenceError).toBe(false);
  expect(values.get(localeStorageKey)).toBe(savedChoice("it"));
});

test("failed System removal commits the resolved locale as session-only", async () => {
  const { store, storage, values } = fixture(savedChoice("en-GB"));
  await store.initialize(async () => ["it-IT"]);
  storage.failWrite = true;
  await store.setChoice("system");
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "it", pendingChoice: null, persistenceError: true });
  expect(values.get(localeStorageKey)).toBe(savedChoice("en-GB"));
});

test.each(["resolve", "reject"] as const)("stale System %s cannot mutate a newer choice or its persistence warning", async (outcome) => {
  const { store, storage, values, document } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  const selectingSystem = store.setChoice("system");
  storage.failWrite = true;
  await store.setChoice("pt-PT");
  const authoritative = store.getSnapshot();
  if (outcome === "resolve") languages.resolve(["it-IT"]);
  else languages.reject(new Error("Native unavailable"));
  await selectingSystem;
  expect(store.getSnapshot()).toBe(authoritative);
  expect(document.documentElement.lang).toBe("pt-PT");
  expect(storage.writes).toBe(1);
  expect(values.get(localeStorageKey)).toBe(savedChoice("en-GB"));
  expect(store.getSnapshot().persistenceError).toBe(true);
});

test("stale System completion cannot overwrite a newer successfully saved manual choice", async () => {
  const { store, storage, values } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  const selecting = store.setChoice("system");
  await store.setChoice("pt-PT");
  languages.resolve(["it"]);
  await selecting;
  expect(store.getSnapshot()).toEqual({ choice: "pt-PT", locale: "pt-PT", pendingChoice: null, persistenceError: false });
  expect(values.get(localeStorageKey)).toBe(savedChoice("pt-PT"));
  expect(storage.writes).toBe(1);
});

test("a newer System request wins when responses arrive out of order", async () => {
  const { store } = fixture(savedChoice("en-GB"));
  const older = deferred<readonly string[]>();
  const newer = deferred<readonly string[]>();
  const source = vi.fn().mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
  await store.initialize(source);
  const first = store.setChoice("system");
  const second = store.setChoice("system");
  newer.resolve(["es"]);
  await second;
  older.resolve(["it"]);
  await first;
  expect(store.getSnapshot().locale).toBe("es");
});

test("native rejection and synchronous source failure safely resolve to en-US", async () => {
  const rejected = fixture();
  await rejected.store.initialize(async () => { throw new Error("Native unavailable"); });
  expect(rejected.store.getSnapshot().locale).toBe("en-US");
  const throwing = fixture();
  await throwing.store.initialize(() => { throw new Error("Native unavailable"); });
  expect(throwing.store.getSnapshot().locale).toBe("en-US");
});

test("an unavailable native source cannot strand startup or apply a late response", async () => {
  vi.useFakeTimers();
  const { store, document } = fixture(null, 50);
  const languages = deferred<readonly string[]>();
  const initialization = store.initialize(() => languages.promise);
  await vi.advanceTimersByTimeAsync(50);
  await initialization;
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "en-US", pendingChoice: null, persistenceError: false });
  languages.resolve(["it"]);
  await Promise.resolve();
  expect(store.getSnapshot().locale).toBe("en-US");
  expect(document.documentElement.lang).toBe("en-US");
});

test("external manual storage choices invalidate pending System without writing back", async () => {
  const { store, external, storage, document } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  const selecting = store.setChoice("system");
  external(savedChoice("es"));
  languages.reject(new Error("Late failure"));
  await selecting;
  expect(store.getSnapshot()).toEqual({ choice: "es", locale: "es", pendingChoice: null, persistenceError: false });
  expect(document.documentElement.lang).toBe("es");
  expect(storage.writes).toBe(0);
});

test("external removal resolves System and ignores invalid and unrelated external updates", async () => {
  const { store, external, storage } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  external("invalid");
  external(savedChoice("it"), "another.preference");
  expect(store.getSnapshot().choice).toBe("en-GB");
  external(null);
  expect(store.getSnapshot().pendingChoice).toBe("system");
  languages.resolve(["pt-AO"]);
  await vi.waitFor(() => expect(store.getSnapshot().locale).toBe("pt-BR"));
  expect(store.getSnapshot().choice).toBe("system");
  expect(storage.writes).toBe(0);
});

test("a storage clear advances intent and resolves System without writing back", async () => {
  const { store, external, storage } = fixture(savedChoice("en-GB"));
  await store.initialize(async () => ["it"]);
  external(null, null);
  await vi.waitFor(() => expect(store.getSnapshot().locale).toBe("it"));
  expect(storage.writes).toBe(0);
});

test("disposing an isolated store prevents a pending response from publishing metadata", async () => {
  const { store, document } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  const selecting = store.setChoice("system");
  store.dispose();
  languages.resolve(["it"]);
  await selecting;
  expect(document.documentElement.lang).toBe("en-GB");
});

test("bundled messages translate and missing noncanonical entries use the real English catalog", () => {
  const incompleteItalian = { ...catalogs.it };
  delete (incompleteItalian as Partial<typeof incompleteItalian>)["common.language"];
  const withMissingEntry = createTranslator({ ...catalogs, it: incompleteItalian });
  expect(translate("it", "common.language")).toBe("Lingua");
  expect(withMissingEntry("it", "common.language")).toBe(catalogs["en-US"]["common.language"]);
});

test("locale-specific numbers use the selected region", () => {
  expect(formatNumber("en-US", 1234.5)).toBe("1,234.5");
  expect(formatNumber("pt-PT", 12345.5)).toBe("12 345,5");
});

test("real ICU count messages preserve zero, singular, plural and selected-locale numbers", () => {
  expect(translate("en-US", "ui.pixels", { count: 0 })).toBe("0 pixels");
  expect(translate("en-US", "ui.pixels", { count: 1 })).toBe("1 pixel");
  expect(translate("en-US", "ui.pixels", { count: 2 })).toBe("2 pixels");
  expect(translate("en-GB", "ui.pixels", { count: 1234 })).toBe("1,234 pixels");
});

test("empty and malformed native results use the safe fallback", async () => {
  const empty = fixture();
  await empty.store.initialize(async () => []);
  expect(empty.store.getSnapshot().locale).toBe("en-US");
  const invalid = fixture();
  await invalid.store.initialize(async () => ["pt--BR", "C", "POSIX"]);
  expect(invalid.store.getSnapshot().locale).toBe("en-US");
  const malformed = fixture();
  await malformed.store.initialize(async () => null as unknown as readonly string[]);
  expect(malformed.store.getSnapshot().locale).toBe("en-US");
});

test("new manual intent during startup prevents native completion from replacing the selection", async () => {
  const { store, values } = fixture();
  const languages = deferred<readonly string[]>();
  const initialization = store.initialize(() => languages.promise);
  await store.setChoice("en-GB");
  languages.resolve(["it"]);
  await initialization;
  expect(store.getSnapshot()).toEqual({ choice: "en-GB", locale: "en-GB", pendingChoice: null, persistenceError: false });
  expect(values.get(localeStorageKey)).toBe(savedChoice("en-GB"));
});

test("pending System keeps an existing session-only warning until the new choice is persisted", async () => {
  const { store, storage } = fixture(savedChoice("en-GB"));
  const languages = deferred<readonly string[]>();
  await store.initialize(() => languages.promise);
  storage.failWrite = true;
  await store.setChoice("pt-PT");
  storage.failWrite = false;
  const selecting = store.setChoice("system");
  expect(store.getSnapshot()).toEqual({ choice: "pt-PT", locale: "pt-PT", pendingChoice: "system", persistenceError: true });
  languages.resolve(["it"]);
  await selecting;
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "it", pendingChoice: null, persistenceError: false });
});

test("a failed native System request commits and persists the specified English fallback", async () => {
  const { store, storage, values } = fixture(savedChoice("en-GB"));
  await store.initialize(async () => { throw new Error("Native unavailable"); });
  await store.setChoice("system");
  expect(store.getSnapshot()).toEqual({ choice: "system", locale: "en-US", pendingChoice: null, persistenceError: false });
  expect(values.has(localeStorageKey)).toBe(false);
  expect(storage.writes).toBe(1);
});

test("missing required ICU parameters fail rather than producing unusable placeholder text", () => {
  expect(() => translate("en-US", "ui.pixels")).toThrow();
});
