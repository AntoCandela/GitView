/** Describes complete interface palettes and coordinated choices without loading icon or grammar assets. */
import type { CodeTheme } from "./codeThemes";
import type { IconTheme } from "../../ui/file-icons/iconThemes";
import type { MessageKey } from "../../i18n";

export const appearanceThemes = [
  { id: "cream", labelKey: "appearance.theme.cream", familyKey: "appearance.family.light", swatch: "#f8f4ec", ink: "#020d26", syntax: "github-light", icons: "classic" },
  { id: "paper", labelKey: "appearance.theme.paper", familyKey: "appearance.family.light", swatch: "#ffffff", ink: "#24292e", syntax: "github-light", icons: "material" },
  { id: "mist", labelKey: "appearance.theme.mist", familyKey: "appearance.family.light", swatch: "#eff1f5", ink: "#4c4f69", syntax: "catppuccin-latte", icons: "catppuccin" },
  { id: "stone", labelKey: "appearance.theme.stone", familyKey: "appearance.family.gray", swatch: "#e8e9e8", ink: "#242b2c", syntax: "solarized-light", icons: "material" },
  { id: "graphite", labelKey: "appearance.theme.graphite", familyKey: "appearance.family.gray", swatch: "#30363d", ink: "#e6edf3", syntax: "github-dark", icons: "material" },
  { id: "midnight", labelKey: "appearance.theme.midnight", familyKey: "appearance.family.dark", swatch: "#0d1117", ink: "#e6edf3", syntax: "github-dark", icons: "material" },
] as const satisfies readonly { id: string; labelKey: MessageKey; familyKey: MessageKey; swatch: string; ink: string; syntax: CodeTheme; icons: IconTheme }[];

export type AppearanceTheme = (typeof appearanceThemes)[number]["id"];
export function isAppearanceTheme(value: unknown): value is AppearanceTheme {
  return appearanceThemes.some(({ id }) => id === value);
}
export function appearanceTheme(id: AppearanceTheme) {
  return appearanceThemes.find((theme) => theme.id === id)!;
}
