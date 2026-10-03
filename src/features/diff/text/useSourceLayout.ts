/** Owns font-aware source widths and aligned variable-height rows without expanding the virtual DOM window. */
import { useLayoutEffect, useMemo, useState, type RefObject } from "react";
import type { LineMode } from "../../appearance";

export interface SourceLayoutRow {
  key: string;
  kind: "source" | "hunk" | "newline";
  old: { text: string } | null;
  new: { text: string } | null;
  heading: string;
  top: number;
  height: number;
}
export interface SourceLayout<Row extends SourceLayoutRow> { rows: Row[]; height: number }
interface Widths { old: number; new: number }
interface Measurements {
  scope: symbol;
  available: Widths;
  maximum: Widths;
  rows: Map<string, Widths>;
}
interface MeasuredHeights { scope: symbol | null; rows: Map<string, number> }
const lineHeight = 22;

/** Uses loaded font glyph advances, including fallback glyphs, and CSS tab stops. */
function sourceTextWidth(context: CanvasRenderingContext2D, text: string) {
  const tabWidth = context.measureText(" ").width * 4;
  let width = 0;
  let inkRight = 0;
  const segments = text.split("\t");
  for (let index = 0; index < segments.length; index++) {
    const metrics = context.measureText(segments[index]);
    inkRight = Math.max(inkRight, width + (metrics.actualBoundingBoxRight || metrics.width));
    width += metrics.width;
    if (index < segments.length - 1 && tabWidth > 0) width = (Math.floor(width / tabWidth) + 1) * tabWidth;
  }
  return Math.ceil(Math.max(width, inkRight));
}

export function useSourceLayout<Row extends SourceLayoutRow>(base: SourceLayout<Row>,
  oldPane: RefObject<HTMLDivElement | null>, newPane: RefObject<HTMLDivElement | null>, lineMode: LineMode,
  visibleRange: { start: number; end: number }): { layout: SourceLayout<Row>; sourceWidths: Widths } {
  const [measurements, setMeasurements] = useState<Measurements | null>(null);
  const [heights, setHeights] = useState<MeasuredHeights>(() => ({ scope: null, rows: new Map() }));

  useLayoutEffect(() => {
    const element = oldPane.current;
    if (!element) return;
    const context = document.createElement("canvas").getContext("2d");
    if (!context) throw new Error("Source text measurement is unavailable.");
    let fontIdentity = "";
    let measured: Measurements | null = null;
    function measureWidths(force = false) {
      const style = getComputedStyle(element!);
      const sourceFont = `${style.fontWeight || "400"} ${style.fontSize || "11px"} ${style.fontFamily || "monospace"}`;
      const noteFont = `${style.fontWeight || "400"} ${style.getPropertyValue("--diff-note-font-size").trim() || "10px"} ${style.fontFamily || "monospace"}`;
      const nextFont = `${sourceFont}:${noteFont}`;
      const available = { old: Math.max(1, element!.clientWidth - 76),
        new: Math.max(1, (newPane.current?.clientWidth ?? element!.clientWidth) - 76) };
      const fontChanged = force || fontIdentity !== nextFont;
      if (!fontChanged && measured?.available.old === available.old && measured.available.new === available.new) return;
      if (fontChanged || !measured) {
        fontIdentity = nextFont;
        const cache = new Map<string, number>();
        const widths = new Map<string, Widths>();
        const maximum = { old: 0, new: 0 };
        function textWidth(text: string, font: string) {
          const key = `${font}:${text}`;
          const known = cache.get(key);
          if (known !== undefined) return known;
          context!.font = font;
          const width = sourceTextWidth(context!, text);
          cache.set(key, width);
          return width;
        }
        // Include offscreen rows so source width does not collapse while scrolling or switching views.
        for (const row of base.rows) {
          const width = { old: 0, new: 0 };
          let padding = 76;
          if (row.kind === "hunk") {
            width.old = width.new = textWidth(row.heading, sourceFont);
            padding = 16;
          } else if (row.kind === "newline") {
            if (row.old) width.old = textWidth("No final newline (old side)", noteFont);
            if (row.new) width.new = textWidth("No final newline (new side)", noteFont);
            padding = 64;
          } else {
            if (row.old) width.old = textWidth(row.old.text, sourceFont);
            if (row.new) width.new = textWidth(row.new.text, sourceFont);
          }
          widths.set(row.key, width);
          maximum.old = Math.max(maximum.old, width.old + padding);
          maximum.new = Math.max(maximum.new, width.new + padding);
        }
        measured = { scope: Symbol("source-geometry"), available, maximum, rows: widths };
      } else measured = { ...measured, scope: Symbol("source-geometry"), available };
      setMeasurements(measured);
    }
    measureWidths();
    const fonts = document.fonts;
    const fontsChanged = () => measureWidths(true);
    fonts?.addEventListener("loadingdone", fontsChanged);
    fonts?.addEventListener("loadingerror", fontsChanged);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => measureWidths());
    observer?.observe(element);
    if (newPane.current) observer?.observe(newPane.current);
    return () => {
      observer?.disconnect();
      fonts?.removeEventListener("loadingdone", fontsChanged);
      fonts?.removeEventListener("loadingerror", fontsChanged);
    };
  }, [base.rows, oldPane, newPane]);

  const layout = useMemo(() => {
    if (lineMode === "scroll" || !measurements) return base;
    const known = heights.scope === measurements.scope ? heights.rows : null;
    let height = 0;
    const rows = base.rows.map((row) => {
      const width = measurements.rows.get(row.key);
      const estimate = row.kind === "hunk" || !width ? row.height : Math.max(1,
        Math.ceil(width.old / measurements.available.old), Math.ceil(width.new / measurements.available.new)) * lineHeight;
      const next = { ...row, top: height, height: known?.get(row.key) ?? estimate };
      height += next.height;
      return next;
    });
    return { rows, height };
  }, [base, lineMode, measurements, heights]);

  useLayoutEffect(() => {
    if (lineMode !== "wrap" || !measurements) return;
    const scope = measurements.scope;
    const elements = [oldPane.current, newPane.current].flatMap((pane) =>
      pane ? Array.from(pane.querySelectorAll<HTMLElement>("[data-source-measure]")) : []);
    let active = true;
    function measureVisible() {
      const actual = new Map<string, { height: number; estimate: number }>();
      for (const element of elements) {
        const row = layout.rows[Number(element.dataset.sourceMeasure)];
        if (!row) continue;
        const height = Math.max(lineHeight, Math.ceil(element.getBoundingClientRect().height));
        actual.set(row.key, { height: Math.max(actual.get(row.key)?.height ?? 0, height), estimate: row.height });
      }
      if (!active) return;
      setHeights((previous) => {
        const known = previous.scope === scope ? previous.rows : new Map<string, number>();
        let changed = false;
        for (const [key, { height, estimate }] of actual) {
          if (height !== (known.get(key) ?? estimate)) { changed = true; break; }
        }
        if (!changed) return previous;
        const rows = new Map(known);
        for (const [key, { height }] of actual) rows.set(key, height);
        return { scope, rows };
      });
    }
    // Canvas estimates cover offscreen rows; only visible native CSS wraps require DOM measurement.
    measureVisible();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measureVisible);
    for (const element of elements) observer?.observe(element);
    return () => { active = false; observer?.disconnect(); };
  }, [layout.rows, lineMode, measurements, oldPane, newPane, visibleRange.start, visibleRange.end]);

  return { layout, sourceWidths: measurements?.maximum ?? { old: 0, new: 0 } };
}
