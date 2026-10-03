/** Bundles upstream file-icon associations, local SVGs and licenses without extension runtimes. */
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { generateManifest, type Manifest } from "material-icon-theme";
import type { Plugin } from "vite";
import catppuccinManifest from "../src/assets/icon-themes/catppuccin-latte/theme.json" with { type: "json" };

const moduleId = "virtual:file-icon-themes";
const resolvedId = `\0${moduleId}`;

function normalizeAssociations(base?: Record<string, string>, light?: Record<string, string>): Record<string, string> {
  // Normalize before merging so light-theme overrides win even when key casing differs.
  return Object.fromEntries([...Object.entries(base ?? {}), ...Object.entries(light ?? {})]
    .map(([name, icon]) => [name.toLowerCase(), icon]));
}

export function fileIconThemes(): Plugin {
  return {
    name: "gitview-file-icon-themes",
    resolveId(id) { if (id === moduleId) return resolvedId; },
    load(id) {
      if (id !== resolvedId) return;
      const require = createRequire(import.meta.url);
      const materialRoot = dirname(require.resolve("material-icon-theme/package.json"));
      const materialVersion = require(resolve(materialRoot, "package.json")).version;
      const catppuccinRoot = fileURLToPath(new URL("../src/assets/icon-themes/catppuccin-latte/", import.meta.url));
      const sources: Record<string, { manifest: Manifest; root: string; license: string }> = {
        material: { manifest: generateManifest(), root: resolve(materialRoot, "dist"),
          license: fileURLToPath(new URL(`../licenses/texts/npm/material-icon-theme/${materialVersion}/LICENSE`, import.meta.url)) },
        catppuccin: { manifest: catppuccinManifest, root: catppuccinRoot,
          license: fileURLToPath(new URL("../licenses/texts/assets/catppuccin-latte/LICENSE.txt", import.meta.url)) },
      };
      const imports: string[] = [];
      const themes: string[] = [];
      for (const [theme, source] of Object.entries(sources)) {
        const { manifest } = source;
        const associations = {
          fileNames: normalizeAssociations(manifest.fileNames, manifest.light?.fileNames),
          fileExtensions: normalizeAssociations(manifest.fileExtensions, manifest.light?.fileExtensions),
          folderNames: normalizeAssociations(manifest.folderNames, manifest.light?.folderNames),
          folderNamesExpanded: normalizeAssociations(manifest.folderNamesExpanded, manifest.light?.folderNamesExpanded),
        };
        const defaults = { file: manifest.file!, folder: manifest.folder!, folderExpanded: manifest.folderExpanded! };
        const iconIds = new Set([...Object.values(defaults), ...Object.values(associations).flatMap(Object.values)]);
        const urls: string[] = [];
        for (const iconId of iconIds) {
          const definition = manifest.iconDefinitions?.[iconId];
          if (!definition?.iconPath) throw new Error(`${theme} icon missing: ${iconId}`);
          const assetPath = resolve(source.root, definition.iconPath);
          const variable = `asset${imports.length}`;
          imports.push(`import ${variable} from ${JSON.stringify(`${assetPath}?url&no-inline`)};`);
          urls.push(`${JSON.stringify(iconId)}: ${variable}`);
        }
        const licenseVariable = `license${imports.length}`;
        imports.push(`import ${licenseVariable} from ${JSON.stringify(`${source.license}?url&no-inline`)};`);
        themes.push(`${JSON.stringify(theme)}: { ...${JSON.stringify({ ...associations, ...defaults })}, iconUrls: {${urls.join(",")}}, licenseUrl: ${licenseVariable} }`);
      }
      return `${imports.join("\n")}\nexport const fileIconThemes = {${themes.join(",")}};`;
    },
  };
}
