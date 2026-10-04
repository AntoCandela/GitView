/** Exercises logical navigation, bounded mounting and progressive directory presentation. */

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ChangeTree, changeDirectoryId, type ChangeTreeDirectory, type ChangeTreeFile } from "../../src/ui/file-explorer/ChangeTree";
import { FileExplorer } from "../../src/ui/file-explorer/FileExplorer";
import { installVirtualLayout } from "../support/virtualLayout";

let restoreLayout: () => void;
beforeEach(() => {
  restoreLayout = installVirtualLayout({ viewportHeight: 280, viewportWidth: 320 });
});
afterEach(() => { cleanup(); restoreLayout(); vi.restoreAllMocks(); });

function filesIn(directory: string[], count: number): ChangeTreeFile[] {
  return Array.from({ length: count }, (_, index) => {
    const segments = [...directory, `file-${index}.ts`];
    return { id: segments.join(":"), displayPath: segments.join("/"), segments, statuses: [{ kind: "committed", change: "modified" }], marker: "M" };
  });
}

function scrollerFor(label = "Files") {
  const list = screen.getByRole("list", { name: label });
  return list.parentElement!;
}

function scroll(scroller: HTMLElement, top: number) {
  scroller.scrollTop = top;
  fireEvent.scroll(scroller);
}

function expectFocusedFile(path: string) {
  expect(document.activeElement).toHaveRole("button");
  expect(document.activeElement).toHaveAccessibleName(`Review ${path}`);
  expect(document.activeElement).toBeVisible();
}

test.each(["tree", "list"] as const)("5000 %s files keep bounded DOM and Tab crosses an unmounted range without activating a file", async (view) => {
  // Focus is committed by each key event's layout effects; no interaction depends on a typing delay.
  const user = userEvent.setup({ delay: null });
  const select = vi.fn();
  render(<><button>Before</button><ChangeTree files={filesIn([], 5000)} label="Files" view={view} onSelect={select} /><button>After</button></>);
  const scroller = scrollerFor();
  const before = screen.getByRole("button", { name: "Before" });
  const after = screen.getByRole("button", { name: "After" });
  // Count every mounted button, including hidden rows, without recomputing each row's accessible name.
  expect(scroller.querySelectorAll("button").length).toBeLessThan(40);
  expect(screen.queryByRole("button", { name: "Review file-40.ts" })).not.toBeInTheDocument();
  await user.click(before);
  await user.tab();
  expectFocusedFile("file-0.ts");
  // One API call avoids per-call scheduling; each Tab still receives its own keydown and keyup.
  await user.keyboard("{Tab}".repeat(40));
  expectFocusedFile("file-40.ts");
  expect(select).not.toHaveBeenCalled();
  expect(scroller.querySelectorAll("button").length).toBeLessThan(40);
  await user.tab({ shift: true });
  expectFocusedFile("file-39.ts");
  await user.keyboard("{Enter}");
  expect(select).toHaveBeenLastCalledWith(expect.objectContaining({ id: "file-39.ts" }));
  await user.click(after);
  await user.tab({ shift: true });
  expectFocusedFile("file-4999.ts");
  await user.tab();
  expect(after).toHaveFocus();
  await user.click(before);
  await user.tab();
  await user.tab({ shift: true });
  expect(before).toHaveFocus();
});

test("expanded hierarchy pins each ancestor once and releases its stack at a sibling section", () => {
  const files = [...filesIn(["a", "nested"], 2500), ...filesIn(["b", "nested"], 2500)];
  render(<ChangeTree files={files} label="Files" onSelect={() => {}}
    directoryExpansion={{ collapsed: new Set(), onToggle: () => {} }} />);
  const scroller = scrollerFor();
  scroll(scroller, 2800);
  const parent = screen.getByRole("button", { name: "Collapse a" });
  const nested = screen.getByRole("button", { name: "Collapse a/nested" });
  expect(parent.closest("li")).toHaveStyle({ top: "2800px" });
  expect(nested.closest("li")).toHaveStyle({ top: "2828px" });
  expect(screen.getAllByRole("button", { name: "Collapse a" })).toHaveLength(1);
  expect(screen.getAllByRole("listitem").length).toBeLessThan(40);
  scroll(scroller, 2502 * 28 + 280);
  expect(screen.getByRole("button", { name: "Collapse b" }).closest("li")).toHaveStyle({ top: `${2502 * 28 + 280}px` });
  expect(screen.getByRole("button", { name: "Collapse b/nested" }).closest("li")).toHaveStyle({ top: `${2502 * 28 + 308}px` });
  expect(screen.queryByRole("button", { name: "Collapse a/nested" })).not.toBeInTheDocument();
});

test("collapsing a focused descendant restores focus to its surviving ancestor", () => {
  const files = filesIn(["src", "nested"], 100);
  const { rerender } = render(<ChangeTree files={files} label="Files" onSelect={() => {}}
    directoryExpansion={{ collapsed: new Set(), onToggle: () => {} }} />);
  scrollerFor();
  act(() => screen.getByRole("button", { name: "Review src/nested/file-0.ts" }).focus());
  rerender(<ChangeTree files={files} label="Files" onSelect={() => {}}
    directoryExpansion={{ collapsed: new Set([changeDirectoryId(["src"])]), onToggle: () => {} }} />);
  expect(screen.getByRole("button", { name: "Expand src" })).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Review src/nested/file-0.ts" })).not.toBeInTheDocument();
});

