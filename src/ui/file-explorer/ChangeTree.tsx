/** Windows native-segment file rows while retaining logical keyboard order and sticky ancestry. */

import { memo, useCallback, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type RefObject } from "react";
import { defaultRangeExtractor, observeElementRect, useVirtualizer, type Range } from "@tanstack/react-virtual";
import { buildChangeHierarchy, flattenChangeRows, summarizeDirectoryChanges, type ChangeTreeDirectory, type ChangeTreeFile, type ChangeTreeRow, type ChangeTreeSummaryProps } from "./changeTreeRows";
import { ChevronDownIcon, ChevronRightIcon } from "../icons";
import { TreeEntryIcon } from "../file-icons/TreeEntryIcon";
import { Tooltip } from "../Tooltip";

export { changeDirectoryId } from "./changeTreeRows";
export type { ChangeTreeDirectory, ChangeTreeFile } from "./changeTreeRows";

const ROW_HEIGHT = 28;
const NO_DIRECTORIES: ChangeTreeDirectory[] = [];

interface TreeProps extends ChangeTreeSummaryProps {
  files: ChangeTreeFile[];
  directories?: ChangeTreeDirectory[];
  label: string;
  selectedId?: string | null;
  onSelect?: (file: ChangeTreeFile) => void;
  view?: "tree" | "list";
  /** Controlled expansion uses segment-derived presentation IDs, not native directory authority. */
  directoryExpansion?: { collapsed: ReadonlySet<string>; onToggle: (id: string) => void };
  /** Inline trees share their owner's scroller and never pin folder headers. */
  virtualScrollRef?: RefObject<HTMLElement | null>;
  /** Optional known offset in the external scroller; otherwise measured from the tree's origin. */
  virtualScrollMargin?: number;
}

