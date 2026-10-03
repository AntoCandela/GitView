/** Describes complete interface palettes and coordinated choices without loading icon or grammar assets. */
import type { CodeTheme } from "./codeThemes";
import type { IconTheme } from "../../ui/file-icons/iconThemes";

export const appearanceThemes = [
  { id: "cream", label: "Cream", family: "Light", swatch: "#f8f4ec", ink: "#020d26", syntax: "github-light", icons: "classic" },
  { id: "paper", label: "Paper", family: "Light", swatch: "#ffffff", ink: "#24292e", syntax: "github-light", icons: "material" },
  { id: "mist", label: "Mist", family: "Light", swatch: "#eff1f5", ink: "#4c4f69", syntax: "catppuccin-latte", icons: "catppuccin" },
  { id: "stone", label: "Stone", family: "Gray", swatch: "#e8e9e8", ink: "#242b2c", syntax: "solarized-light", icons: "material" },
  { id: "graphite", label: "Graphite", family: "Gray", swatch: "#30363d", ink: "#e6edf3", syntax: "github-dark", icons: "material" },
  { id: "midnight", label: "Midnight", family: "Dark", swatch: "#0d1117", ink: "#e6edf3", syntax: "github-dark", icons: "material" },
] as const satisfies readonly { id: string; label: string; family: "Light" | "Gray" | "Dark"; swatch: string; ink: string; syntax: CodeTheme; icons: IconTheme }[];

export type AppearanceTheme = (typeof appearanceThemes)[number]["id"];
export function isAppearanceTheme(value: unknown): value is AppearanceTheme {
  return appearanceThemes.some(({ id }) => id === value);
}
export function appearanceTheme(id: AppearanceTheme) {
  return appearanceThemes.find((theme) => theme.id === id)!;
}
