/** Shares file-view switching and directory controls without owning file data or review authority. */

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChangeTree, changeDirectoryId, type ChangeTreeDirectory, type ChangeTreeFile } from "./ChangeTree";
import { CollapseAllIcon, ExpandAllIcon, FileIcon, FolderIcon } from "../icons";
import type { ChangeTreeSummaryProps } from "./changeTreeRows";
import { Tooltip } from "../Tooltip";
import { useTranslation } from "../../i18n";

const NO_DIRECTORIES: ChangeTreeDirectory[] = [];

export function FileExplorer({ files, directories = NO_DIRECTORIES, summaryFiles, treeLabel, listLabel, selectedId, onSelect, count = files.length, countLabel, onExpandedDirectoriesChange, actions, children }: ChangeTreeSummaryProps & {
  files: ChangeTreeFile[];
  directories?: ChangeTreeDirectory[];
  treeLabel: string;
  listLabel: string;
  selectedId?: string | null;
  onSelect: (file: ChangeTreeFile) => void;
  count?: number | null;
  countLabel?: string;
  /** Reports explicit directories whose contents are needed; list view requests every known directory. */
  onExpandedDirectoriesChange?: (directories: ChangeTreeDirectory[], view: "tree" | "list") => void;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<"tree" | "list">("tree");
  const [expansion, setExpansion] = useState(() => ({ defaultExpanded: false, exceptions: new Set<string>() }));
  const directoryIds = useMemo(() => {
    const ids = new Set<string>();
    for (const file of files) {
      for (let depth = 1; depth < file.segments.length; depth++) ids.add(changeDirectoryId(file.segments.slice(0, depth)));
    }
    for (const directory of directories) {
      for (let depth = 1; depth <= directory.segments.length; depth++) ids.add(changeDirectoryId(directory.segments.slice(0, depth)));
    }
    return ids;
  }, [files, directories]);
  const collapsed = useMemo(() => {
    const ids = new Set<string>();
    for (const id of directoryIds) {
      const expanded = expansion.defaultExpanded ? !expansion.exceptions.has(id) : expansion.exceptions.has(id);
      if (!expanded) ids.add(id);
    }
    return ids;
  }, [directoryIds, expansion]);
  const expandedDirectories = useMemo(() => directories.filter((directory) => {
    if (view === "list") return true;
    for (let depth = 1; depth <= directory.segments.length; depth++) {
      if (collapsed.has(changeDirectoryId(directory.segments.slice(0, depth)))) return false;
    }
    return true;
  }), [directories, view, collapsed]);
  const lastReported = useRef<{ ids: string[]; view: "tree" | "list" } | null>(null);
  useEffect(() => {
    if (!onExpandedDirectoriesChange) { lastReported.current = null; return; }
    const previous = lastReported.current;
    if (previous?.view === view && previous.ids.length === expandedDirectories.length
      && expandedDirectories.every((directory, index) => directory.id === previous.ids[index])) return;
    lastReported.current = { ids: expandedDirectories.map((directory) => directory.id), view };
    onExpandedDirectoriesChange(expandedDirectories, view);
  }, [expandedDirectories, view, onExpandedDirectoriesChange]);
  let allExpanded = directoryIds.size > 0;
  for (const id of directoryIds) {
    if (collapsed.has(id)) { allExpanded = false; break; }
  }
  const action = t(allExpanded ? "tree.collapseAll" : "tree.expandAll");
  return <>
    <div className="files-toolbar">
      <span className="files-count">{countLabel ?? (count === null ? t("tree.files") : t("tree.fileCount", { count }))}</span>
      {actions}
      <div className="files-view-controls">
        <Tooltip content={action} trigger={<button type="button" className="files-expand" aria-label={action}
          disabled={view !== "tree" || directoryIds.size === 0 || count === null}
          onClick={() => setExpansion({ defaultExpanded: !allExpanded, exceptions: new Set() })}>
          {allExpanded ? <CollapseAllIcon size={16} aria-hidden="true" /> : <ExpandAllIcon size={16} aria-hidden="true" />}
        </button>} />
        <Tooltip content={t(view === "tree" ? "tree.switchList" : "tree.switchTree")}
          trigger={<button type="button" className="files-view-toggle"
            aria-label={t(view === "tree" ? "tree.switchList" : "tree.switchTree")}
            onClick={() => setView((current) => current === "tree" ? "list" : "tree")}>
            {view === "tree" ? <FolderIcon size={16} aria-hidden="true" /> : <FileIcon size={16} aria-hidden="true" />}
            <span>{t(view === "tree" ? "tree.tree" : "tree.list")}</span>
          </button>} />
      </div>
    </div>
    {children ?? <ChangeTree files={files} directories={directories} summaryFiles={summaryFiles} label={view === "tree" ? treeLabel : listLabel} view={view}
      selectedId={selectedId} onSelect={onSelect} directoryExpansion={{ collapsed, onToggle: (id) => setExpansion((current) => {
        const exceptions = new Set(current.exceptions);
        if (!exceptions.delete(id)) exceptions.add(id);
        return { ...current, exceptions };
      }) }} />}
  </>;
}