export function ChangeTree({ files, directories = NO_DIRECTORIES, summaryFiles = files, label, selectedId, onSelect, view = "tree", directoryExpansion, virtualScrollRef, virtualScrollMargin }: TreeProps) {
  const hierarchy = useMemo(() => buildChangeHierarchy(files, directories), [files, directories]);
  const directorySummaries = useMemo(() => summaryFiles === null ? null : summarizeDirectoryChanges(summaryFiles), [summaryFiles]);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const collapsed = useMemo(() => {
    if (directoryExpansion) return directoryExpansion.collapsed;
    const ids = new Set<string>();
    const visit = (directory: typeof hierarchy) => {
      for (const child of directory.directories.values()) {
        if (!expanded.has(child.id)) ids.add(child.id);
        visit(child);
      }
    };
    visit(hierarchy);
    return ids;
  }, [hierarchy, expanded, directoryExpansion?.collapsed]);
  const toggleDirectory = useCallback((id: string) => setExpanded((current) => {
    const next = new Set(current);
    if (!next.delete(id)) next.add(id);
    return next;
  }), []);
  const onToggle = directoryExpansion?.onToggle ?? toggleDirectory;
  const rows = useMemo(() => flattenChangeRows(hierarchy, files, view, collapsed), [hierarchy, files, view, collapsed]);
  const rowIndices = useMemo(() => new Map(rows.map((row, index) => [row.key, index])), [rows]);
  const selectable = onSelect !== undefined;
  const interactiveBoundaries = useMemo(() => {
    if (rows.length === 0) return [];
    if (selectable) return [0, rows.length - 1];
    const first = rows.findIndex((row) => !row.file);
    if (first < 0) return [];
    let last = rows.length - 1;
    while (rows[last].file) last--;
    return [first, last];
  }, [rows, selectable]);
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLUListElement>(null);
  const [measuredMargin, setMeasuredMargin] = useState(0);
  const margin = virtualScrollRef ? virtualScrollMargin ?? measuredMargin : 0;
  const sticky = view === "tree" && !virtualScrollRef;
  const [focusedKey, setFocusedKey] = useState<string | null>(null);
  const focusedIndex = focusedKey === null ? undefined : rowIndices.get(focusedKey);
  const pendingFocus = useRef<string | null>(null);
  const focusInside = useRef(false);
  const previousRows = useRef(rows);
  const savedScroll = useRef(0);
  const getItemKey = useCallback((index: number) => rows[index].key, [rows]);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => virtualScrollRef?.current ?? containerRef.current,
    getItemKey,
    estimateSize: () => ROW_HEIGHT,
    initialRect: { width: 280, height: 480 },
    scrollMargin: margin,
    overscan: 6,
    // A hidden pane keeps its last viewport; zero-layout samples retain the bounded initial range.
    observeElementRect: (instance, callback) => observeElementRect(instance, (rect) => {
      if (rect.height > 0) callback(rect);
    }),
    rangeExtractor: useCallback((range: Range) => {
      const indices = new Set(defaultRangeExtractor(range));
      // Logical boundaries keep ordinary Tab entry/exit correct even after pointer scrolling.
      for (const index of interactiveBoundaries) indices.add(index);
      if (focusedIndex !== undefined) indices.add(focusedIndex);
      if (sticky) {
        let index: number | null = range.startIndex;
        while (index !== null && rows[index]) {
          indices.add(index);
          index = rows[index].parent;
        }
      }
      return [...indices].sort((left, right) => left - right);
    }, [rows, focusedIndex, sticky, interactiveBoundaries]),
  });
  const visibleRows = virtualizer.getVirtualItems();
  const scrollOffset = Math.max(0, (virtualizer.scrollOffset ?? 0) - margin);

  useLayoutEffect(() => {
    const container = containerRef.current;
    const content = contentRef.current;
    const scroller = virtualScrollRef?.current ?? container;
    if (!container || !content || !scroller) return;
    let hidden = scroller.clientHeight === 0;
    function measure() {
      if (!container || !content || !scroller) return;
      if (virtualScrollRef && virtualScrollMargin === undefined) {
        const offset = content.getBoundingClientRect().top - scroller.getBoundingClientRect().top - scroller.clientTop + scroller.scrollTop;
        setMeasuredMargin((current) => Math.abs(current - offset) < 0.5 ? current : offset);
      } else if (!virtualScrollRef) {
        if (hidden && scroller.clientHeight > 0) scroller.scrollTop = savedScroll.current;
        hidden = scroller.clientHeight === 0;
      }
    }
    measure();
    const resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    const mutations = virtualScrollRef && typeof MutationObserver !== "undefined" ? new MutationObserver(measure) : null;
    let element: HTMLElement | null = content;
    while (element) {
      resize?.observe(element);
      mutations?.observe(element, { attributes: true, attributeFilter: ["style", "class"] });
      if (element === scroller) break;
      element = element.parentElement;
    }
    return () => { resize?.disconnect(); mutations?.disconnect(); };
  }, [virtualScrollRef, virtualScrollMargin]);

  useLayoutEffect(() => {
    const previous = previousRows.current;
    if (previous === rows) return;
    previousRows.current = rows;
    const scroller = virtualScrollRef?.current ?? containerRef.current;
    if (!scroller) return;
    if (focusInside.current && focusedKey !== null && !rowIndices.has(focusedKey)) {
      let oldIndex: number | null = previous.findIndex((row) => row.key === focusedKey);
      while (oldIndex !== null && oldIndex >= 0) {
        const old: ChangeTreeRow = previous[oldIndex];
        const replacement = rowIndices.get(old.key);
        if (replacement !== undefined) {
          pendingFocus.current = rows[replacement].key;
          setFocusedKey(rows[replacement].key);
          break;
        }
        oldIndex = old.parent;
      }
      if (pendingFocus.current === null) {
        const fallback = rows.find((row) => !row.file || onSelect);
        if (fallback) {
          pendingFocus.current = fallback.key;
          setFocusedKey(fallback.key);
        } else {
          setFocusedKey(null);
          containerRef.current?.focus({ preventScroll: true });
        }
      }
    }
    // Preserve the leading native-segment anchor when progressive pages insert preceding rows.
    // A shorter replacement may already have clamped the DOM scrollTop before this layout effect.
    const offset = (virtualScrollRef ? scroller.scrollTop : savedScroll.current) - margin;
    if (offset < 0 || offset >= previous.length * ROW_HEIGHT) return;
    let oldIndex: number | null = Math.floor(offset / ROW_HEIGHT);
    while (oldIndex !== null) {
      const replacement = rowIndices.get(previous[oldIndex].key);
      if (replacement !== undefined) {
        const next = margin + replacement * ROW_HEIGHT + offset % ROW_HEIGHT;
        if (!virtualScrollRef) savedScroll.current = next;
        if (Math.abs(next - scroller.scrollTop) >= 1) virtualizer.scrollToOffset(next);
        break;
      }
      oldIndex = previous[oldIndex].parent;
    }
  }, [rows, rowIndices, focusedKey, margin, virtualizer, virtualScrollRef, onSelect]);

  const revealRow = useCallback((index: number) => {
    const scroller = virtualScrollRef?.current ?? containerRef.current;
    if (!scroller || scroller.clientHeight === 0) return;
    const row = rows[index];
    const naturalTop = margin + index * ROW_HEIGHT;
    const top = sticky && !row.file && row.expanded
      ? Math.max(naturalTop, Math.min(scroller.scrollTop + row.depth * ROW_HEIGHT, margin + (row.end - 1) * ROW_HEIGHT))
      : naturalTop;
    const inset = sticky ? row.depth * ROW_HEIGHT : 0;
    if (top < scroller.scrollTop + inset) virtualizer.scrollToOffset(Math.max(0, top - inset));
    else if (top + ROW_HEIGHT > scroller.scrollTop + scroller.clientHeight) virtualizer.scrollToOffset(top + ROW_HEIGHT - scroller.clientHeight);
  }, [virtualScrollRef, rows, margin, sticky, virtualizer]);

  const focusRow = useCallback((index: number) => {
    setFocusedKey(rows[index].key);
    revealRow(index);
  }, [rows, revealRow]);

  useLayoutEffect(() => {
    if (pendingFocus.current === null) return;
    const index = rowIndices.get(pendingFocus.current);
    if (index === undefined) return;
    const button = contentRef.current?.querySelector<HTMLButtonElement>(`[data-tree-index="${index}"] button`);
    if (button) {
      button.focus({ preventScroll: true });
      pendingFocus.current = null;
    }
  }, [visibleRows, rowIndices]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Tab" || event.altKey || event.ctrlKey || event.metaKey) return;
    const element = (event.target as HTMLElement).closest<HTMLElement>("[data-tree-index]");
    if (!element) return;
    const direction = event.shiftKey ? -1 : 1;
    let index = Number(element.dataset.treeIndex) + direction;
    while (index >= 0 && index < rows.length && rows[index].file && !onSelect) index += direction;
    if (index < 0 || index >= rows.length) return;
    event.preventDefault();
    pendingFocus.current = rows[index].key;
    setFocusedKey(rows[index].key);
    revealRow(index);
  }

  return <div ref={containerRef} tabIndex={-1} className={`changes-tree${view === "tree" ? " changes-tree-hierarchy" : ""}${virtualScrollRef ? " changes-tree-external" : ""}`}
    onKeyDown={handleKeyDown}
    onScroll={(event) => { if (event.currentTarget.clientHeight > 0) savedScroll.current = event.currentTarget.scrollTop; }}
    onFocus={() => { focusInside.current = true; }}
    onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) { focusInside.current = false; setFocusedKey(null); } }}>
    <ul ref={contentRef} className="changes-tree-content" aria-label={label} style={{ height: virtualizer.getTotalSize() }}>
      {visibleRows.map((virtualRow) => {
        const row = rows[virtualRow.index];
        const naturalTop = virtualRow.start - margin;
        const top = sticky && !row.file && row.expanded
          ? Math.max(naturalTop, Math.min(scrollOffset + row.depth * ROW_HEIGHT, (row.end - 1) * ROW_HEIGHT)) : naturalTop;
        const status = row.file ? row.file.status || "Status unavailable"
          : directorySummaries === null ? "Status unavailable" : directorySummaries.get(row.item.id) ?? "No known changes";
        return <MountedChangeRow key={row.key} row={row} index={virtualRow.index} count={rows.length}
          top={top} sticky={sticky} selected={row.file !== undefined && selectedId === row.file.id}
          status={status} onSelect={onSelect} onToggle={onToggle} onFocus={focusRow} />;
      })}
    </ul>
  </div>;
}

