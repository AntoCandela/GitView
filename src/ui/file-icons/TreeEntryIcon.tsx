/** Keeps file-tree artwork decorative and independent of Git status and selection. */
import { File, Folder } from "lucide-react";
import { useIconTheme } from "./IconThemeProvider";
import type { IconTheme } from "./iconThemes";
import { fileIcon, folderIcon } from "./fileIconLookup";

export function TreeEntryIcon({ name, kind, expanded = false, theme: previewTheme }: {
  name: string;
  kind: "file" | "folder";
  expanded?: boolean;
  theme?: IconTheme;
}) {
  const preference = useIconTheme();
  const theme = previewTheme ?? preference?.theme ?? "classic";
  if (theme === "classic") return kind === "folder" ? <Folder aria-hidden="true" /> : <File aria-hidden="true" />;
  const src = kind === "folder" ? folderIcon(theme, name, expanded) : fileIcon(theme, name);
  return <img className="tree-entry-icon" src={src} alt="" aria-hidden="true" width={16} height={16} draggable={false} />;
}
