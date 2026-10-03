/** Lists bundled syntax palettes for the UI; only the worker imports actual Shiki theme data. */
export const codeThemes = [
  { id: "plain", label: "Plain", source: null,
    preview: { background: "var(--canvas)", foreground: "var(--ink)", muted: "var(--muted)", keyword: "var(--ink)", string: "var(--ink)",
      addition: "var(--diff-add-bg)", removal: "var(--diff-remove-bg)", inlineAdd: "var(--diff-inline-add-bg)", inlineRemove: "var(--diff-inline-remove-bg)" } },
  { id: "github-light", label: "GitHub Light", source: "https://github.com/primer/github-vscode-theme",
    preview: { background: "#fff", foreground: "#24292e", muted: "#57606a", keyword: "#d73a49", string: "#032f62",
      addition: "#e6ffec", removal: "#ffebe9", inlineAdd: "#abf2bc", inlineRemove: "#ffcecb" } },
  { id: "catppuccin-latte", label: "Catppuccin Latte", source: "https://github.com/catppuccin/vscode",
    preview: { background: "#eff1f5", foreground: "#4c4f69", muted: "#5c5f77", keyword: "#8839ef", string: "#40a02b",
      addition: "#d7eedc", removal: "#f6deda", inlineAdd: "#a9d9b1", inlineRemove: "#edb7ab" } },
  { id: "solarized-light", label: "Solarized Light", source: "https://github.com/microsoft/vscode/blob/main/extensions/theme-solarized-light/themes/solarized-light-color-theme.json",
    preview: { background: "#fdf6e3", foreground: "#657b83", muted: "#657b83", keyword: "#586e75", string: "#2aa198",
      addition: "#e5efdf", removal: "#f9e6d8", inlineAdd: "#bbdbb9", inlineRemove: "#edc2a7" } },
  { id: "github-dark", label: "GitHub Dark", source: "https://github.com/primer/github-vscode-theme",
    preview: { background: "#24292e", foreground: "#e1e4e8", muted: "#a4acb5", keyword: "#f97583", string: "#9ecbff",
      addition: "#173b2d", removal: "#48262d", inlineAdd: "#276044", inlineRemove: "#783740" } },
] as const;

export type CodeTheme = (typeof codeThemes)[number]["id"];
export function isCodeTheme(value: unknown): value is CodeTheme {
  return codeThemes.some(({ id }) => id === value);
}
export function codeTheme(id: CodeTheme) {
  return codeThemes.find((theme) => theme.id === id)!;
}
