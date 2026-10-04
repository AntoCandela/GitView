/** Validates bundled locale coverage and ICU contracts without printing translated or private payloads. */
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parse, TYPE } from "@formatjs/icu-messageformat-parser";

export const requiredLocales = ["pt-BR", "pt-PT", "it", "es", "en-US", "en-GB"];
const argumentKinds = {
  [TYPE.argument]: "argument",
  [TYPE.number]: "number",
  [TYPE.date]: "date",
  [TYPE.time]: "time",
  [TYPE.select]: "select",
  [TYPE.plural]: "plural",
};
const safeKey = /^[a-zA-Z][a-zA-Z0-9]*(?:[._-][a-zA-Z0-9]+)*$/;
const pluralCategories = Object.fromEntries(requiredLocales.map((locale) => [locale, {
  cardinal: new Intl.PluralRules(locale, { type: "cardinal" }).resolvedOptions().pluralCategories,
  ordinal: new Intl.PluralRules(locale, { type: "ordinal" }).resolvedOptions().pluralCategories,
}]));

function issue(locale, key, code) {
  return { locale: requiredLocales.includes(locale) ? locale : null, key: typeof key === "string" && safeKey.test(key) ? key : null, code };
}

function addContract(contracts, argument, contract) {
  let existing = contracts.get(argument);
  if (!existing) {
    existing = new Set();
    contracts.set(argument, existing);
  }
  existing.add(contract);
}

function parseContract(message, locale, key, issues) {
  let elements;
  try {
    elements = parse(message, { ignoreTag: true, requiresOtherClause: true });
  } catch {
    issues.push(issue(locale, key, "malformed_icu"));
    return null;
  }
  const contract = { arguments: new Map(), plurals: new Map(), selects: new Map() };
  const categories = pluralCategories[locale];
  let invalidCategory = false;

  function visit(nodes) {
    for (const element of nodes) {
      const kind = argumentKinds[element.type];
      if (kind) addContract(contract.arguments, element.value, kind);
      if (element.type === TYPE.plural) {
        const selectors = Object.keys(element.options);
        for (const selector of selectors) {
          if (!selector.startsWith("=") && !categories[element.pluralType].includes(selector)) invalidCategory = true;
        }
        const exactCases = selectors.filter((selector) => selector.startsWith("=")).sort();
        addContract(contract.plurals, element.value, JSON.stringify([element.pluralType, element.offset, exactCases]));
      } else if (element.type === TYPE.select) {
        addContract(contract.selects, element.value, JSON.stringify(Object.keys(element.options).sort()));
      }
      if (element.type === TYPE.plural || element.type === TYPE.select) {
        for (const option of Object.values(element.options)) visit(option.value);
      }
    }
  }

  visit(elements);
  if (invalidCategory) issues.push(issue(locale, key, "invalid_plural_category"));
  return contract;
}

function sameContracts(canonical, translated) {
  if (canonical.size !== translated.size) return false;
  for (const [argument, contracts] of canonical) {
    const other = translated.get(argument);
    if (!other || contracts.size !== other.size) return false;
    for (const contract of contracts) if (!other.has(contract)) return false;
  }
  return true;
}

function isCatalog(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

/** Returns only stable issue codes and safe catalog identifiers; a nonempty result blocks release. */
export function checkCatalogs(catalogs) {
  const issues = [];
  if (!isCatalog(catalogs)) return [issue(null, null, "invalid_catalogs")];
  for (const locale of Object.keys(catalogs)) {
    if (!requiredLocales.includes(locale)) issues.push(issue(null, null, "extra_locale"));
  }
  for (const locale of requiredLocales) {
    if (!Object.hasOwn(catalogs, locale)) issues.push(issue(locale, null, "missing_locale"));
    else if (!isCatalog(catalogs[locale]) || Object.keys(catalogs[locale]).length === 0) issues.push(issue(locale, null, "invalid_catalog"));
  }
  const canonical = catalogs["en-US"];
  if (!isCatalog(canonical) || Object.keys(canonical).length === 0) return issues;
  const canonicalContracts = new Map();
  const canonicalKeys = Object.keys(canonical);

  for (const key of canonicalKeys) {
    if (!safeKey.test(key)) issues.push(issue("en-US", key, "invalid_key"));
    const message = canonical[key];
    if (typeof message !== "string" || message.trim().length === 0) {
      issues.push(issue("en-US", key, "invalid_message"));
      continue;
    }
    const contract = parseContract(message, "en-US", key, issues);
    if (contract) canonicalContracts.set(key, contract);
  }

  for (const locale of requiredLocales) {
    if (locale === "en-US" || !isCatalog(catalogs[locale])) continue;
    const catalog = catalogs[locale];
    for (const key of Object.keys(catalog)) {
      if (!Object.hasOwn(canonical, key)) issues.push(issue(locale, key, "extra_key"));
    }
    for (const key of canonicalKeys) {
      if (!Object.hasOwn(catalog, key)) {
        issues.push(issue(locale, key, "missing_key"));
        continue;
      }
      const message = catalog[key];
      if (typeof message !== "string" || message.trim().length === 0) {
        issues.push(issue(locale, key, "invalid_message"));
        continue;
      }
      const translated = parseContract(message, locale, key, issues);
      const original = canonicalContracts.get(key);
      if (!translated || !original) continue;
      if (!sameContracts(original.arguments, translated.arguments)) issues.push(issue(locale, key, "incompatible_arguments"));
      if (!sameContracts(original.plurals, translated.plurals)) issues.push(issue(locale, key, "incompatible_plural"));
      if (!sameContracts(original.selects, translated.selects)) issues.push(issue(locale, key, "incompatible_select"));
    }
  }
  return issues;
}

/** Loads only the six fixed local resources; I/O and JSON failures never include paths or source text. */
export async function checkCatalogDirectory(directory = new URL("../src/i18n/locales/", import.meta.url)) {
  const catalogs = {};
  const issues = [];
  await Promise.all(requiredLocales.map(async (locale) => {
    let content;
    try { content = await readFile(new URL(`${locale}.json`, directory), "utf8"); }
    catch {
      issues.push(issue(locale, null, "unreadable_catalog"));
      return;
    }
    try { catalogs[locale] = JSON.parse(content); }
    catch { issues.push(issue(locale, null, "invalid_json")); }
  }));
  return [...issues, ...checkCatalogs(catalogs)].sort((left, right) =>
    `${left.locale ?? ""}:${left.key ?? ""}:${left.code}`.localeCompare(`${right.locale ?? ""}:${right.key ?? ""}:${right.code}`, "en-US"));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === resolve(process.argv[1])) {
  try {
    const issues = await checkCatalogDirectory();
    if (issues.length > 0) {
      for (const { locale, key, code } of issues) console.error(`locales: ${locale ?? "catalogs"}${key ? `/${key}` : ""}: ${code}`);
      process.exitCode = 1;
    } else {
      console.log(`locales: ${requiredLocales.length} complete catalogs passed ICU validation`);
    }
  } catch {
    console.error("locales: catalog_validation_unavailable");
    process.exitCode = 1;
  }
}
