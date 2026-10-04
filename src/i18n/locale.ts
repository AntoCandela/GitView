/** Defines supported interface locales and ordered, region-aware system matching. */
export const locales = ["pt-BR", "pt-PT", "it", "es", "en-US", "en-GB"] as const;
export type Locale = typeof locales[number];
export type LocaleChoice = "system" | Locale;
export type PreferredLanguageSource = () => Promise<readonly string[]>;
export type LocaleSnapshot = Readonly<{
  choice: LocaleChoice;
  locale: Locale;
  pendingChoice: "system" | null;
  persistenceError: boolean;
}>;

const familyLocales: Readonly<Record<string, Locale>> = {
  pt: "pt-BR",
  it: "it",
  es: "es",
  en: "en-US",
};

export function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && locales.some((locale) => locale === value);
}

/** Each preference gets its exact regional match, then its family, before trying the next. */
export function resolveLocale(preferences: readonly string[]): Locale {
  for (const preference of preferences) {
    if (typeof preference !== "string") continue;
    try {
      const tag = new Intl.Locale(preference);
      if (isLocale(tag.baseName)) return tag.baseName;
      const family = familyLocales[tag.language];
      if (family) return family;
    } catch {
      // Malformed OS preferences do not displace later valid preferences.
    }
  }
  return "en-US";
}
