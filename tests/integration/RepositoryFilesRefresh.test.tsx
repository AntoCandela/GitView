/** Exercises mounted repository-tree reading anchors across deferred native listing replacements. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryDirectory, RepositoryFile, RepositoryFilesResult } from "../../src/contracts/browsing";
import { RepositoryFiles } from "../../src/features/repositories/files/RepositoryFiles";
import { deferred } from "../support/deferred";
import { readyObservation, reviewClient } from "../support/review";
import { installVirtualLayout } from "../support/virtualLayout";

let restoreLayout: () => void;
beforeEach(() => {
  vi.useFakeTimers();
  restoreLayout = installVirtualLayout({ viewportHeight: 280 });
});
afterEach(() => { cleanup(); restoreLayout(); vi.restoreAllMocks(); vi.useRealTimers(); });

function files(listing: string, count = 240): RepositoryFile[] {
  return Array.from({ length: count }, (_, index) => {
    const name = `file-${String(index).padStart(3, "0")}.ts`;
    return { id: `${listing}-${index}`, displayPath: `src/${name}`, segments: ["src", name] };
  });
}
function page(listingId: string, directoryId: string | null, files: RepositoryFile[], directories: RepositoryDirectory[] = [], cursor: string | null = null): RepositoryFilesResult {
  return { kind: "files", entryId: "one", listingId, directoryId, files, directories, cursor };
}
async function pageTurn() { await act(async () => { await vi.advanceTimersByTimeAsync(16); }); }
function scroll(scroller: HTMLElement, top: number) {
  scroller.scrollTop = top;
  fireEvent.scroll(scroller);
}
function treeScroller() { return screen.getByRole("list", { name: "Repository file hierarchy" }).parentElement!; }
function expectAnchor(scroller: HTMLElement, top: number) {
  expect(treeScroller()).toBe(scroller);
  expect(scroller.scrollTop).toBe(top);
  const row = screen.getByRole("button", { name: "Review src/file-100.ts" }).closest("li")!;
  expect(Number.parseFloat(row.style.top) - scroller.scrollTop).toBe(-7);
  expect(screen.getByRole("button", { name: "Collapse src" })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getAllByRole("listitem").length).toBeLessThan(40);
}

test.each(["explicit refresh", "ready revision"])("%s preserves the reading anchor through delayed, multi-page replacement without relabeling old authority", async (refresh) => {
  const client = reviewClient();
  const onSelect = vi.fn();
  const oldFiles = files("old");
  const replacement = files("new");
  const oldDirectory = { id: "old-src", displayPath: "src", segments: ["src"] };
  const newDirectory = { ...oldDirectory, id: "new-src" };
  const root = deferred<RepositoryFilesResult>();
  const firstPage = deferred<RepositoryFilesResult>();
  const lastPage = deferred<RepositoryFilesResult>();
  client.listRepositoryFiles = async (_entry, request) => request.directoryId === null
    ? page("old", null, [], [oldDirectory]) : page("old", oldDirectory.id, oldFiles);
  const props = { client, entryId: "one", enabled: true, observation: readyObservation([], 1), selected: null, onSelect };
  const { rerender } = render(<RepositoryFiles {...props} />);
  await pageTurn();
  fireEvent.click(screen.getByRole("button", { name: "Expand src" }));
  await pageTurn();
  expect(screen.getByText("240 files")).toBeVisible();
  const scroller = treeScroller();
  scroll(scroller, 2835);
  expectAnchor(scroller, 2835);

  client.listRepositoryFiles = async (_entry, request) => request.directoryId === null ? root.promise
    : request.cursor === null ? firstPage.promise : lastPage.promise;
  if (refresh === "explicit refresh") fireEvent.click(screen.getByRole("button", { name: "Refresh files" }));
  else rerender(<RepositoryFiles {...props} observation={readyObservation([], 2)} />);
  expectAnchor(scroller, 2835);
  await pageTurn();
  expectAnchor(scroller, 2835);
  fireEvent.click(screen.getByRole("button", { name: "Review src/file-100.ts" }));
  expect(onSelect).toHaveBeenLastCalledWith({ ...oldFiles[100], listingId: "old" });

  await act(async () => root.resolve(page("new", null, [], [newDirectory])));
  await pageTurn();
  expectAnchor(scroller, 2835);
  await act(async () => firstPage.resolve(page("new", newDirectory.id, replacement.slice(0, 80), [], "next")));
  await pageTurn();
  expectAnchor(scroller, 2835);
  fireEvent.click(screen.getByRole("button", { name: "Review src/file-100.ts" }));
  expect(onSelect).toHaveBeenLastCalledWith({ ...oldFiles[100], listingId: "old" });

  const inserted = { id: "new-inserted", displayPath: "src/a-new.ts", segments: ["src", "a-new.ts"] };
  await act(async () => lastPage.resolve(page("new", newDirectory.id, [inserted, ...replacement.slice(80)])));
  await pageTurn();
  expectAnchor(scroller, 2863);
  expect(screen.getByText("241 files")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Review src/file-100.ts" }));
  expect(onSelect).toHaveBeenLastCalledWith({ ...replacement[100], listingId: "new" });
});

test.each(["client", "repository", "same-ID selection"])("changing %s scope hides old rows and rejects their late continuation", async (scope) => {
  const oldClient = reviewClient();
  const nextClient = scope === "client" ? reviewClient() : oldClient;
  const late = deferred<RepositoryFilesResult>();
  const next = deferred<RepositoryFilesResult>();
  const oldFile = { id: "old-file", displayPath: "old.ts", segments: ["old.ts"] };
  const newFile = { id: "new-file", displayPath: "new.ts", segments: ["new.ts"] };
  oldClient.listRepositoryFiles = async (_entry, request) => request.cursor === null
    ? page("old", null, [oldFile], [], "next") : late.promise;
  const onSelect = vi.fn();
  const props = { client: oldClient, entryId: "one", enabled: true, observation: readyObservation([], 1), selected: null, onSelect };
  const { rerender } = render(<RepositoryFiles key="one:0" {...props} />);
  await pageTurn();
  await pageTurn();
  expect(screen.getByRole("button", { name: "Review old.ts" })).toBeVisible();

  nextClient.listRepositoryFiles = () => next.promise;
  const entryId = scope === "repository" ? "two" : "one";
  rerender(<RepositoryFiles key={scope === "same-ID selection" ? "one:1" : "one:0"} {...props} client={nextClient} entryId={entryId} />);
  expect(screen.queryByRole("button", { name: "Review old.ts" })).not.toBeInTheDocument();
  await pageTurn();
  await act(async () => late.resolve(page("old", null, [{ id: "late", displayPath: "late.ts", segments: ["late.ts"] }])));
  expect(screen.queryByRole("button", { name: "Review late.ts" })).not.toBeInTheDocument();
  await act(async () => next.resolve({ kind: "files", entryId, listingId: "new", directoryId: null, cursor: null, files: [newFile], directories: [] }));
  fireEvent.click(screen.getByRole("button", { name: "Review new.ts" }));
  expect(onSelect).toHaveBeenCalledExactlyOnceWith({ ...newFile, listingId: "new" });
});

test("a shorter replacement preserves the surviving leading row even when layout clamps the old scroll range", async () => {
  const client = reviewClient();
  const oldDirectory = { id: "old-src", displayPath: "src", segments: ["src"] };
  const newDirectory = { ...oldDirectory, id: "new-src" };
  client.listRepositoryFiles = async (_entry, request) => request.directoryId === null
    ? page("old", null, [], [oldDirectory]) : page("old", oldDirectory.id, files("old"));
  render(<RepositoryFiles client={client} entryId="one" enabled observation={readyObservation([], 1)} selected={null} onSelect={() => {}} />);
  await pageTurn();
  fireEvent.click(screen.getByRole("button", { name: "Expand src" }));
  await pageTurn();
  const scroller = treeScroller();
  // jsdom does not clamp scrollTop after content shrink; model that browser-owned layout behavior.
  let requestedTop = 0;
  Object.defineProperty(scroller, "scrollTop", {
    configurable: true,
    get: () => Math.min(requestedTop, Math.max(0, scroller.scrollHeight - scroller.clientHeight)),
    set: (top: number) => { requestedTop = top; },
  });
  scroll(scroller, 2835);
  expectAnchor(scroller, 2835);
  client.listRepositoryFiles = async (_entry, request) => request.directoryId === null
    ? page("new", null, [], [newDirectory]) : page("new", newDirectory.id, files("new").slice(100, 120));
  fireEvent.click(screen.getByRole("button", { name: "Refresh files" }));
  await pageTurn();
  await pageTurn();
  expectAnchor(scroller, 35);
  expect(screen.getByText("20 files")).toBeVisible();
});
