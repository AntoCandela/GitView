/** Worker-only local theme loaders; explicit imports keep the renderer free of theme payloads. */
import type { CodeTheme } from "../../appearance";

export const codeThemeLoaders = {
  "github-light": () => import("shiki/themes/github-light.mjs"),
  "catppuccin-latte": () => import("shiki/themes/catppuccin-latte.mjs"),
  "solarized-light": () => import("shiki/themes/solarized-light.mjs"),
  "github-dark": () => import("shiki/themes/github-dark.mjs"),
} satisfies Record<Exclude<CodeTheme, "plain">, () => Promise<{ default: unknown }>>;
