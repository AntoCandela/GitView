/** Exercises bounded pointer and keyboard resizing, including the shared two-axis junction. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ResizableWorkbench } from "../../src/app/workbench/ResizableWorkbench";
import { resetPanelLayout } from "../../src/ui/resize/panelLayout";

const pointerEvent = window.PointerEvent;
const resizeObserver = window.ResizeObserver;
// Restore shared jsdom shims after each scenario's global-stub cleanup.
beforeEach(() => {
  vi.stubGlobal("PointerEvent", pointerEvent);
  vi.stubGlobal("ResizeObserver", resizeObserver);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

test("a widened file pane is constrained when its available viewport shrinks", async () => {
  let viewport = 1000;
  let resize: () => void = () => {};
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({
    width: viewport, height: 600, x: 0, y: 0, top: 0, left: 0, right: viewport, bottom: 600, toJSON: () => ({}),
  }));
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: () => void) {}
    observe(element: Element) { if (element.classList.contains("workbench")) resize = this.callback; }
    disconnect() {}
  });
  const user = userEvent.setup();
  render(<ResizableWorkbench files={<span>Working files</span>} history={<span>Commit graph</span>} comparison={<span>File comparison</span>} />);
  const divider = screen.getByRole("separator", { name: "Resize file list" });
  divider.focus();
  await user.keyboard("{End}");
  expect(Number(divider.getAttribute("aria-valuenow"))).toBeGreaterThan(500);
  act(() => { viewport = 500; resize(); });
  const constrained = Number(divider.getAttribute("aria-valuenow"));
  expect(constrained).toBeGreaterThan(0);
  expect(constrained).toBeLessThan(500);
  await user.keyboard("{Home}");
  expect(Number(divider.getAttribute("aria-valuenow"))).toBeLessThan(constrained);
  expect(screen.getByText("Working files")).toBeVisible();
  expect(screen.getByText("Commit graph")).toBeVisible();
});

test("row resizing preserves both rows when the window height shrinks", async () => {
  let height = 700;
  let resize: () => void = () => {};
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({
    width: 800, height, x: 0, y: 0, top: 0, left: 0, right: 800, bottom: height, toJSON: () => ({}),
  }));
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: () => void) {}
    observe(element: Element) { if (element.classList.contains("workbench")) resize = this.callback; }
    disconnect() {}
  });
  const user = userEvent.setup();
  render(<ResizableWorkbench files={<span>Working files</span>} history={<span>Commit graph</span>} comparison={<span>File comparison</span>} />);
  const divider = screen.getByRole("separator", { name: "Resize panel rows" });
  divider.focus();
  await user.keyboard("{End}");
  const expanded = Number(divider.getAttribute("aria-valuenow"));
  act(() => { height = 240; resize(); });
  const constrained = Number(divider.getAttribute("aria-valuenow"));
  expect(constrained).toBeLessThan(expanded);
  expect(constrained).toBeLessThan(240);
  expect(constrained).toBeGreaterThan(0);
  await user.keyboard("{Home}");
  expect(Number(divider.getAttribute("aria-valuenow"))).toBeLessThan(constrained);
  expect(screen.getByText("Working files")).toBeVisible();
  expect(screen.getByText("Commit graph")).toBeVisible();
  expect(screen.getByText("File comparison")).toBeVisible();
});

function renderPointerWorkbench() {
  const rectangle = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    width: 1000, height: 600, x: 0, y: 0, top: 0, left: 0, right: 1000, bottom: 600, toJSON: () => ({}),
  });
  render(<ResizableWorkbench files={<span>Working files</span>} history={<span>Commit graph</span>} comparison={<span>File comparison</span>} />);
  return {
    rectangle,
    junction: screen.getByRole("button", { name: "Resize all panels" }),
    column: screen.getByRole("separator", { name: "Resize file list" }),
    row: screen.getByRole("separator", { name: "Resize panel rows" }),
  };
}

// Pointer capture is absent in jsdom; actual browser dragging verifies capture outside the handle.
function installPointerCapture(element: HTMLElement) {
  let captured: number | null = null;
  Object.assign(element, {
    setPointerCapture: (id: number) => { captured = id; },
    hasPointerCapture: (id: number) => captured === id,
    releasePointerCapture: () => { captured = null; },
  });
}

test("dragging the junction changes both splitters and stops when the pointer is released", () => {
  const { junction, column, row } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 342, clientY: 332 });
  expect(column).toHaveAttribute("aria-valuenow", "340");
  expect(row).toHaveAttribute("aria-valuenow", "330");
  fireEvent.pointerUp(junction, { pointerId: 7 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 500, clientY: 500 });
  expect(column).toHaveAttribute("aria-valuenow", "340");
  expect(row).toHaveAttribute("aria-valuenow", "330");
});

test("junction dragging constrains each axis independently at both extremes", () => {
  const { junction, column, row } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 10000, clientY: 332 });
  expect(column).toHaveAttribute("aria-valuenow", "815");
  expect(row).toHaveAttribute("aria-valuenow", "330");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: -10000, clientY: 10000 });
  expect(column).toHaveAttribute("aria-valuenow", "128");
  expect(row).toHaveAttribute("aria-valuenow", "467");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 342, clientY: -10000 });
  expect(column).toHaveAttribute("aria-valuenow", "340");
  expect(row).toHaveAttribute("aria-valuenow", "128");
});

test.each(["pointerCancel", "lostPointerCapture"] as const)("%s ends junction dragging without accepting another pointer", (endEvent) => {
  const { junction, column, row } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 342, clientY: 332 });
  fireEvent[endEvent](junction, { pointerId: 8 });
  fireEvent.pointerMove(junction, { pointerId: 8, clientX: 1000, clientY: 1000 });
  expect(column).toHaveAttribute("aria-valuenow", "340");
  expect(row).toHaveAttribute("aria-valuenow", "330");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 362, clientY: 352 });
  expect(column).toHaveAttribute("aria-valuenow", "360");
  expect(row).toHaveAttribute("aria-valuenow", "350");
  fireEvent[endEvent](junction, { pointerId: 7 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 500, clientY: 500 });
  expect(column).toHaveAttribute("aria-valuenow", "360");
  expect(row).toHaveAttribute("aria-valuenow", "350");
});

test.each([
  { axis: "column", expectedWidth: "340", expectedHeight: "270" },
  { axis: "row", expectedWidth: "240", expectedHeight: "330" },
] as const)("$axis dragging still changes only its own axis", ({ axis, expectedWidth, expectedHeight }) => {
  const { column, row } = renderPointerWorkbench();
  const target = axis === "column" ? column : row;
  installPointerCapture(target);
  fireEvent.pointerDown(target, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(target, { pointerId: 7, clientX: 342, clientY: 332 });
  expect(column).toHaveAttribute("aria-valuenow", expectedWidth);
  expect(row).toHaveAttribute("aria-valuenow", expectedHeight);
});

test("the junction supports independent keyboard axes and bounded two-axis extremes", async () => {
  const { junction, column, row } = renderPointerWorkbench();
  junction.focus();
  const user = userEvent.setup();
  await user.keyboard("{ArrowRight}{Shift>}{ArrowDown}{/Shift}");
  expect(column).toHaveAttribute("aria-valuenow", "256");
  expect(row).toHaveAttribute("aria-valuenow", "302");
  await user.keyboard("{Home}");
  expect(column).toHaveAttribute("aria-valuenow", "128");
  expect(row).toHaveAttribute("aria-valuenow", "128");
  await user.keyboard("{End}");
  expect(column).toHaveAttribute("aria-valuenow", "815");
  expect(row).toHaveAttribute("aria-valuenow", "467");
});

test.each([120, 800])("the common row divider snaps above either lower panel at x=%i and releases outside six pixels", (clientX) => {
  const { row, column } = renderPointerWorkbench();
  installPointerCapture(row);
  fireEvent.pointerDown(row, { pointerId: 7, button: 0, isPrimary: true, clientX, clientY: 272 });
  fireEvent.pointerMove(row, { pointerId: 7, clientX, clientY: 305.5 });
  const workbench = screen.getByRole("region", { name: "Repository workbench" });
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("297.5px");
  expect(column).toHaveAttribute("aria-valuenow", "240");
  expect(row).toHaveClass("is-snapped");
  expect(column).not.toHaveClass("is-snapped");
  fireEvent.pointerMove(row, { pointerId: 7, clientX, clientY: 306.5 });
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("304.5px");
  expect(row).not.toHaveClass("is-snapped");
});

test("the junction snaps both axes to equal usable halves and releases each outside the snap zone", () => {
  const { junction, column, row } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 505.5, clientY: 293.5 });
  const workbench = screen.getByRole("region", { name: "Repository workbench" });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("497.5px");
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("297.5px");
  expect(column).toHaveClass("is-snapped");
  expect(row).toHaveClass("is-snapped");
  expect(junction).toHaveClass("is-snapped");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 506.5, clientY: 292.5 });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("504.5px");
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("290.5px");
  expect(column).not.toHaveClass("is-snapped");
  expect(row).not.toHaveClass("is-snapped");
  expect(junction).not.toHaveClass("is-snapped");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 499.5, clientY: 299.5 });
  expect(junction).toHaveClass("is-snapped");
  fireEvent.pointerUp(junction, { pointerId: 7 });
  expect(column).not.toHaveClass("is-snapped");
  expect(row).not.toHaveClass("is-snapped");
  expect(junction).not.toHaveClass("is-snapped");
});

test("column pointer resizing uses the same midpoint snap without changing keyboard increments", () => {
  const { column } = renderPointerWorkbench();
  installPointerCapture(column);
  fireEvent.pointerDown(column, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 400 });
  fireEvent.pointerMove(column, { pointerId: 7, clientX: 495.5, clientY: 400 });
  expect(column).toHaveClass("is-snapped");
  fireEvent.pointerUp(column, { pointerId: 7 });
  expect(column).not.toHaveClass("is-snapped");
  const workbench = screen.getByRole("region", { name: "Repository workbench" });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("497.5px");
  fireEvent.keyDown(column, { key: "ArrowLeft" });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("481.5px");
  fireEvent.keyDown(column, { key: "ArrowRight", shiftKey: true });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("513.5px");
});

test("a drag follows the live workbench origin and dimensions while retaining its original grab offset", () => {
  const { junction, rectangle } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 244, clientY: 271 });
  rectangle.mockReturnValue({
    width: 780, height: 520, x: 220, y: 80, top: 80, left: 220, right: 1000, bottom: 600, toJSON: () => ({}),
  });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 611.5, clientY: 338.5 });
  const workbench = screen.getByRole("region", { name: "Repository workbench" });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("387.5px");
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("257.5px");
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 628, clientY: 381 });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("404px");
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("300px");
});

test.each([
  { availableWidth: 400, expectedWidth: "197.5px" },
  { availableWidth: 600, expectedWidth: "297.5px" },
  { availableWidth: 1000, expectedWidth: "497.5px" },
])("reset evenly splits the final $availableWidth-pixel workspace after opening the sidebar without remounting content", ({ availableWidth, expectedWidth }) => {
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => {
    const width = document.querySelector("[data-sidebar-open=true]") ? availableWidth : availableWidth + 280;
    return { width, height: 600, x: 0, y: 0, top: 0, left: 0, right: width, bottom: 600, toJSON: () => ({}) };
  });
  function SelectedFile() {
    const [selected, setSelected] = useState(false);
    return <button onClick={() => setSelected(true)}>{selected ? "Selected file" : "Select file"}</button>;
  }
  function Workspace() {
    const [sidebarOpen, setSidebarOpen] = useState(false);
    return <div data-sidebar-open={sidebarOpen}>
      <button onClick={() => { resetPanelLayout(); setSidebarOpen(true); }}>Reset panels</button>
      <ResizableWorkbench files={<SelectedFile />} history={<span>Commit graph</span>} comparison={<span>File comparison</span>} />
    </div>;
  }
  render(<Workspace />);
  const column = screen.getByRole("separator", { name: "Resize file list" });
  const row = screen.getByRole("separator", { name: "Resize panel rows" });
  fireEvent.click(screen.getByRole("button", { name: "Select file" }));
  const selected = screen.getByRole("button", { name: "Selected file" });
  fireEvent.keyDown(column, { key: "End" });
  fireEvent.keyDown(row, { key: "End" });
  expect(row).toHaveAttribute("aria-valuenow", "467");
  fireEvent.click(screen.getByRole("button", { name: "Reset panels" }));
  expect(screen.getByRole("region", { name: "Repository workbench" }).style.getPropertyValue("--workbench-left-width")).toBe(expectedWidth);
  expect(row).toHaveAttribute("aria-valuenow", "270");
  expect(screen.getByRole("button", { name: "Selected file" })).toBe(selected);
});

test.each(["pointerCancel", "lostPointerCapture"] as const)("%s clears the active midpoint cue without changing the snapped position", (endEvent) => {
  const { junction, column, row } = renderPointerWorkbench();
  installPointerCapture(junction);
  fireEvent.pointerDown(junction, { pointerId: 7, button: 0, isPrimary: true, clientX: 242, clientY: 272 });
  fireEvent.pointerMove(junction, { pointerId: 7, clientX: 499.5, clientY: 299.5 });
  expect(junction).toHaveClass("is-snapped");
  fireEvent[endEvent](junction, { pointerId: 7 });
  expect(junction).not.toHaveClass("is-snapped");
  expect(column).not.toHaveClass("is-snapped");
  expect(row).not.toHaveClass("is-snapped");
  const workbench = screen.getByRole("region", { name: "Repository workbench" });
  expect(workbench.style.getPropertyValue("--workbench-left-width")).toBe("497.5px");
  expect(workbench.style.getPropertyValue("--workbench-top-height")).toBe("297.5px");
});
