/** Supplies deterministic jsdom geometry while exercising the real tree/history virtualizers. */
import { vi } from "vitest";

export function installVirtualLayout({ viewportHeight = 360, viewportWidth = 480, clientHeight = viewportHeight }: {
  viewportHeight?: number;
  viewportWidth?: number;
  clientHeight?: number;
} = {}): () => void {
  const scrollTo = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollTo");
  const boundingRect = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
    if (this.matches(".history-scroll, .changes-tree")) return viewportHeight;
    if (this.classList.contains("history-row")) {
      return Number.parseFloat(this.querySelector<HTMLElement>(".history-header")?.style.getPropertyValue("--history-header-height") ?? "") || 28;
    }
    return 28;
  });
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(viewportWidth);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.matches(".history-scroll, .changes-tree") ? viewportHeight : clientHeight;
  });
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) {
    const content = this.querySelector<HTMLElement>(".history-rows, .changes-tree-content");
    return Number.parseFloat(content?.style.height ?? "0") || this.clientHeight;
  });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    if (this.classList.contains("changes-tree") || this.classList.contains("changes-tree-content")) {
      const row = this.closest<HTMLElement>(".history-row");
      const rowTop = Number(row?.style.transform.match(/translateY\(([-.\d]+)px\)/)?.[1] ?? 0);
      const headerHeight = Number.parseFloat(row?.querySelector<HTMLElement>(".history-header")?.style.getPropertyValue("--history-header-height") ?? "") || 0;
      const parentHeight = row?.querySelector(".history-parent-choice") ? 28 : 0;
      const scrollTop = this.closest(".history-scroll")?.scrollTop ?? 0;
      return new DOMRect(0, rowTop + (row ? headerHeight + 4 + parentHeight : 0) - scrollTop, viewportWidth, this.offsetHeight);
    }
    if (this.matches(".history-scroll, .history-row, .history-lanes")) return new DOMRect(0, 0, viewportWidth, this.offsetHeight);
    return boundingRect.call(this);
  });
  Object.defineProperty(HTMLElement.prototype, "scrollTo", {
    configurable: true,
    value: function (this: HTMLElement, options: ScrollToOptions) {
      this.scrollTop = options.top ?? this.scrollTop;
      queueMicrotask(() => this.dispatchEvent(new Event("scroll")));
    },
  });
  return () => {
    if (scrollTo) Object.defineProperty(HTMLElement.prototype, "scrollTo", scrollTo);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  };
}
