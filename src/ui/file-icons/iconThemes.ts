/** Owns file-icon theme choices and presentation metadata; icon associations remain upstream-owned. */
import { fileIconThemes } from "virtual:file-icon-themes";

export const iconThemes = [
  {
    id: "classic",
    label: "Classic",
    preview: { folder: "src", files: ["App.tsx", "index.ts", "main.rs"] },
    license: null,
  },
  {
    id: "material",
    label: "Material",
    preview: { folder: "src", files: ["App.tsx", "index.ts", "main.rs"] },
    license: { label: "Material Icon Theme · MIT license", url: fileIconThemes.material.licenseUrl,
      sourceUrl: "https://github.com/material-extensions/vscode-material-icon-theme" },
  },
  {
    id: "catppuccin",
    label: "Catppuccin Latte",
    preview: { folder: "src", files: ["App.tsx", "index.ts", "main.rs"] },
    license: { label: "Catppuccin Icons · MIT license", url: fileIconThemes.catppuccin.licenseUrl,
      sourceUrl: "https://github.com/catppuccin/vscode-icons" },
  },
] as const;

export type IconTheme = (typeof iconThemes)[number]["id"];

export function isIconTheme(value: string | null): value is IconTheme {
  return iconThemes.some(({ id }) => id === value);
}
