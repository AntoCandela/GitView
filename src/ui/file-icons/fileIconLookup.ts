/** Resolves display basenames only; native paths and file contents never participate in icon choice. */
import { fileIconThemes } from "virtual:file-icon-themes";
import type { IconTheme } from "./iconThemes";

function association(table: Record<string, string>, name: string): string | undefined {
  return Object.hasOwn(table, name) ? table[name] : undefined;
}

/** Exact names win over the longest matching compound extension; matching ignores case. */
export function fileIcon(theme: Exclude<IconTheme, "classic">, name: string): string {
  const { fileNames, fileExtensions, file, iconUrls } = fileIconThemes[theme];
  const basename = name.toLowerCase();
  const namedIcon = association(fileNames, basename);
  if (namedIcon) return iconUrls[namedIcon];
  for (let dot = basename.indexOf("."); dot !== -1; dot = basename.indexOf(".", dot + 1)) {
    const extensionIcon = association(fileExtensions, basename.slice(dot + 1));
    if (extensionIcon) return iconUrls[extensionIcon];
  }
  return iconUrls[file];
}

/** Folders use their own basename and expanded state, with an upstream generic fallback. */
export function folderIcon(theme: Exclude<IconTheme, "classic">, name: string, expanded: boolean): string {
  const icons = fileIconThemes[theme];
  const names = expanded ? icons.folderNamesExpanded : icons.folderNames;
  const fallback = expanded ? icons.folderExpanded : icons.folder;
  return icons.iconUrls[association(names, name.toLowerCase()) ?? fallback];
}
