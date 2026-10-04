/** Exercises real catalog validation separately from the runtime's safe English fallback. */
import { describe, expect, test } from "vitest";
import { catalogs, createTranslator } from "../../src/i18n/messages";
// @ts-expect-error The verifier is a shared Node ESM script, not a renderer dependency.
import { checkCatalogs } from "../../scripts/check-locales.mjs";

function fixture() {
  const canonical = {
    greeting: "Hello {name}",
    count: "{count, plural, =0 {No files} one {# file} other {# files}}",
    number: "Size: {size, number}",
    selection: "{mode, select, tree {Tree} list {List} other {Files}}",
    ordinal: "{place, selectordinal, one {#st} two {#nd} few {#rd} other {#th}}",
  };
  const regional = {
    ...canonical,
    ordinal: "{place, selectordinal, other {#}}",
  };
  return {
    "en-US": { ...canonical },
    "en-GB": { ...canonical },
    "pt-BR": { ...regional },
    "pt-PT": { ...regional },
    it: { ...regional },
    es: { ...regional },
  };
}

test("all six bundled catalogs satisfy exact key and ICU compatibility", () => {
  expect(checkCatalogs(catalogs)).toEqual([]);
});

test("valid catalogs allow independent regional wording and locale-specific plural categories", () => {
  const resources = fixture();
  resources["pt-BR"].count = "{count, plural, =0 {Nenhum arquivo} one {# arquivo} many {# arquivos} other {# arquivos}}";
  resources["pt-PT"].count = "{count, plural, =0 {Nenhum ficheiro} one {# ficheiro} other {# ficheiros}}";
  expect(checkCatalogs(resources)).toEqual([]);
});

test("missing required locales fail validation", () => {
  const { es: _spanish, ...resources } = fixture();
  expect(checkCatalogs(resources)).toContainEqual({ locale: "es", key: null, code: "missing_locale" });
});

test("unexpected catalogs fail instead of silently expanding supported locales", () => {
  expect(checkCatalogs({ ...fixture(), fr: {} })).toContainEqual({ locale: null, key: null, code: "extra_locale" });
});

test("missing and extra keys fail even when the runtime can fall back", () => {
  const incomplete = { ...catalogs.it };
  delete (incomplete as Partial<typeof incomplete>)["common.language"];
  const resources = { ...catalogs, it: { ...incomplete, unknown: "Unexpected" } };
  expect(createTranslator(resources)("it", "common.language")).toBe(catalogs["en-US"]["common.language"]);
  expect(checkCatalogs(resources)).toEqual(expect.arrayContaining([
    { locale: "it", key: "common.language", code: "missing_key" },
    { locale: "it", key: "unknown", code: "extra_key" },
  ]));
});

test.each([null, [], "catalog", { key: 12 }, { key: "" }])("invalid canonical resources fail: %j", (canonical) => {
  expect(checkCatalogs({ ...fixture(), "en-US": canonical })).not.toEqual([]);
});

describe("malformed ICU and parameter contracts", () => {
  test("malformed canonical ICU fails before regional comparison", () => {
    const resources = fixture();
    resources["en-US"].greeting = "Hello {name";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "en-US", key: "greeting", code: "malformed_icu" });
  });

  test("malformed localized ICU is rejected", () => {
    const resources = fixture();
    resources.it.greeting = "Ciao {name";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "it", key: "greeting", code: "malformed_icu" });
  });

  test.each(["Hello", "Hello {otherName}", "Hello {name} {extra}"])("changed required arguments fail: %s", (message) => {
    const resources = fixture();
    resources.es.greeting = message;
    expect(checkCatalogs(resources)).toContainEqual({ locale: "es", key: "greeting", code: "incompatible_arguments" });
  });

  test("changing a number parameter into a date fails", () => {
    const resources = fixture();
    resources.it.number = "Dimensione: {size, date}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "it", key: "number", code: "incompatible_arguments" });
  });

  test("arguments nested inside plural options are checked", () => {
    const resources = fixture();
    resources.es.count = "{count, plural, =0 {Ningún archivo} one {{filename}} other {# archivos}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "es", key: "count", code: "incompatible_arguments" });
  });

  test("HTML-like text remains inert text, not an executable ICU rich-text tag", () => {
    const resources = fixture();
    resources.it.greeting = "<b>Ciao {name}</b>";
    expect(checkCatalogs(resources)).toEqual([]);
  });
});

describe("plural and select compatibility", () => {
  test("a plural without the required other option fails parsing", () => {
    const resources = fixture();
    resources.it.count = "{count, plural, one {Un file}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "it", key: "count", code: "malformed_icu" });
  });

  test("a category unavailable in the target locale fails", () => {
    const resources = fixture();
    resources.es.count = "{count, plural, =0 {Nada} one {# archivo} few {# archivos} other {# archivos}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "es", key: "count", code: "invalid_plural_category" });
  });

  test("an invalid category in the canonical locale fails too", () => {
    const resources = fixture();
    resources["en-US"].count = "{count, plural, =0 {None} one {# file} many {# files} other {# files}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "en-US", key: "count", code: "invalid_plural_category" });
  });

  test("cardinal and ordinal plural rules are not interchangeable", () => {
    const resources = fixture();
    resources["en-GB"].count = "{count, selectordinal, =0 {No files} one {# file} other {# files}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "en-GB", key: "count", code: "incompatible_plural" });
  });

  test("dropping exact-number semantics fails", () => {
    const resources = fixture();
    resources.es.count = "{count, plural, one {# archivo} other {# archivos}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "es", key: "count", code: "incompatible_plural" });
  });

  test("changing a plural offset fails", () => {
    const resources = fixture();
    resources.it.count = "{count, plural, offset:1 =0 {Nessun file} one {# file} other {# file}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "it", key: "count", code: "incompatible_plural" });
  });

  test("changing stable select discriminants fails", () => {
    const resources = fixture();
    resources.it.selection = "{mode, select, albero {Albero} list {Elenco} other {File}}";
    expect(checkCatalogs(resources)).toContainEqual({ locale: "it", key: "selection", code: "incompatible_select" });
  });
});

test("validation diagnostics contain safe identifiers and codes, never message content", () => {
  const resources = fixture();
  resources.it.greeting = "private /home/fixture/secret {";
  const issues = checkCatalogs({ ...resources, "private\nlocale": {} });
  expect(JSON.stringify(issues)).not.toContain("private");
  expect(JSON.stringify(issues)).not.toContain("/home/");
});
