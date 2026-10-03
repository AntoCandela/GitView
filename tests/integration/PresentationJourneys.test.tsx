/** Exercises presentation transitions on the real workspace; renderer fixtures do not prove native or visual behavior. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryClient, WorkspaceSnapshot } from "../../src/contracts/repositories";
import { Workspace } from "../../src/app/Workspace";
import { historyOids, historyPage, mergeHistory } from "../support/history";
import { reviewClient, textReview } from "../support/review";
import { installVirtualLayout } from "../support/virtualLayout";

const preferenceKeys = ["gitview.app-theme", "gitview.icon-theme", "gitview.code-review"];
let savedPreferences: Array<string | null>;
let restoreVirtualLayout: () => void;

beforeEach(() => {
  savedPreferences = preferenceKeys.map((key) => localStorage.getItem(key));
  localStorage.setItem("gitview.app-theme", "cream");
  localStorage.setItem("gitview.icon-theme", "classic");
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "changes", theme: "match", lineMode: "scroll" }));
  // Keep a ten-line source viewport while measuring the real virtualized lists.
  restoreVirtualLayout = installVirtualLayout({ viewportWidth: 300, clientHeight: 220 });
});
afterEach(() => {
  cleanup();
  restoreVirtualLayout();
  vi.restoreAllMocks();
  preferenceKeys.forEach((key, index) => {
    const previous = savedPreferences[index];
    if (previous === null) localStorage.removeItem(key);
    else localStorage.setItem(key, previous);
  });
});

function presentationClient(): RepositoryClient {
  const client = reviewClient(async (entryId, _revision, pathId, category) => textReview("working presentation source", {
    entryId, pathId, category, from: category === "staged" ? "HEAD" : "index",
    to: category === "staged" ? "index" : "working_files",
  }));
  const snapshot: WorkspaceSnapshot = {
    revision: 1, restoring: false, persistenceError: null, activeContextId: "one",
    entries: [{ id: "one", kind: "working_tree", repositoryLabel: "atlas", locationLabel: "/fixture/atlas",
      head: { kind: "branch", name: "main" }, availability: "available" }],
  };
  client.snapshot = async () => snapshot;
  client.refreshEntryAvailability = async () => snapshot;
  client.historyPage = async () => ({ kind: "page", page: historyPage(mergeHistory()) });
  client.commitFiles = async (_entryId, commitOid, parentOid) => ({
    kind: "files", commitOid, parentOid: parentOid ?? historyOids.first, parents: [historyOids.first, historyOids.second],
    files: [{ id: "committed-file", displayPath: "archive/version.ts", segments: ["archive", "version.ts"], kind: "modified" }],
  });
  client.reviewCommitFile = async (entryId, commitOid, parentOid, fileId) => ({
    kind: "text", entryId, commitOid, parentOid, fileId, displayPath: "archive/version.ts", contextLabel: "atlas",
    fromAbsent: false, toAbsent: false, fromContent: "parent presentation source\n", toContent: "committed presentation source\n",
    hunks: [{ oldStart: 1, oldCount: 1, newStart: 1, newCount: 1, lines: [
      { kind: "removal", text: "parent presentation source" }, { kind: "addition", text: "committed presentation source" },
    ] }],
  });
  return client;
}

function mountWorkspace(client = presentationClient()) {
  const view = render(<Workspace client={client} />);
  // Reload the shared preference stores from this scenario's isolated storage values.
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: null })));
  return view;
}

async function chooseLayout(user: UserEvent, trigger: HTMLElement, name: string) {
  await user.click(trigger);
  const dialog = await screen.findByRole("dialog", { name: "Workbench layout" });
  const option = within(within(dialog).getByRole("radiogroup", { name: "Panel arrangement" })).getByRole("radio", { name });
  await user.click(option);
  expect(option).toBeChecked();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(dialog).not.toBeInTheDocument());
}

const historyAbove = "Graph above, Preview bottom left, Files bottom right";
const filesAbove = "Files above, Graph bottom left, Preview bottom right";

test.each([
  { context: "working", path: "src/example.ts", source: "working presentation source", other: "committed presentation source" },
  { context: "committed", path: "archive/version.ts", source: "committed presentation source", other: "working presentation source" },
])("the selected $context comparison survives layout and appearance changes", async ({ context, path, source, other }) => {
  // Keep application timers real without adding artificial timer ticks between interactions.
  const user = userEvent.setup({ delay: null });
  mountWorkspace();
  // Keep accessible-name scans local to the active surface, not every mounted workbench control.
  const layout = screen.getByRole("button", { name: "Workbench layout" });
  const toolbar = within(layout.closest("header")!);
  const ancestry = within(await screen.findByRole("region", { name: "Commit ancestry" }));
  const commit = await ancestry.findByRole("button", { name: `Merge topic, Commit ${historyOids.merge}` });
  await user.click(commit);
  const committedFiles = within(await ancestry.findByRole("list", { name: "Committed file hierarchy" }));
  await user.click(await committedFiles.findByRole("button", { name: "Expand archive" }));
  const workingFiles = within(screen.getByRole("list", { name: "Changed file hierarchy" }));
  await user.click(workingFiles.getByRole("button", { name: "Expand src" }));
  await committedFiles.findByRole("button", { name: "Review archive/version.ts" });
  const selectedFiles = context === "working" ? workingFiles : committedFiles;
  await user.click(selectedFiles.getByRole("button", { name: `Review ${path}` }));
  expect(await screen.findByText(source)).toBeVisible();

  await chooseLayout(user, layout, historyAbove);
  await user.click(toolbar.getByRole("button", { name: "Appearance" }));
  const appearance = await screen.findByRole("dialog", { name: "Appearance" });
  await user.click(within(appearance).getByRole("button", { name: "Midnight" }));
  await user.keyboard("{Escape}");
  await waitFor(() => expect(appearance).not.toBeInTheDocument());
  await chooseLayout(user, layout, filesAbove);

  expect(selectedFiles.getByRole("button", { name: `Review ${path}` })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("heading", { name: path })).toBeVisible();
  expect(screen.getByText(source)).toBeVisible();
  expect(screen.queryByText(other)).not.toBeInTheDocument();
  expect(commit).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("list", { name: "Committed file hierarchy" })).toBeVisible();
  expect(screen.getAllByRole("region", { name: "Read-only file comparison" })).toHaveLength(1);
  expect(screen.getByRole("region", { name: "Read-only file comparison" }))
    .toHaveStyle({ "--code-bg": "#24292e", "--code-ink": "#e1e4e8" });
});

test("saved reading and palette choices restore after remount while the session layout resets", async () => {
  // Keep real application timers, but do not add artificial typing/pointer delays to this persistence journey.
  const user = userEvent.setup({ delay: null });
  const client = presentationClient();
  const first = mountWorkspace(client);
  // Query each active surface locally so unrelated workbench controls do not dominate the journey.
  const workingFiles = within(await screen.findByRole("list", { name: "Changed file hierarchy" }));
  await user.click(await workingFiles.findByRole("button", { name: "Expand src" }));
  await user.click(await workingFiles.findByRole("button", { name: "Review src/example.ts" }));
  const selectedReview = within(await screen.findByRole("region", { name: "Selected file review" }));
  await selectedReview.findByText("working presentation source");
  const layout = screen.getByRole("button", { name: "Workbench layout" });
  const toolbar = within(layout.closest("header")!);
  await user.click(layout);
  const initialDialog = await screen.findByRole("dialog", { name: "Workbench layout" });
  const initialLayouts = within(within(initialDialog).getByRole("radiogroup", { name: "Panel arrangement" }));
  const initialLayout = initialLayouts.getByRole("radio", { checked: true }).getAttribute("aria-label")!;
  // Change the layout in the already-open menu; a second close/open adds no persistence coverage.
  const changedLayout = initialLayouts.getByRole("radio", { name: historyAbove });
  await user.click(changedLayout);
  expect(changedLayout).toBeChecked();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(initialDialog).not.toBeInTheDocument());
  await user.click(within(selectedReview.getByRole("radiogroup", { name: "Code view" })).getByRole("radio", { name: "Full file" }));
  await user.click(within(selectedReview.getByRole("radiogroup", { name: "Long lines" })).getByRole("radio", { name: "Wrap" }));
  await user.click(toolbar.getByRole("button", { name: "Appearance" }));
  const appearance = within(await screen.findByRole("dialog", { name: "Appearance" }));
  await user.click(appearance.getByRole("button", { name: "Midnight" }));
  await user.click(appearance.getByText("Syntax"));
  await user.click(within(appearance.getByRole("radiogroup", { name: "Syntax palette" })).getByRole("radio", { name: "GitHub Light" }));
  expect(JSON.parse(localStorage.getItem("gitview.code-review")!))
    .toEqual({ mode: "full", theme: "github-light", lineMode: "wrap" });
  expect(localStorage.getItem("gitview.app-theme")).toBe("midnight");

  first.unmount();
  mountWorkspace(client);
  const restoredFiles = within(await screen.findByRole("list", { name: "Changed file hierarchy" }));
  await user.click(await restoredFiles.findByRole("button", { name: "Expand src" }));
  await user.click(await restoredFiles.findByRole("button", { name: "Review src/example.ts" }));
  const restoredReview = within(await screen.findByRole("region", { name: "Selected file review" }));
  expect(await restoredReview.findByText("working presentation source")).toBeVisible();
  expect(within(restoredReview.getByRole("radiogroup", { name: "Code view" })).getByRole("radio", { name: "Full file" })).toBeChecked();
  expect(within(restoredReview.getByRole("radiogroup", { name: "Long lines" })).getByRole("radio", { name: "Wrap" })).toBeChecked();
  expect(document.documentElement).toHaveAttribute("data-appearance", "midnight");
  expect(restoredReview.getByRole("region", { name: "Read-only file comparison" }))
    .toHaveStyle({ "--code-bg": "#fff", "--code-ink": "#24292e" });
  await user.click(screen.getByRole("button", { name: "Workbench layout" }));
  const restoredDialog = await screen.findByRole("dialog", { name: "Workbench layout" });
  const restoredLayouts = within(within(restoredDialog).getByRole("radiogroup", { name: "Panel arrangement" }));
  expect(restoredLayouts.getByRole("radio", { name: initialLayout })).toBeChecked();
  expect(restoredLayouts.getByRole("radio", { name: historyAbove })).not.toBeChecked();
});

test("keyboard layout navigation and nested repository menus return focus without losing the comparison", async () => {
  const user = userEvent.setup();
  mountWorkspace();
  await user.click(await screen.findByRole("button", { name: "Expand src" }));
  await user.click(await screen.findByRole("button", { name: "Review src/example.ts" }));
  await screen.findByText("working presentation source");
  const layout = screen.getByRole("button", { name: "Workbench layout" });
  layout.focus();
  await user.keyboard("{Enter}");
  await waitFor(() => expect(within(screen.getByRole("radiogroup", { name: "Panel arrangement" }))
    .getByRole("radio", { checked: true })).toHaveFocus());
  await user.keyboard("{End}");
  expect(screen.getByRole("radio", { name: historyAbove })).toBeChecked();
  expect(screen.getByRole("radio", { name: historyAbove })).toHaveFocus();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(layout).toHaveFocus());

  const repository = screen.getByRole("button", { name: "Current repository: atlas" });
  repository.focus();
  await user.keyboard(" ");
  expect(repository).toHaveAttribute("aria-expanded", "true");
  const actions = within(screen.getByRole("navigation", { name: "Repositories" }))
    .getByRole("button", { name: "Actions for atlas" });
  actions.focus();
  await user.keyboard("{Enter}");
  await waitFor(() => expect(screen.getByRole("menuitem", { name: "Rename" })).toHaveFocus());
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("menuitem", { name: "Remove from sidebar" })).toHaveFocus();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(actions).toHaveFocus());
  expect(repository).toHaveAttribute("aria-expanded", "true");
  await user.tab({ shift: true });
  expect(screen.getByRole("button", { name: "atlas main" })).toHaveFocus();
  await screen.findByRole("tooltip");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("tooltip")).not.toBeInTheDocument());
  await user.keyboard("{Escape}");
  expect(repository).toHaveFocus();
  expect(repository).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByRole("button", { name: "Review src/example.ts" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByText("working presentation source")).toBeVisible();
});

// Eight full-workspace transitions need scheduling headroom in the shared parallel run.
test("repeated layout and reading-mode transitions keep a late source line reachable with bounded mounted rows", async () => {
  const user = userEvent.setup();
  const lines = Array.from({ length: 10_000 }, (_, index) => `presentation source ${index}`);
  const review = textReview("");
  review.fromContent = review.toContent = `${lines.join("\n")}\n`;
  review.hunks = [{ oldStart: 1, oldCount: lines.length, newStart: 1, newCount: lines.length,
    lines: lines.map((text) => ({ kind: "context", text })) }];
  const client = presentationClient();
  client.reviewFile = async () => review;
  mountWorkspace(client);
  const layout = screen.getByRole("button", { name: "Workbench layout" });
  const workingFiles = within(await screen.findByRole("list", { name: "Changed file hierarchy" }));
  await user.click(workingFiles.getByRole("button", { name: "Expand src" }));
  const selectedFile = workingFiles.getByRole("button", { name: "Review src/example.ts" });
  await user.click(selectedFile);
  const selectedReview = within(await screen.findByRole("region", { name: "Selected file review" }));
  const modes = within(selectedReview.getByRole("radiogroup", { name: "Code view" }));
  const comparison = await selectedReview.findByRole("region", { name: "Read-only file comparison" });
  const sourcePanes = within(comparison);
  const next = sourcePanes.getByRole("region", { name: "New source hunks" });
  next.scrollTop = 26 + 9000 * 22;
  fireEvent.scroll(next);

  // Repetition is deliberate: each mounted view must stay bounded, not only the initial render.
  for (let cycle = 0; cycle < 4; cycle++) {
    await chooseLayout(user, layout, historyAbove);
    await user.click(modes.getByRole("radio", { name: "Full file" }));
    expect(within(sourcePanes.getByRole("region", { name: "New source file" })).getByText("presentation source 9000")).toBeVisible();
    expect(document.querySelectorAll(".diff-line").length).toBeLessThan(200);
    await chooseLayout(user, layout, filesAbove);
    await user.click(modes.getByRole("radio", { name: "Changes" }));
    const old = sourcePanes.getByRole("region", { name: "Old source hunks" });
    const current = sourcePanes.getByRole("region", { name: "New source hunks" });
    expect(within(old).getByText("presentation source 9000")).toBeVisible();
    expect(within(current).getByText("presentation source 9000")).toBeVisible();
    expect(within(current).queryByText("presentation source 0")).not.toBeInTheDocument();
    expect(old.querySelectorAll(".diff-line").length).toBeLessThan(100);
    expect(current.querySelectorAll(".diff-line").length).toBeLessThan(100);
    expect(screen.getAllByRole("region", { name: "Read-only file comparison" })).toHaveLength(1);
  }
  expect(screen.getByRole("button", { name: "Review src/example.ts" })).toHaveAttribute("aria-pressed", "true");
}, 10_000);
