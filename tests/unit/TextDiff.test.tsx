/** Exercises paired hunk presentation and reading continuity, including duplicate and virtualized source rows. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { TextHunk } from "../../src/contracts/diff";
import { TextDiff } from "../../src/features/diff/text/TextDiff";
import { textReview } from "../support/review";
import { resetPanelLayout } from "../../src/ui/resize/panelLayout";

let glyphScale = 1;

beforeEach(() => {
  resetPanelLayout();
  // jsdom has no layout; the source viewport contains ten of the component's fixed-height rows.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(220);
  glyphScale = 1;
  // Canvas is also absent in jsdom. Model fallback wide glyphs separately from ordinary mono advances.
  const context = {
    font: "",
    measureText(text: string) {
      let width = 0;
      for (const character of text) width += (character === "界" ? 18 : 6) * glyphScale;
      return { width, actualBoundingBoxRight: width } as TextMetrics;
    },
  } as CanvasRenderingContext2D;
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(context);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("the snapped diff proportion survives file remounts and resets without replacing source panes", () => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 1005, height: 600, x: 80, y: 40, top: 40, left: 80, right: 1085, bottom: 640, toJSON: () => ({}),
  });
  const first = render(<TextDiff review={textReview("file A")} />);
  const divider = screen.getByRole("separator", { name: "Resize old and new versions" });
  let captured: number | null = null;
  Object.assign(divider, {
    setPointerCapture: (id: number) => { captured = id; },
    hasPointerCapture: (id: number) => captured === id,
    releasePointerCapture: () => { captured = null; },
  });
  fireEvent.pointerDown(divider, { pointerId: 1, button: 0, isPrimary: true, clientX: 582 });
  fireEvent.pointerMove(divider, { pointerId: 1, clientX: 587 });
  expect(divider).toHaveAttribute("aria-valuetext", "50% old, 50% new");
  fireEvent.pointerMove(divider, { pointerId: 1, clientX: 382 });
  expect(divider).toHaveAttribute("aria-valuetext", "30% old, 70% new");
  fireEvent.pointerCancel(divider, { pointerId: 1 });
  fireEvent.pointerMove(divider, { pointerId: 1, clientX: 782 });
  expect(divider).toHaveAttribute("aria-valuetext", "30% old, 70% new");
  first.unmount();
  render(<TextDiff review={textReview("file B")} />);
  const nextDivider = screen.getByRole("separator", { name: "Resize old and new versions" });
  expect(nextDivider).toHaveAttribute("aria-valuetext", "30% old, 70% new");
  const source = screen.getByRole("region", { name: "New source hunks" });
  act(() => resetPanelLayout());
  expect(nextDivider).toHaveAttribute("aria-valuetext", "50% old, 50% new");
  expect(screen.getByRole("region", { name: "New source hunks" })).toBe(source);
  expect(within(source).getByText("file B")).toBeVisible();
});

function comparison(lines: string[]) {
  const result = textReview("");
  result.hunks = [{ oldStart: 1, oldCount: lines.length, newStart: 1, newCount: lines.length, lines: lines.map((text) => ({ kind: "context", text })) }];
  return result;
}

function panes() {
  return { old: screen.getByRole("region", { name: "Old source hunks" }), new: screen.getByRole("region", { name: "New source hunks" }) };
}

function scrollTo(pane: HTMLElement, top: number) {
  pane.scrollTop = top;
  fireEvent.scroll(pane);
}

function sourceRow(pane: HTMLElement, text: string) {
  return within(pane).getByText(text).closest<HTMLElement>(".diff-line")!;
}

test("context and replacements align with the correct side numbers, leaving unmatched removals empty on the new side", () => {
  const review = textReview("");
  const lines: TextHunk["lines"] = [
    { kind: "context", text: "before" },
    { kind: "removal", text: "old first" },
    { kind: "removal", text: "old second" },
    { kind: "removal", text: "old third" },
    { kind: "addition", text: "new first" },
    { kind: "addition", text: "new second" },
    { kind: "context", text: "after" },
  ];
  review.hunks = [{ oldStart: 10, oldCount: 5, newStart: 20, newCount: 4, lines }];
  render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  for (const [oldText, newText] of [["before", "before"], ["old first", "new first"], ["old second", "new second"], ["after", "after"]]) {
    expect(sourceRow(old, oldText).style.top).toBe(sourceRow(next, newText).style.top);
  }
  expect(sourceRow(old, "old first")).toHaveAccessibleName("Old removal line 11");
  expect(sourceRow(next, "new first")).toHaveAccessibleName("New addition line 21");
  expect(sourceRow(old, "after")).toHaveAccessibleName("Old context line 14");
  expect(sourceRow(next, "after")).toHaveAccessibleName("New context line 23");
  expect(within(next).getByLabelText("No new line").style.top).toBe(sourceRow(old, "old third").style.top);
  expect(within(old).queryByText("new first")).not.toBeInTheDocument();
  expect(within(next).queryByText("old first")).not.toBeInTheDocument();
});

test("one-sided additions expose an absent old file without inventing old content, and escape source text", () => {
  const review = textReview("");
  review.from = "absent";
  review.fromAbsent = true;
  review.hunks = [{ oldStart: 0, oldCount: 0, newStart: 1, newCount: 1,
    lines: [{ kind: "addition", text: "<img src=x onerror=alert(1)>", noFinalNewline: true }] }];
  const { container } = render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  expect(screen.getByText("Old · Absent")).toBeVisible();
  expect(within(old).getByLabelText("No old line")).toBeEmptyDOMElement();
  expect(within(next).getByText("<img src=x onerror=alert(1)>")).toBeVisible();
  expect(container.querySelector("img")).toBeNull();
  expect(within(next).getByText("No final newline (new side)")).toBeVisible();
  expect(within(old).queryByText(/No final newline/)).not.toBeInTheDocument();
});

test("a deletion exposes an absent new file and retains its old-side newline notice", () => {
  const review = textReview("");
  review.toAbsent = true;
  review.hunks = [{ oldStart: 1, oldCount: 1, newStart: 0, newCount: 0,
    lines: [{ kind: "removal", text: "removed", noFinalNewline: true }] }];
  render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  expect(screen.getByText("New · Absent")).toBeVisible();
  expect(within(next).getByLabelText("No new line")).toBeEmptyDOMElement();
  expect(within(old).getByText("No final newline (old side)")).toBeVisible();
  expect(within(next).queryByText("removed")).not.toBeInTheDocument();
});

test("hunk gaps stay explicit and never invent intermediate source lines", () => {
  const review = comparison(["first snippet"]);
  review.hunks.push({ oldStart: 50, oldCount: 1, newStart: 80, newCount: 1, lines: [{ kind: "context", text: "later snippet" }] });
  render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  expect(within(old).getByText("@@ -50,1 +80,1 @@")).toBeVisible();
  expect(sourceRow(old, "later snippet")).toHaveAccessibleName("Old context line 50");
  expect(sourceRow(next, "later snippet")).toHaveAccessibleName("New context line 80");
  expect(old.querySelectorAll(".diff-line")).toHaveLength(2);
  expect(screen.getByRole("region", { name: "Read-only file comparison" })).toHaveAccessibleDescription(/Changed hunks only, not complete files/);
});

test("a surviving line retains its reading offset without blanking the updating comparison or coupling horizontal reading", () => {
  const verified = comparison(["first", "anchor", "last"]);
  const { rerender } = render(<TextDiff review={verified} />);
  const { old, new: next } = panes();
  const comparisonRegion = screen.getByRole("region", { name: "Read-only file comparison" });
  old.scrollLeft = 120;
  next.scrollLeft = 260;
  scrollTo(old, 52);
  expect(next.scrollTop).toBe(52);
  rerender(<TextDiff review={verified} updating />);
  expect(screen.getByRole("region", { name: "Last verified file comparison" })).toBe(comparisonRegion);
  expect(within(old).getByText("anchor")).toBeVisible();
  expect(old.scrollTop).toBe(52);
  rerender(<TextDiff review={comparison(["inserted", "first", "anchor", "last"])} />);
  expect(old.scrollTop).toBe(74);
  expect(next.scrollTop).toBe(74);
  expect(old.scrollLeft).toBe(120);
  expect(next.scrollLeft).toBe(260);
  expect(screen.queryByText(/Reading position reset/)).not.toBeInTheDocument();
});

test("a removed reading anchor resets both source panes on both axes and announces why", () => {
  const { rerender } = render(<TextDiff review={comparison(["first", "anchor", "last"])} />);
  const { old, new: next } = panes();
  old.scrollLeft = 120;
  next.scrollLeft = 260;
  scrollTo(next, 52);
  rerender(<TextDiff review={comparison(["replacement"])} />);
  for (const pane of [old, next]) {
    expect(pane.scrollTop).toBe(0);
    expect(pane.scrollLeft).toBe(0);
  }
  expect(screen.getByRole("status")).toHaveTextContent("Reading position reset");
});

test("duplicate source lines retain the selected occurrence rather than jumping to the first copy", () => {
  const { rerender } = render(<TextDiff review={comparison(["same", "middle", "same", "last"])} />);
  const { old } = panes();
  scrollTo(old, 74);
  rerender(<TextDiff review={comparison(["inserted", "same", "middle", "same", "last"])} />);
  expect(old.scrollTop).toBe(96);
});

test("prepending an identical line preserves the original second copy through another refresh", () => {
  const { rerender } = render(<TextDiff review={comparison(["same", "middle", "same", "last"])} />);
  const { old } = panes();
  scrollTo(old, 74);
  rerender(<TextDiff review={comparison(["same", "same", "middle", "same", "last"])} />);
  expect(old.scrollTop).toBe(96);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  rerender(<TextDiff review={comparison(["same", "middle", "same", "last"])} />);
  expect(old.scrollTop).toBe(74);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("deleting the first identical line preserves the surviving second copy", () => {
  const { rerender } = render(<TextDiff review={comparison(["same", "middle", "same", "last"])} />);
  const { old } = panes();
  scrollTo(old, 74);
  rerender(<TextDiff review={comparison(["middle", "same", "last"])} />);
  expect(old.scrollTop).toBe(52);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("removing the selected duplicate resets instead of borrowing the earlier identical line", () => {
  const { rerender } = render(<TextDiff review={comparison(["same", "middle", "same", "last"])} />);
  const { old } = panes();
  scrollTo(old, 74);
  rerender(<TextDiff review={comparison(["same", "middle", "last"])} />);
  expect(old.scrollTop).toBe(0);
  expect(screen.getByRole("status")).toHaveTextContent("Reading position reset");
});

test.each([
  { lines: ["changed header", "same", "same", "middle", "same", "last", "changed footer"], scrollTop: 118 },
  { lines: ["changed header", "middle", "same", "last", "changed footer"], scrollTop: 74 },
])("duplicate context identifies the surviving copy when both diff ends change: $scrollTop", ({ lines, scrollTop }) => {
  const { rerender } = render(<TextDiff review={comparison(["header", "same", "middle", "same", "last", "footer"])} />);
  const { old } = panes();
  scrollTo(old, 96);
  rerender(<TextDiff review={comparison(lines)} />);
  expect(old.scrollTop).toBe(scrollTop);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("ambiguous repeated contexts reset instead of choosing an arbitrary surviving copy", () => {
  const { rerender } = render(<TextDiff review={comparison(["old header", "before", "same", "after", "old footer"])} />);
  const { old } = panes();
  scrollTo(old, 74);
  rerender(<TextDiff review={comparison(["new header", "before", "same", "after", "separator", "before", "same", "after", "new footer"])} />);
  expect(old.scrollTop).toBe(0);
  expect(screen.getByRole("status")).toHaveTextContent("Reading position reset");
});

test("large previews keep bounded DOM while either pane can navigate to late source rows and retain an offscreen anchor on refresh", () => {
  const lines = Array.from({ length: 10_000 }, (_, index) => `source ${index}`);
  const { rerender } = render(<TextDiff review={comparison(lines)} />);
  const { old, new: next } = panes();
  expect(within(old).getByText("source 0")).toBeVisible();
  expect(within(old).queryByText("source 9000")).not.toBeInTheDocument();
  scrollTo(next, 26 + 9000 * 22 + 4);
  expect(old.scrollTop).toBe(next.scrollTop);
  expect(within(old).getByText("source 9000")).toBeVisible();
  expect(within(next).getByText("source 9000")).toBeVisible();
  expect(within(old).queryByText("source 0")).not.toBeInTheDocument();
  expect(old.querySelectorAll(".diff-line").length).toBeLessThan(100);
  expect(next.querySelectorAll(".diff-line").length).toBeLessThan(100);
  rerender(<TextDiff review={comparison(["inserted", ...lines])} />);
  expect(old.scrollTop).toBe(26 + 9001 * 22 + 4);
  expect(next.scrollTop).toBe(old.scrollTop);
  expect(within(old).getByText("source 9000")).toBeVisible();
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
  scrollTo(old, 0);
  expect(within(next).getByText("inserted")).toBeVisible();
  expect(within(next).queryByText("source 9000")).not.toBeInTheDocument();
});

test("a queued horizontal scroll does not undo a vertical move in the other source pane", () => {
  render(<TextDiff review={comparison(Array.from({ length: 200 }, (_, index) => `line ${index}`))} />);
  const { old, new: next } = panes();
  scrollTo(old, 1200);
  next.scrollLeft = 90;
  old.scrollTop = 1600;
  fireEvent.scroll(next);
  fireEvent.scroll(old);
  expect(old.scrollTop).toBe(1600);
  expect(next.scrollTop).toBe(1600);
  expect(next.scrollLeft).toBe(90);
  expect(within(old).getByText("line 71")).toBeVisible();
});

test("virtualizing away a hunk header or newline notice keeps sufficient horizontal width and the reading offset", () => {
  const review = comparison(Array.from({ length: 100 }, () => "x"));
  review.hunks[0].lines[0].noFinalNewline = true;
  render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  const oldContent = old.querySelector<HTMLElement>(".diff-source-content")!;
  const newContent = next.querySelector<HTMLElement>(".diff-source-content")!;
  const initialWidth = oldContent.style.width;
  // The notice's text plus its source gutter is wider than either the hunk label or the one-character source.
  expect(Number(initialWidth.match(/,\s*(\d+)px/)?.[1])).toBeGreaterThanOrEqual(220);
  old.scrollLeft = 170;
  next.scrollLeft = 150;
  scrollTo(old, 26 + 81 * 22);
  expect(within(old).queryByText(/No final newline/)).not.toBeInTheDocument();
  expect(within(old).queryByText(/@@/)).not.toBeInTheDocument();
  expect(oldContent.style.width).toBe(initialWidth);
  expect(newContent.style.width).toBe(initialWidth);
  expect(old.scrollLeft).toBe(170);
  expect(next.scrollLeft).toBe(150);
});

test("offscreen wide Unicode source and tab stops determine a stable readable width", () => {
  const lines = Array.from({ length: 100 }, (_, index) => `line ${index}`);
  lines[90] = `${"界".repeat(80)}\tx`;
  render(<TextDiff review={comparison(lines)} />);
  const { old } = panes();
  const content = old.querySelector<HTMLElement>(".diff-source-content")!;
  const initialWidth = content.style.width;
  expect(Number(initialWidth.match(/,\s*(\d+)px/)?.[1])).toBeGreaterThanOrEqual(1546);
  expect(within(old).queryByText(lines[90])).not.toBeInTheDocument();
  old.scrollLeft = 1100;
  scrollTo(old, 26 + 90 * 22);
  expect(within(old).getByText(lines[90], { exact: true, normalizer: (text) => text })).toBeVisible();
  expect(content.style.width).toBe(initialWidth);
  expect(old.scrollLeft).toBe(1100);
});

test("finishing webfont loading recomputes source widths without losing the selected vertical row", () => {
  const previousFonts = Object.getOwnPropertyDescriptor(document, "fonts");
  const fonts = new EventTarget();
  Object.defineProperty(document, "fonts", { configurable: true, value: fonts });
  try {
    render(<TextDiff review={comparison(Array.from({ length: 100 }, (_, index) => `${"wide text ".repeat(40)}${index}`))} />);
    const { old, new: next } = panes();
    scrollTo(old, 26 + 50 * 22 + 4);
    const content = old.querySelector<HTMLElement>(".diff-source-content")!;
    const initialWidth = Number(content.style.width.match(/,\s*(\d+)px/)?.[1]);
    glyphScale = 1.5;
    act(() => { fonts.dispatchEvent(new Event("loadingdone")); });
    expect(Number(content.style.width.match(/,\s*(\d+)px/)?.[1])).toBeGreaterThan(initialWidth);
    expect(old.scrollTop).toBe(26 + 50 * 22 + 4);
    expect(next.scrollTop).toBe(old.scrollTop);
  } finally {
    if (previousFonts) Object.defineProperty(document, "fonts", previousFonts);
    else Reflect.deleteProperty(document, "fonts");
  }
});

test("full snapshots fill separated hunk gaps and retain the changed reading line when toggling views", () => {
  const review = textReview("");
  const oldLines = Array.from({ length: 100 }, (_, index) => `source ${index + 1}`);
  const newLines = [...oldLines];
  newLines[9] = "changed ten";
  newLines[79] = "changed eighty";
  review.fromContent = `${oldLines.join("\n")}\n`;
  review.toContent = `${newLines.join("\n")}\n`;
  review.hunks = [10, 80].map((number) => ({
    oldStart: number, newStart: number, oldCount: 1, newCount: 1,
    lines: [{ kind: "removal" as const, text: oldLines[number - 1] },
      { kind: "addition" as const, text: newLines[number - 1] }],
  }));
  const { rerender } = render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  expect(within(old).queryByText("source 1", { exact: true })).not.toBeInTheDocument();
  scrollTo(old, 26);
  old.scrollLeft = 80;
  next.scrollLeft = 120;
  rerender(<TextDiff review={review} mode="full" />);
  expect(old).toHaveAccessibleName("Old source file");
  expect(old.scrollTop).toBe(9 * 22);
  expect(next.scrollTop).toBe(old.scrollTop);
  expect(old.scrollLeft).toBe(80);
  expect(next.scrollLeft).toBe(120);
  scrollTo(old, 50 * 22);
  expect(sourceRow(old, "source 51")).toHaveAccessibleName("Old context line 51");
  expect(sourceRow(next, "source 51")).toHaveAccessibleName("New context line 51");
  expect(within(old).queryByText("source 1", { exact: true })).not.toBeInTheDocument();
  scrollTo(next, 79 * 22);
  expect(sourceRow(old, "source 80")).toHaveAccessibleName("Old removal line 80");
  expect(sourceRow(next, "changed eighty")).toHaveAccessibleName("New addition line 80");
  rerender(<TextDiff review={review} />);
  expect(old.scrollTop).toBe(74);
  expect(sourceRow(next, "changed eighty")).toBeVisible();
});

test("full snapshots preserve zero-count insertion alignment and distinguish empty from absent endpoints", () => {
  const review = textReview("");
  review.fromContent = "first\nlast";
  review.toContent = "first\ninserted\nlast";
  review.hunks = [{ oldStart: 1, oldCount: 0, newStart: 2, newCount: 1,
    lines: [{ kind: "addition", text: "inserted" }] }];
  const { rerender } = render(<TextDiff review={review} mode="full" />);
  const old = screen.getByRole("region", { name: "Old source file" });
  const next = screen.getByRole("region", { name: "New source file" });
  expect(sourceRow(old, "last").style.top).toBe(sourceRow(next, "last").style.top);
  expect(sourceRow(old, "last")).toHaveAccessibleName("Old context line 2");
  expect(sourceRow(next, "last")).toHaveAccessibleName("New context line 3");
  expect(within(old).getByText("No final newline (old side)")).toBeVisible();
  expect(within(next).getByText("No final newline (new side)")).toBeVisible();
  rerender(<TextDiff review={{ ...review, fromContent: "", toContent: "", hunks: [] }} mode="full" />);
  expect(old.querySelector(".diff-line")).toBeNull();
  expect(next.querySelector(".diff-line")).toBeNull();
  expect(screen.getByText("Old · Index")).toBeVisible();
  rerender(<TextDiff review={{ ...review, fromContent: "", toContent: "", hunks: [], fromAbsent: true }} mode="full" />);
  expect(screen.getByText("Old · Absent")).toBeVisible();
});

test("switching from a later hunk header keeps its source and independent horizontal positions", () => {
  const review = textReview("");
  const oldLines = Array.from({ length: 100 }, (_, index) => `line ${index + 1}`);
  const newLines = [...oldLines];
  newLines[79] = "changed eighty";
  review.fromContent = `${oldLines.join("\n")}\n`;
  review.toContent = `${newLines.join("\n")}\n`;
  review.hunks = [10, 80].map((number) => ({ oldStart: number, newStart: number, oldCount: 1, newCount: 1,
    lines: [{ kind: "removal" as const, text: oldLines[number - 1] },
      { kind: "addition" as const, text: newLines[number - 1] }] }));
  const { rerender } = render(<TextDiff review={review} />);
  const { old, new: next } = panes();
  old.scrollLeft = 40;
  next.scrollLeft = 90;
  scrollTo(old, 48);
  rerender(<TextDiff review={review} mode="full" />);
  expect(old.scrollTop).toBe(79 * 22);
  expect(next.scrollTop).toBe(old.scrollTop);
  expect(old.scrollLeft).toBe(40);
  expect(next.scrollLeft).toBe(90);
  expect(sourceRow(next, "changed eighty")).toHaveAccessibleName("New addition line 80");
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("mode anchors never confuse surviving new-side numbers with earlier deleted old-side numbers", () => {
  const review = textReview("");
  const oldLines = Array.from({ length: 220 }, (_, index) => `line ${index + 1}`);
  const newLines = [...oldLines.slice(0, 99), ...oldLines.slice(199)];
  review.fromContent = `${oldLines.join("\n")}\n`;
  review.toContent = `${newLines.join("\n")}\n`;
  review.hunks = [{ oldStart: 99, oldCount: 102, newStart: 99, newCount: 2, lines: [
    { kind: "context", text: "line 99" },
    ...oldLines.slice(99, 199).map((text) => ({ kind: "removal" as const, text })),
    { kind: "context", text: "line 200" },
  ] }];
  const { rerender } = render(<TextDiff review={review} mode="full" />);
  const old = screen.getByRole("region", { name: "Old source file" });
  const next = screen.getByRole("region", { name: "New source file" });
  scrollTo(next, 199 * 22);
  rerender(<TextDiff review={review} />);
  expect(old.scrollTop).toBe(26 + 101 * 22);
  expect(next.scrollTop).toBe(old.scrollTop);
  expect(sourceRow(old, "line 200")).toHaveAccessibleName("Old context line 200");
  expect(sourceRow(next, "line 200")).toHaveAccessibleName("New context line 100");
  expect(within(old).queryByText("line 100", { exact: true })).not.toBeInTheDocument();
});

test.each(["scroll", "widen"] as const)("shrinking a wrapped row through %s keeps its source line at the leading edge", (transition) => {
  let paneWidth = 300;
  const observers = new Set<ResizeObserverCallback>();
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: ResizeObserverCallback) { observers.add(callback); }
    observe() {}
    unobserve() {}
    disconnect() { observers.delete(this.callback); }
  });
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(() => paneWidth);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const wraps = this.closest(".diff-viewport")?.classList.contains("is-wrapped");
    const height = this.matches("[data-source-measure]") && wraps
      ? Math.max(1, Math.ceil((this.textContent?.length ?? 0) * 6 / (paneWidth - 76))) * 22 : 22;
    return DOMRect.fromRect({ height });
  });
  try {
    const longLine = `long source ${"x".repeat(400)}`;
    const lines = [longLine, ...Array.from({ length: 100 }, (_, index) => `following ${index}`)];
    const review = comparison(lines);
    review.fromContent = review.toContent = `${lines.join("\n")}\n`;
    const { rerender } = render(<TextDiff review={review} lineMode="wrap" />);
    const source = panes();
    const originalRow = sourceRow(source.new, longLine);
    expect(Number.parseFloat(originalRow.style.height)).toBeGreaterThan(154);
    scrollTo(source.new, Number.parseFloat(originalRow.style.top) + 154);

    if (transition === "scroll") rerender(<TextDiff review={review} lineMode="scroll" />);
    else {
      paneWidth = 1200;
      act(() => { for (const callback of [...observers]) callback([], {} as ResizeObserver); });
    }

    for (const pane of [source.old, source.new]) {
      const row = sourceRow(pane, longLine);
      const top = Number.parseFloat(row.style.top);
      expect(pane.scrollTop).toBeGreaterThanOrEqual(top);
      expect(pane.scrollTop).toBeLessThan(top + Number.parseFloat(row.style.height));
    }
    expect(source.old.scrollTop).toBe(source.new.scrollTop);
    expect(screen.queryByText(/Reading position reset/)).not.toBeInTheDocument();
  } finally {
    vi.unstubAllGlobals();
  }
});
