/** Renders verified snapshots and Git hunks in virtualized aligned panes with stable reading anchors. */

import { useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { ReviewIdentity, ReviewText, TextHunk } from "../../../contracts/diff";
import { codeTheme, type CodeTheme, type ReviewMode, type LineMode } from "../../appearance";
import type { Token, ChangedRange } from "../highlighting/highlighting";
import { sourceLines } from "../highlighting/highlighting";
import { useCodeHighlight } from "../highlighting/useCodeHighlight";
import { useSourceLayout } from "./useSourceLayout";
import { ResizeDivider } from "../../../ui/resize/ResizeDivider";
import { constrainResize, dividerSize } from "../../../ui/resize/resizeGeometry";
import { setDiffRatio, usePanelLayout } from "../../../ui/resize/panelLayout";

type TextReview = ReviewText & Pick<ReviewIdentity, "fromAbsent" | "toAbsent"> & {
  displayPath: string;
  from?: ReviewIdentity["from"];
  to?: ReviewIdentity["to"];
};
type SourceLine = TextHunk["lines"][number] & { number: number };
interface ReadingAnchor { rowIndex: number; offset: number }
interface ReadingRow { contentKey: string }
interface ComparisonRow extends ReadingRow {
  key: string;
  kind: "source" | "hunk" | "newline";
  old: SourceLine | null;
  new: SourceLine | null;
  heading: string;
  top: number;
  height: number;
}

const READING_CONTEXT_LINES = 32;
const SOURCE_ROW_HEIGHT = 22;
const HUNK_ROW_HEIGHT = 26;
const OVERSCAN_ROWS = 12;

/** Aligns unchanged ends, then uses bounded mutual context; ambiguous duplicate identities reset. */
function survivingReadingRow(previous: ReadingRow[], next: ReadingRow[], rowIndex: number): number | null {
  let oldEnd = previous.length - 1;
  let newEnd = next.length - 1;
  // Align the unchanged tail first: a prepended duplicate must not steal a later copy.
  while (oldEnd >= 0 && newEnd >= 0 && previous[oldEnd].contentKey === next[newEnd].contentKey) {
    oldEnd--;
    newEnd--;
  }
  if (rowIndex > oldEnd) return newEnd + rowIndex - oldEnd;

  let start = 0;
  while (start <= oldEnd && start <= newEnd && previous[start].contentKey === next[start].contentKey) start++;
  if (rowIndex < start) return rowIndex;

  function contextScore(oldIndex: number, newIndex: number) {
    let score = 0;
    for (let direction = -1; direction <= 1; direction += 2) {
      for (let distance = 1; distance <= READING_CONTEXT_LINES; distance++) {
        const oldNeighbor = previous[oldIndex + distance * direction];
        const newNeighbor = next[newIndex + distance * direction];
        if (!oldNeighbor || !newNeighbor || oldNeighbor.contentKey !== newNeighbor.contentKey) break;
        score++;
      }
    }
    return score;
  }

  const contentKey = previous[rowIndex].contentKey;
  let candidate = -1;
  let bestScore = -1;
  let nextCopies = 0;
  let tied = false;
  for (let index = start; index <= newEnd; index++) {
    if (next[index].contentKey !== contentKey) continue;
    nextCopies++;
    const score = contextScore(rowIndex, index);
    if (score > bestScore) { candidate = index; bestScore = score; tied = false; }
    else if (score === bestScore) tied = true;
  }
  if (candidate < 0 || tied) return null;

  let oldCopies = 0;
  let reverseBest = -1;
  let reverseScore = -1;
  let reverseTied = false;
  for (let index = start; index <= oldEnd; index++) {
    if (previous[index].contentKey !== contentKey) continue;
    oldCopies++;
    const score = contextScore(index, candidate);
    if (score > reverseScore) { reverseBest = index; reverseScore = score; reverseTied = false; }
    else if (score === reverseScore) reverseTied = true;
  }
  if (oldCopies === 1 && nextCopies === 1) return candidate;
  // A duplicate must be a mutual, unambiguous context match, not merely the same occurrence number.
  return bestScore > 0 && !reverseTied && reverseBest === rowIndex ? candidate : null;
}

/** Maps headers to their next source line and compares line numbers within one endpoint only. */
function modeReadingRow(previous: ComparisonRow[], next: ComparisonRow[], rowIndex: number): number | null {
  let source = previous[rowIndex];
  if (source?.kind === "hunk") {
    for (let index = rowIndex + 1; index < previous.length; index++) {
      if (previous[index].kind === "source") { source = previous[index]; break; }
    }
  }
  if (!source) return null;
  const side = source.new ? "new" : "old";
  const number = source[side]?.number;
  if (number === undefined) return null;
  let candidate: number | null = null;
  let distance = Infinity;
  for (let index = 0; index < next.length; index++) {
    const row = next[index];
    const line = row[side];
    if (row.kind !== "source" || !line) continue;
    const difference = Math.abs(line.number - number);
    if (difference < distance) { distance = difference; candidate = index; }
    if (difference === 0) break;
  }
  return candidate;
}

function comparisonRows(review: TextReview | null, mode: ReviewMode) {
  const rows: ComparisonRow[] = [];
  const occurrences = new Map<string, number>();
  let height = 0;

  function append(kind: ComparisonRow["kind"], old: SourceLine | null, next: SourceLine | null, heading = "") {
    // Line numbers move on refresh; content and side ownership identify the reading row.
    const contentKey = JSON.stringify([kind, old && [old.kind, old.text, Boolean(old.noFinalNewline)],
      next && [next.kind, next.text, Boolean(next.noFinalNewline)], heading]);
    const occurrence = occurrences.get(contentKey) ?? 0;
    occurrences.set(contentKey, occurrence + 1);
    const rowHeight = kind === "hunk" ? HUNK_ROW_HEIGHT : SOURCE_ROW_HEIGHT;
    rows.push({ kind, old, new: next, heading, contentKey, key: `${contentKey}:${occurrence}`, top: height, height: rowHeight });
    height += rowHeight;
  }

  function appendSource(old: SourceLine | null, next: SourceLine | null) {
    append("source", old, next);
    if (old?.noFinalNewline || next?.noFinalNewline) append("newline", old?.noFinalNewline ? old : null, next?.noFinalNewline ? next : null);
  }

  const oldLines = mode === "full" && review ? sourceLines(review.fromContent) : [];
  const newLines = mode === "full" && review ? sourceLines(review.toContent) : [];
  let oldCursor = 0;
  let newCursor = 0;
  function appendContext(oldEnd: number, newEnd: number) {
    while (oldCursor < oldEnd || newCursor < newEnd) {
      const old = oldCursor < oldEnd ? { kind: "context" as const, text: oldLines[oldCursor], number: ++oldCursor,
        noFinalNewline: oldCursor === oldLines.length && !review!.fromContent.endsWith("\n") } : null;
      const next = newCursor < newEnd ? { kind: "context" as const, text: newLines[newCursor], number: ++newCursor,
        noFinalNewline: newCursor === newLines.length && !review!.toContent.endsWith("\n") } : null;
      appendSource(old, next);
    }
  }

  for (const hunk of review?.hunks ?? []) {
    if (mode === "full") appendContext(hunk.oldCount === 0 ? hunk.oldStart : hunk.oldStart - 1,
      hunk.newCount === 0 ? hunk.newStart : hunk.newStart - 1);
    else append("hunk", null, null, `@@ -${hunk.oldStart},${hunk.oldCount} +${hunk.newStart},${hunk.newCount} @@`);
    let oldNumber = hunk.oldStart;
    let newNumber = hunk.newStart;
    let removals: SourceLine[] = [];
    let additions: SourceLine[] = [];
    function flushChanges() {
      for (let index = 0; index < Math.max(removals.length, additions.length); index++) {
        appendSource(removals[index] ?? null, additions[index] ?? null);
      }
      removals = [];
      additions = [];
    }
    for (const line of hunk.lines) {
      if (line.kind === "context") {
        flushChanges();
        appendSource({ ...line, number: oldNumber++ }, { ...line, number: newNumber++ });
      } else if (line.kind === "removal") removals.push({ ...line, number: oldNumber++ });
      else additions.push({ ...line, number: newNumber++ });
    }
    flushChanges();
    oldCursor = hunk.oldCount === 0 ? hunk.oldStart : hunk.oldStart - 1 + hunk.oldCount;
    newCursor = hunk.newCount === 0 ? hunk.newStart : hunk.newStart - 1 + hunk.newCount;
  }
  if (mode === "full") appendContext(oldLines.length, newLines.length);
  return { rows, height };
}

/** Finds a visible row without measuring or traversing the rendered DOM. */
function rowAtOffset(rows: ComparisonRow[], offset: number) {
  let start = 0;
  let end = rows.length;
  while (start < end) {
    const middle = (start + end) >>> 1;
    if (rows[middle].top + rows[middle].height <= offset) start = middle + 1;
    else end = middle;
  }
  return Math.min(start, Math.max(0, rows.length - 1));
}

/** Color and word emphasis do not change glyph weight or source geometry. */
function renderSource(text: string, tokens?: Token[], changes: ChangedRange[] = []) {
  if (text === "") return " ";
  const tokenText = tokens?.map((token) => token.text).join("");
  if (tokenText !== text && changes.length === 0) return text;
  const safeTokens = tokenText === text ? tokens! : [{ text }];
  let offset = 0;
  return safeTokens.flatMap((token, tokenIndex) => {
    const start = offset;
    const end = start + token.text.length;
    offset = end;
    const boundaries = [start, end];
    for (const range of changes) {
      if (range.start > start && range.start < end) boundaries.push(range.start);
      if (range.end > start && range.end < end) boundaries.push(range.end);
    }
    boundaries.sort((a, b) => a - b);
    return boundaries.slice(1).map((boundary, index) => {
      const from = boundaries[index];
      const changed = changes.some((range) => range.start <= from && range.end > from);
      return <span key={`${tokenIndex}:${index}`} style={token.color ? { color: token.color } : undefined}
        className={changed ? "diff-inline-change" : undefined}>{text.slice(from, boundary)}</span>;
    });
  });
}

export function TextDiff({ review, updating = false, oldEndpointLabel, newEndpointLabel, mode = "changes", theme = "plain", lineMode = "scroll" }: {
  review: TextReview | null;
  updating?: boolean;
  oldEndpointLabel?: string;
  newEndpointLabel?: string;
  mode?: ReviewMode;
  theme?: CodeTheme;
  lineMode?: LineMode;
}) {
  const comparisonPane = useRef<HTMLDivElement>(null);
  const oldPaneId = useId();
  const { diffRatio } = usePanelLayout();
  const [comparisonWidth, setComparisonWidth] = useState(0);
  const usableWidth = Math.max(0, comparisonWidth - dividerSize);
  const minimumWidth = Math.min(128, usableWidth * 0.4);
  const oldWidth = constrainResize(usableWidth * diffRatio, minimumWidth, usableWidth - minimumWidth);
  const visibleRatio = usableWidth > 0 ? oldWidth / usableWidth : diffRatio;
  useLayoutEffect(() => {
    const element = comparisonPane.current;
    if (!element) return;
    const measure = () => setComparisonWidth(element.getBoundingClientRect().width);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const oldPane = useRef<HTMLDivElement>(null);
  const newPane = useRef<HTMLDivElement>(null);
  const anchor = useRef<ReadingAnchor | null>(null);
  const hasRenderedText = useRef(false);
  const sharedScrollTop = useRef(0);
  const renderedRows = useRef<ComparisonRow[]>([]);
  const [notice, setNotice] = useState(false);
  const [visibleRange, setVisibleRange] = useState({ start: 0, end: 40 });
  const horizontalReading = useRef({ old: 0, new: 0 });
  const previousLineMode = useRef(lineMode);
  const previousMode = useRef(mode);
  const comparison = useMemo(() => comparisonRows(review, mode), [review, mode]);
  const { layout, sourceWidths } = useSourceLayout(comparison, oldPane, newPane, lineMode, visibleRange);
  const highlight = useCodeHighlight(review, theme);
  const { rows } = layout;

  function updateVisibleRange(element: HTMLDivElement) {
    const start = Math.max(0, rowAtOffset(rows, element.scrollTop) - OVERSCAN_ROWS);
    const end = Math.min(rows.length, rowAtOffset(rows, element.scrollTop + (element.clientHeight || 440)) + OVERSCAN_ROWS + 1);
    setVisibleRange((previous) => previous.start === start && previous.end === end ? previous : { start, end });
  }

  useLayoutEffect(() => {
    const element = oldPane.current;
    if (!element || !review) return;
    if (hasRenderedText.current && anchor.current) {
      const readingAnchor = anchor.current;
      const survivingIndex = previousMode.current !== mode
        ? modeReadingRow(renderedRows.current, rows, readingAnchor.rowIndex)
        : survivingReadingRow(renderedRows.current, rows, readingAnchor.rowIndex);
      if (survivingIndex !== null) {
        const row = rows[survivingIndex];
        // A shorter wrapped row cannot retain an offset beyond its last visual line.
        const offset = -readingAnchor.offset < row.height
          ? readingAnchor.offset : -Math.max(0, row.height - SOURCE_ROW_HEIGHT);
        anchor.current = { rowIndex: survivingIndex, offset };
        const scrollTop = row.top - offset;
        element.scrollTop = scrollTop;
        if (newPane.current) newPane.current.scrollTop = scrollTop;
        sharedScrollTop.current = element.scrollTop;
        setNotice(false);
      } else {
        for (const pane of [element, newPane.current]) {
          if (pane) { pane.scrollTop = 0; pane.scrollLeft = 0; }
        }
        anchor.current = null;
        horizontalReading.current = { old: 0, new: 0 };
        sharedScrollTop.current = 0;
        setNotice(true);
      }
    }
    hasRenderedText.current = true;
    previousMode.current = mode;
    renderedRows.current = rows;
    updateVisibleRange(element);
  }, [review, rows, mode]);

  useLayoutEffect(() => {
    if (previousLineMode.current !== lineMode && lineMode === "scroll") {
      if (oldPane.current) oldPane.current.scrollLeft = horizontalReading.current.old;
      if (newPane.current) newPane.current.scrollLeft = horizontalReading.current.new;
    }
    previousLineMode.current = lineMode;
  }, [lineMode]);

  useLayoutEffect(() => {
    const element = oldPane.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => updateVisibleRange(element));
    observer.observe(element);
    return () => observer.disconnect();
  }, [rows]);

  function rememberReadingAnchor(element: HTMLDivElement, other: HTMLDivElement | null) {
    if (lineMode === "scroll") horizontalReading.current = {
      old: oldPane.current?.scrollLeft ?? 0, new: newPane.current?.scrollLeft ?? 0,
    };
    if (!review || element.scrollTop === sharedScrollTop.current) return;
    sharedScrollTop.current = element.scrollTop;
    if (other && other.scrollTop !== element.scrollTop) other.scrollTop = element.scrollTop;
    if (element.scrollTop === 0 || rows.length === 0) anchor.current = null;
    else {
      const rowIndex = rowAtOffset(rows, element.scrollTop);
      anchor.current = { rowIndex, offset: rows[rowIndex].top - element.scrollTop };
    }
    updateVisibleRange(element);
  }

  function renderPane(side: "old" | "new") {
    const width = sourceWidths[side];
    return <div id={side === "old" ? oldPaneId : undefined} className={`diff-source-pane ${side}`} ref={side === "old" ? oldPane : newPane}
      role="region" aria-label={`${side === "old" ? "Old" : "New"} source ${mode === "full" ? "file" : "hunks"}`} tabIndex={0}
      onScroll={(event) => rememberReadingAnchor(event.currentTarget, side === "old" ? newPane.current : oldPane.current)}>
      <div className="diff-source-content" style={{ height: layout.height, width: lineMode === "wrap" ? "100%" : `max(100%, ${width}px)` }}>
        {rows.slice(visibleRange.start, visibleRange.end).map((row, index) => {
          const line = row[side];
          const rowIndex = visibleRange.start + index;
          if (row.kind === "hunk") return <div key={row.key} className="diff-hunk" style={{ top: row.top, height: row.height }}>{row.heading}</div>;
          if (row.kind === "newline") return <div key={row.key} className="diff-no-newline" style={{ top: row.top, height: row.height }}>
            {line ? <span data-source-measure={rowIndex}>{`No final newline (${side} side)`}</span> : null}
          </div>;
          return <div key={row.key} className={`diff-line ${line?.kind ?? "absent"}`} style={{ top: row.top, height: row.height }}
            data-reading-index={rowIndex} aria-label={line ? `${side === "old" ? "Old" : "New"} ${line.kind} line ${line.number}` : `No ${side} line`}>
            {line ? <>
              <span className="diff-line-number" aria-label={`${side === "old" ? "Old" : "New"} line number`}>{line.number}</span>
              <span className="diff-line-sign" aria-hidden="true">{line.kind === "addition" ? "+" : line.kind === "removal" ? "−" : " "}</span>
              <code data-source-measure={rowIndex}>{renderSource(line.text, highlight.code?.[side][line.number - 1],
                highlight.code?.[side === "old" ? "oldChanges" : "newChanges"][line.number])}</code>
            </> : null}
          </div>;
        })}
      </div>
    </div>;
  }

  const oldLabel = oldEndpointLabel ?? (review?.fromAbsent || review?.from === "absent" ? "Absent" : review?.from === "HEAD" ? "HEAD" : "Index");
  const newLabel = newEndpointLabel ?? (review?.toAbsent ? "Absent" : review?.to === "index" ? "Index" : "Working files");
  const codePalette = codeTheme(theme).preview;
  return <>
    {notice && review ? <p className="diff-reading-notice" role="status">Reading position reset: the previously visible line changed.</p> : null}
    {highlight.error && theme !== "plain" && review ? <p className="diff-reading-notice" role="status">Syntax highlighting unavailable; source remains readable.</p> : null}
    <div ref={comparisonPane} className="diff-comparison" hidden={!review}
      style={{ "--diff-columns": `minmax(0, ${visibleRatio}fr) ${dividerSize}px minmax(0, ${1 - visibleRatio}fr)` } as CSSProperties}>
      <div className="diff-source-headings"><span>Old · {oldLabel}</span><span>New · {newLabel}</span></div>
      <ResizeDivider label="Resize old and new versions" controls={oldPaneId} root={comparisonPane}
        value={oldWidth} min={minimumWidth} max={usableWidth - minimumWidth} snap
        className="diff-divider" valueText={`${Math.round(visibleRatio * 100)}% old, ${Math.round((1 - visibleRatio) * 100)}% new`}
        onChange={(value) => {
          const available = (comparisonPane.current?.getBoundingClientRect().width ?? 0) - dividerSize;
          if (available > 0) setDiffRatio(value / available);
        }} />
      <div className={`diff-viewport${lineMode === "wrap" ? " is-wrapped" : ""}`}
        style={{ "--code-bg": codePalette.background, "--code-ink": codePalette.foreground,
          "--code-muted": codePalette.muted, "--code-add-bg": codePalette.addition, "--code-remove-bg": codePalette.removal,
          "--code-inline-add-bg": codePalette.inlineAdd, "--code-inline-remove-bg": codePalette.inlineRemove } as CSSProperties}
        role="region" aria-label={updating ? "Last verified file comparison" : "Read-only file comparison"}
        aria-description={`${mode === "full" ? "Complete verified file snapshots." : "Changed hunks only, not complete files."} Vertical scrolling is shared; ${lineMode === "wrap" ? "long source lines wrap within each pane." : "each source pane scrolls horizontally independently."}`}>
        {renderPane("old")}{renderPane("new")}
      </div>
    </div>
  </>;
}
