/** Supplies PointerEvent semantics missing from jsdom so headless UI keyboard clicks use real handlers. */

import { vi } from "vitest";

if (!window.PointerEvent) {
  class TestPointerEvent extends MouseEvent {
    readonly pointerId: number;
    readonly pointerType: string;
    readonly isPrimary: boolean;

    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 0;
      this.pointerType = init.pointerType ?? "mouse";
      this.isPrimary = init.isPrimary ?? true;
    }
  }
  vi.stubGlobal("PointerEvent", TestPointerEvent);
}

// jsdom has no layout engine; resize scenarios install their own observable dimensions.
if (!window.ResizeObserver) {
  vi.stubGlobal("ResizeObserver", class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
}

// jsdom has no font rasterizer; browser smoke checks use the real canvas and loaded fonts.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: (type: string) => type === "2d" ? {
    font: "",
    measureText: (text: string) => ({ width: text.length * 7 }),
  } : null,
});
