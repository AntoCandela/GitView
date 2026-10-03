/** Covers directory-page completeness, paused traversal and stale native replies. */

import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryFilesResult } from "../../src/contracts/browsing";
import { useRepositoryFiles } from "../../src/features/repositories/files/useRepositoryFiles";
import { deferred } from "../support/deferred";
import { reviewClient } from "../support/review";

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.useRealTimers(); });
const directory = { id: "docs", displayPath: "docs", segments: ["docs"] };
const first = { id: "readme", displayPath: "README.md", segments: ["README.md"] };
const nested = { id: "guide", displayPath: "docs/guide.md", segments: ["docs", "guide.md"] };
async function pageTurn() { await act(async () => { await vi.advanceTimersByTimeAsync(16); }); }

test("a completed root page is still partial until discovered directories complete", async () => {
  const client = reviewClient();
  client.listRepositoryFiles = async (_entryId, request) => request.directoryId === null
    ? { kind: "files", entryId: "one", listingId: "listing", directoryId: null, cursor: null, files: [first], directories: [directory] }
    : { kind: "files", entryId: "one", listingId: "listing", directoryId: directory.id, cursor: null, files: [nested], directories: [] };
  const { result } = renderHook(() => useRepositoryFiles(client, "one", true, 1));
  await pageTurn();
  expect(result.current.files).toEqual([first]);
  expect(result.current.complete).toBe(false);
  await pageTurn();
  expect(result.current.files).toEqual([first]);
  act(() => result.current.expandDirectories([directory]));
  await pageTurn();
  expect(result.current.files).toEqual([first, nested]);
  expect(result.current.complete).toBe(true);
});

test("collapse accepts an in-flight page but pauses its continuation without losing cursor authority", async () => {
  const client = reviewClient();
  const pending = deferred<RepositoryFilesResult>();
  const last = { id: "last", displayPath: "docs/last.md", segments: ["docs", "last.md"] };
  client.listRepositoryFiles = async (_entryId, request) => {
    if (request.directoryId === null) return { kind: "files", entryId: "one", listingId: "listing", directoryId: null,
      cursor: null, files: [], directories: [directory] };
    if (request.cursor === null) return pending.promise;
    if (request.cursor !== "next") throw new Error("Unexpected continuation");
    return { kind: "files", entryId: "one", listingId: "listing", directoryId: directory.id,
      cursor: null, files: [last], directories: [] };
  };
  const { result } = renderHook(() => useRepositoryFiles(client, "one", true, 1));
  await pageTurn();
  act(() => result.current.expandDirectories([directory]));
  await pageTurn();
  act(() => result.current.expandDirectories([]));
  await act(async () => pending.resolve({ kind: "files", entryId: "one", listingId: "listing", directoryId: directory.id,
    cursor: "next", files: [nested], directories: [] }));
  await pageTurn();
  expect(result.current.files).toEqual([nested]);
  expect(result.current.complete).toBe(false);
  act(() => result.current.expandDirectories([directory]));
  await pageTurn();
  expect(result.current.files).toEqual([nested, last]);
  expect(result.current.complete).toBe(true);
});

test("a late page from the previous repository cannot replace current files", async () => {
  const client = reviewClient();
  const pending = deferred<RepositoryFilesResult>();
  client.listRepositoryFiles = async (entryId) => entryId === "one" ? pending.promise
    : { kind: "files", entryId, listingId: "new", directoryId: null, cursor: null, files: [nested], directories: [] };
  const { result, rerender } = renderHook(({ entryId }) => useRepositoryFiles(client, entryId, true, 1), { initialProps: { entryId: "one" } });
  await pageTurn();
  rerender({ entryId: "two" });
  await pageTurn();
  await act(async () => pending.resolve({ kind: "files", entryId: "one", listingId: "old", directoryId: null,
    cursor: null, files: [first], directories: [] }));
  expect(result.current.listingId).toBe("new");
  expect(result.current.files).toEqual([nested]);
});

test("a failed continuation keeps loaded files usable without claiming a complete repository", async () => {
  const client = reviewClient();
  client.listRepositoryFiles = async (_entryId, request) => request.cursor === null
    ? { kind: "files", entryId: "one", listingId: "listing", directoryId: null, cursor: "next", files: [first], directories: [] }
    : { kind: "unavailable", code: "inaccessible", message: "Directory changed. Refresh files." };
  const { result } = renderHook(() => useRepositoryFiles(client, "one", true, 1));
  await pageTurn();
  await pageTurn();
  expect(result.current.files).toEqual([first]);
  expect(result.current.listingId).toBe("listing");
  expect(result.current.complete).toBe(false);
  expect(result.current.error).not.toBeNull();
});

test("a delayed refresh keeps the previous listing usable and reports a replacement failure without claiming completion", async () => {
  const client = reviewClient();
  const pending = deferred<RepositoryFilesResult>();
  client.listRepositoryFiles = async () => ({ kind: "files", entryId: "one", listingId: "old", directoryId: null,
    cursor: null, files: [first], directories: [] });
  const { result } = renderHook(() => useRepositoryFiles(client, "one", true, 1));
  await pageTurn();
  client.listRepositoryFiles = () => pending.promise;
  act(() => result.current.refresh());
  expect(result.current.files).toEqual([first]);
  expect(result.current.listingId).toBe("old");
  expect(result.current.complete).toBe(false);
  await pageTurn();
  await act(async () => pending.resolve({ kind: "unavailable", code: "inaccessible", message: "Refresh unavailable." }));
  expect(result.current.files).toEqual([first]);
  expect(result.current.listingId).toBe("old");
  expect(result.current.complete).toBe(false);
  expect(result.current.error).toBe("Refresh unavailable.");
});

test("replacement commits the reachable pages without waiting for collapsed branches or accumulating older rows", async () => {
  const client = reviewClient();
  client.listRepositoryFiles = async (_entry, request) => request.directoryId === null
    ? { kind: "files", entryId: "one", listingId: "old", directoryId: null, cursor: null, files: [first], directories: [directory] }
    : { kind: "files", entryId: "one", listingId: "old", directoryId: directory.id, cursor: null, files: [nested], directories: [] };
  const { result } = renderHook(() => useRepositoryFiles(client, "one", true, 1));
  await pageTurn();
  act(() => result.current.expandDirectories([directory]));
  await pageTurn();
  expect(result.current.files).toEqual([first, nested]);
  act(() => result.current.expandDirectories([]));
  const replacement = { id: "current", displayPath: "current.md", segments: ["current.md"] };
  const nextDirectory = { ...directory, id: "new-docs" };
  client.listRepositoryFiles = async () => ({ kind: "files", entryId: "one", listingId: "new", directoryId: null,
    cursor: null, files: [replacement], directories: [nextDirectory] });
  act(() => result.current.refresh());
  await pageTurn();
  expect(result.current.files).toEqual([replacement]);
  expect(result.current.listingId).toBe("new");
  expect(result.current.complete).toBe(false);
  client.listRepositoryFiles = async () => ({ kind: "files", entryId: "one", listingId: "latest", directoryId: null,
    cursor: null, files: [], directories: [] });
  act(() => result.current.refresh());
  await pageTurn();
  expect(result.current.files).toEqual([]);
  expect(result.current.directories).toEqual([]);
  expect(result.current.listingId).toBe("latest");
  expect(result.current.complete).toBe(true);
});
