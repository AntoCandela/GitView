/** Exercises sidebar bounds and non-destructive sizing resets independently of repository loading. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { WorkspaceSidebar } from "../../src/app/WorkspaceSidebar";
import { resetPanelLayout } from "../../src/ui/resize/panelLayout";

let bodyWidth = 1000;
let notifyResize: () => void;
const pointerEvent = window.PointerEvent;

beforeEach(() => {
  bodyWidth = 1000;
  vi.stubGlobal("innerWidth", 1000);
  vi.stubGlobal("PointerEvent", pointerEvent);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(() => ({
    width: bodyWidth, height: 600, x: 24, y: 64, top: 64, left: 24, right: 24 + bodyWidth, bottom: 664, toJSON: () => ({}),
  }));
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { notifyResize = callback; }
    observe() {}
    disconnect() {}
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function sidebar(open = true) {
  return <div className="workspace-body"><WorkspaceSidebar open={open}><input aria-label="Retained file selection" defaultValue="selected.ts" /></WorkspaceSidebar></div>;
}

function divider() {
  return screen.getByRole("separator", { name: "Resize repository files sidebar" });
}

test("desktop resizing preserves 320 pixels for the main region using observed body bounds", () => {
  render(sidebar());
  expect(divider()).toHaveAttribute("aria-valuenow", "280");
  fireEvent.keyDown(divider(), { key: "End" });
  expect(divider()).toHaveAttribute("aria-valuenow", "480");
  act(() => { bodyWidth = 740; notifyResize(); });
  expect(divider()).toHaveAttribute("aria-valuemax", "420");
  expect(divider()).toHaveAttribute("aria-valuenow", "420");
  fireEvent.keyDown(divider(), { key: "Home" });
  expect(divider()).toHaveAttribute("aria-valuenow", "180");
});

test("narrow overlays leave 40 pixels outside and safely lower the minimum", () => {
  vi.stubGlobal("innerWidth", 210);
  bodyWidth = 200;
  render(sidebar());
  expect(divider()).toHaveAttribute("aria-valuemin", "160");
  expect(divider()).toHaveAttribute("aria-valuemax", "160");
  expect(divider()).toHaveAttribute("aria-valuenow", "160");
  act(() => { bodyWidth = 600; notifyResize(); });
  expect(divider()).toHaveAttribute("aria-valuemax", "170");
  expect(divider()).toHaveAttribute("aria-valuenow", "170");
});

test("collapse retains width and mounted content while a hidden reset restores the default width", () => {
  const view = render(sidebar());
  const selection = screen.getByRole("textbox");
  fireEvent.keyDown(divider(), { key: "ArrowRight" });
  expect(divider()).toHaveAttribute("aria-valuenow", "296");
  view.rerender(sidebar(false));
  view.rerender(sidebar());
  expect(divider()).toHaveAttribute("aria-valuenow", "296");
  expect(screen.getByRole("textbox")).toBe(selection);
  view.rerender(sidebar(false));
  act(() => resetPanelLayout());
  view.rerender(sidebar());
  expect(divider()).toHaveAttribute("aria-valuenow", "280");
  expect(screen.getByRole("textbox")).toBe(selection);
  expect(selection).toHaveValue("selected.ts");
});

test("an empty sidebar drags relative to the actual workspace origin", () => {
  render(<div className="workspace-body"><WorkspaceSidebar open><p>Choose a repository.</p></WorkspaceSidebar></div>);
  const edge = divider();
  let captured: number | null = null;
  Object.assign(edge, {
    setPointerCapture: (id: number) => { captured = id; },
    hasPointerCapture: (id: number) => captured === id,
    releasePointerCapture: () => { captured = null; },
  });
  fireEvent.pointerDown(edge, { pointerId: 7, button: 0, isPrimary: true, clientX: 302, clientY: 100 });
  fireEvent.pointerMove(edge, { pointerId: 7, clientX: 402, clientY: 100 });
  expect(edge).toHaveAttribute("aria-valuenow", "380");
  fireEvent.pointerUp(edge, { pointerId: 7 });
  fireEvent.pointerMove(edge, { pointerId: 7, clientX: 452, clientY: 100 });
  expect(edge).toHaveAttribute("aria-valuenow", "380");
});
