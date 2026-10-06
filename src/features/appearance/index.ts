/** Exposes shared appearance palettes and review presentation preferences to other features. */
export { AppearanceMenu } from "./AppearanceMenu";
export { useAppearanceTheme, ReadOnlyAppearanceProvider } from "./appearancePreference";
export { appearanceTheme } from "./appearanceThemes";
export { codeTheme, codeThemes } from "./codeThemes";
export type { CodeTheme } from "./codeThemes";
export { useReviewChoices, ReadOnlyReviewProvider } from "./reviewPreferences";
export type { ReviewChoices, ReviewMode, LineMode } from "./reviewPreferences";