interface MountedChangeRowProps {
  row: ChangeTreeRow;
  index: number;
  count: number;
  top: number;
  sticky: boolean;
  selected: boolean;
  status: string;
  onSelect?: (file: ChangeTreeFile) => void;
  onToggle: (id: string) => void;
  onFocus: (index: number) => void;
}

const MountedChangeRow = memo(function MountedChangeRow({ row, index, count, top, sticky, selected, status, onSelect, onToggle, onFocus }: MountedChangeRowProps) {
  const name = row.item.segments[row.item.segments.length - 1];
  const style = { top, "--row-depth": row.depth, zIndex: sticky && !row.file ? 1000 - row.depth : undefined } as CSSProperties;
  const content = row.file ? <>
    <TreeEntryIcon kind="file" name={name} /><span>{name}</span>
    <span className={`change-marker${row.file.unsupported ? " unsupported" : ""}`} aria-label={row.file.status}>{row.file.marker}</span>
  </> : null;
  return <li data-tree-index={index} className={`change-virtual-row ${row.file ? "change-file" : "change-directory"}`}
    aria-posinset={index + 1} aria-setsize={count} style={style}
    onFocus={() => onFocus(index)}>
    <Tooltip content={<><div>{row.item.displayPath}</div><div>{status}</div></>}
      trigger={row.file ? onSelect ? <button type="button" className="change-file-name" aria-label={`Review ${row.file.displayPath}`}
        aria-pressed={selected} onClick={() => onSelect(row.file!)}>{content}</button>
        : <div className="change-file-name">{content}</div>
        : <button type="button" className="directory-toggle" aria-expanded={row.expanded}
          aria-label={`${row.expanded ? "Collapse" : "Expand"} ${row.item.displayPath}`}
          onClick={() => onToggle(row.item.id)}>
          {row.expanded ? <ChevronDownIcon aria-hidden="true" /> : <ChevronRightIcon aria-hidden="true" />}
          <TreeEntryIcon kind="folder" name={name} expanded={row.expanded} /><span>{name}</span>
        </button>} />
  </li>;
});
