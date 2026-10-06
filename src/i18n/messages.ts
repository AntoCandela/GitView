/** Formats bundled, typed ICU messages; repository text is never a translation key. */
import { IntlMessageFormat } from "intl-messageformat";
import brazilianPortuguese from "./locales/pt-BR.json";
import europeanPortuguese from "./locales/pt-PT.json";
import italian from "./locales/it.json";
import spanish from "./locales/es.json";
import americanEnglish from "./locales/en-US.json";
import britishEnglish from "./locales/en-GB.json";
import type { Locale } from "./locale";

export type MessageKey = keyof typeof americanEnglish;
export type MessageParams = Record<string, string | number>;
type MessageCatalog = Partial<Record<MessageKey, string>>;
export type MessageCatalogs = Readonly<Partial<Record<Locale, MessageCatalog>>> & { "en-US": Record<MessageKey, string> };
export type Translator = (locale: Locale, key: MessageKey, params?: MessageParams) => string;

export const catalogs: MessageCatalogs = {
  "pt-BR": brazilianPortuguese,
  "pt-PT": europeanPortuguese,
  it: italian,
  es: spanish,
  "en-US": americanEnglish,
  "en-GB": britishEnglish,
};

/** Isolates formatter caches and fallback fixtures without modifying bundled resources. */
export function createTranslator(resources: MessageCatalogs): Translator {
  const formatters = new Map<Locale, Map<MessageKey, IntlMessageFormat>>();
  return (locale, key, params) => {
    let localeFormatters = formatters.get(locale);
    if (!localeFormatters) {
      localeFormatters = new Map();
      formatters.set(locale, localeFormatters);
    }
    let formatter = localeFormatters.get(key);
    if (!formatter) {
      const canonical = resources["en-US"][key];
      if (typeof canonical !== "string") throw new Error("Unknown canonical message");
      if (key.startsWith("companion.") && typeof resources[locale]?.[key] !== "string") {
        throw new Error("Missing companion translation");
      }
      const message = resources[locale]?.[key] ?? canonical;
      formatter = new IntlMessageFormat(message, locale, undefined, { ignoreTag: true });
      localeFormatters.set(key, formatter);
    }
    return String(formatter.format(params));
  };
}

export const translate = createTranslator(catalogs);
const numberFormats = new Map<Locale, Intl.NumberFormat>();

export function formatNumber(locale: Locale, value: number): string {
  let formatter = numberFormats.get(locale);
  if (!formatter) {
    formatter = new Intl.NumberFormat(locale);
    numberFormats.set(locale, formatter);
  }
  return formatter.format(value);
}
