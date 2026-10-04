/** Covers complete descendant status summaries independently of collapsed or partially loaded rows. */

import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ChangeTree, type ChangeTreeFile } from "../../src/ui/file-explorer/ChangeTree";
import { FileExplorer } from "../../src/ui/file-explorer/FileExplorer";
import { RepositoryFiles } from "../../src/features/repositories/files/RepositoryFiles";
import { changedFile, readyObservation, reviewClient } from "../support/review";

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(280);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(280);
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(320);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function file(segments: string[], ...statuses: ChangeTreeFile["statuses"]): ChangeTreeFile {
  return { id: JSON.stringify(segments), displayPath: segments.join("/"), segments, statuses, marker: "M" };
}

test("collapsed folders count all nested changes by status, including both sides of one file", async () => {
  const user = userEvent.setup();
  render(<ChangeTree label="Changes" files={[
    file(["src", "nested", "both.ts"], { kind: "staged", change: "modified" }, { kind: "unstaged", change: "modified" }),
    file(["src", "other.ts"], { kind: "unstaged", change: "modified" }),
    file(["src", "nested", "added.ts"], { kind: "staged", change: "added" }),
    file(["src", "nested", "deleted.ts"], { kind: "unstaged", change: "deleted" }),
    file(["src", "new.ts"], { kind: "untracked" }),
    file(["src", "conflict.ts"], { kind: "conflict" }),
    file(["src", "module"], { kind: "unsupported", change: "submodule" }),
    file(["src", "unchanged.ts"], { kind: "unchanged" }),
    file(["src-other", "excluded.ts"], { kind: "unstaged", change: "modified" }),
  ]} onSelect={() => {}} />);
  const folder = screen.getByRole("button", { name: "Expand src" });
  expect(folder).not.toHaveAttribute("title");
  expect(screen.queryByRole("button", { name: /^Review / })).not.toBeInTheDocument();
  await user.hover(folder);
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("src");
  expect(tooltip).toHaveTextContent("Staged Modified: 1");
  expect(tooltip).toHaveTextContent("Unstaged Modified: 2");
  expect(tooltip).toHaveTextContent("Staged Added: 1");
  expect(tooltip).toHaveTextContent("Unstaged Deleted: 1");
  expect(tooltip).toHaveTextContent("Untracked: 1");
  expect(tooltip).toHaveTextContent("Conflict: 1");
  expect(tooltip).toHaveTextContent("Submodule · unsupported: 1");
  expect(tooltip).not.toHaveTextContent("Unchanged");
});

test("committed-style statuses default to the complete files even when descendants are collapsed", async () => {
  const user = userEvent.setup();
  render(<ChangeTree label="Committed files" files={[
    file(["docs", "deep", "added.md"], { kind: "committed", change: "added" }),
    file(["docs", "deep", "modified.md"], { kind: "committed", change: "modified" }),
    file(["docs", "removed.md"], { kind: "committed", change: "deleted" }),
  ]} />);
  await user.hover(screen.getByRole("button", { name: "Expand docs" }));
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("Added: 1");
  expect(tooltip).toHaveTextContent("Modified: 1");
  expect(tooltip).toHaveTextContent("Deleted: 1");
});

test("sidebar summaries include changed descendants before their native directory page is loaded", async () => {
  const user = userEvent.setup();
  const client = reviewClient();
  client.listRepositoryFiles = async (entryId, request) => ({
    kind: "files", entryId, listingId: "listing", directoryId: request.directoryId, cursor: null,
    files: [], directories: [{ id: "native-src", displayPath: "src", segments: ["src"] }],
  });
  render(<RepositoryFiles client={client} entryId="one" enabled observation={readyObservation([
    { ...changedFile, displayPath: "src/unloaded/deep.ts", segments: ["src", "unloaded", "deep.ts"] },
  ])} selected={null} onSelect={() => {}} />);
  const folder = await screen.findByRole("button", { name: "Expand src" });
  expect(screen.getByText("0 files loaded")).toBeVisible();
  expect(screen.queryByRole("button", { name: /^Review / })).not.toBeInTheDocument();
  await user.hover(folder);
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("Staged Modified: 1");
  expect(tooltip).toHaveTextContent("Unstaged Modified: 1");
});

test.each([true, false])("sidebar distinguishes no known changes from unavailable status (ready: %s)", async (ready) => {
  const user = userEvent.setup();
  const client = reviewClient();
  client.listRepositoryFiles = async (entryId, request) => ({
    kind: "files", entryId, listingId: "listing", directoryId: request.directoryId, cursor: null,
    files: [{ id: "readme", displayPath: "README.md", segments: ["README.md"] }],
    directories: [{ id: "empty", displayPath: "empty", segments: ["empty"] }],
  });
  render(<RepositoryFiles client={client} entryId="one" enabled observation={ready ? readyObservation([]) : null}
    selected={null} onSelect={() => {}} />);
  await user.hover(await screen.findByRole("button", { name: "Expand empty" }));
  expect(await screen.findByRole("tooltip")).toHaveTextContent(ready ? "No known changes" : "Status unavailable");
  await user.unhover(screen.getByRole("button", { name: "Expand empty" }));
  await user.hover(screen.getByRole("button", { name: "Review README.md" }));
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("README.md");
  expect(tooltip).toHaveTextContent(ready ? "Unchanged" : "Status unavailable");
});

test("file and view controls use shared tooltips without competing native titles", async () => {
  const user = userEvent.setup();
  render(<FileExplorer files={[file(["src", "nested", "file.ts"], { kind: "staged", change: "modified" }, { kind: "unstaged", change: "deleted" })]}
    treeLabel="Files" listLabel="File list" onSelect={() => {}} />);
  const expand = screen.getByRole("button", { name: "Expand all" });
  expect(expand).not.toHaveAttribute("title");
  await user.hover(expand);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Expand all");
  await user.unhover(expand);
  const toggle = screen.getByRole("button", { name: "Switch to list view" });
  expect(toggle).not.toHaveAttribute("title");
  await user.hover(toggle);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Switch to list view");
  await user.click(toggle);
  await user.unhover(toggle);
  const row = screen.getByRole("button", { name: "Review src/nested/file.ts" });
  expect(row).not.toHaveAttribute("title");
  await user.hover(row);
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("src/nested/file.ts");
  expect(tooltip).toHaveTextContent("Staged Modified, Unstaged Deleted");
});