test("refresh preserves the leading native-segment anchor when earlier rows and native IDs change", () => {
  const files = filesIn([], 5000);
  const { rerender } = render(<ChangeTree files={files} label="Files" view="list" onSelect={() => {}} />);
  const scroller = scrollerFor();
  scroll(scroller, 2807);
  const inserted: ChangeTreeFile = { id: "inserted", displayPath: "inserted.ts", segments: ["inserted.ts"], statuses: [], marker: "" };
  rerender(<ChangeTree files={[inserted, ...files.map((file) => ({ ...file, id: `refresh:${file.id}` }))]} label="Files" view="list" onSelect={() => {}} />);
  expect(scroller.scrollTop).toBe(2835);
  expect(screen.getByRole("button", { name: "Review file-100.ts" })).toBeInTheDocument();
});

test("empty explicit directories report only expanded ancestry, and list mode discovers every directory", async () => {
  const user = userEvent.setup();
  const src = { id: "native-src", displayPath: "src", segments: ["src"] };
  const nested = { id: "native-nested", displayPath: "src/nested", segments: ["src", "nested"] };
  const received: { directories: ChangeTreeDirectory[]; view: "tree" | "list" }[] = [];
  const props = { files: [], treeLabel: "Files", listLabel: "File list", onSelect: () => {}, countLabel: "0 files loaded",
    onExpandedDirectoriesChange: (directories: ChangeTreeDirectory[], view: "tree" | "list") => { received.push({ directories, view }); } };
  const { rerender } = render(<FileExplorer {...props} directories={[src]} />);
  expect(received.at(-1)).toEqual({ directories: [], view: "tree" });
  await user.click(screen.getByRole("button", { name: "Expand src" }));
  expect(received.at(-1)?.directories).toEqual([src]);
  await user.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(received.at(-1)?.directories).toEqual([]);
  rerender(<FileExplorer {...props} directories={[src, nested]} />);
  expect(screen.queryByRole("button", { name: "Collapse src/nested" })).not.toBeInTheDocument();
  expect(received.at(-1)?.directories).toEqual([]);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  expect(received.at(-1)?.directories).toEqual([src, nested]);
  await user.click(screen.getByRole("button", { name: "Collapse all" }));
  await user.click(screen.getByRole("button", { name: "Switch to list view" }));
  expect(received.at(-1)).toEqual({ directories: [src, nested], view: "list" });
  expect(screen.getByText("0 files loaded")).toBeInTheDocument();
});

test("directory notifications do not loop when their owner returns fresh arrays and callbacks", () => {
  const report = vi.fn();
  function Owner() {
    const [directories, setDirectories] = useState([{ id: "native-src", displayPath: "src", segments: ["src"] }]);
    return <FileExplorer files={[]} directories={directories} treeLabel="Files" listLabel="File list" onSelect={() => {}}
      onExpandedDirectoriesChange={(expanded) => { report(expanded); setDirectories([...directories]); }} />;
  }
  render(<Owner />);
  expect(report).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Expand src" })).toBeInTheDocument();
});

test("hiding, reopening and resizing preserves reading position while mounting only the new viewport", () => {
  let height = 280;
  const notify = new Set<() => void>();
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(() => height);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(() => height);
  class ViewportObserver implements ResizeObserver {
    readonly notify: () => void;
    constructor(callback: ResizeObserverCallback) {
      this.notify = () => callback([], this);
      notify.add(this.notify);
    }
    observe() {}
    unobserve() {}
    disconnect() { notify.delete(this.notify); }
  }
  vi.spyOn(window, "ResizeObserver").mockImplementation(function(callback: ResizeObserverCallback) {
    return new ViewportObserver(callback);
  });
  render(<ChangeTree files={filesIn([], 5000)} label="Files" view="list" onSelect={() => {}} />);
  const scroller = scrollerFor();
  scroll(scroller, 2807);
  height = 0;
  act(() => { for (const callback of notify) callback(); });
  scroll(scroller, 0);
  height = 560;
  act(() => { for (const callback of notify) callback(); });
  expect(scroller.scrollTop).toBe(2807);
  fireEvent.scroll(scroller);
  expect(screen.getByRole("button", { name: "Review file-100.ts" })).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /^Review / }).length).toBeLessThan(50);
});

test("reverse Tab entry reaches the final folder when file leaves have no activation", async () => {
  const user = userEvent.setup();
  render(<><ChangeTree files={filesIn(["a"], 5000)} directories={[{ id: "native-z", displayPath: "z", segments: ["z"] }]} label="Files"
    directoryExpansion={{ collapsed: new Set(), onToggle: () => {} }} />
    <button>After</button></>);
  scrollerFor();
  await user.click(screen.getByRole("button", { name: "After" }));
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: "Collapse z" })).toHaveFocus();
});
