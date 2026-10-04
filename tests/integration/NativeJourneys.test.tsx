/** Exercises accessible Workspace choices through the production adapter and a real disposable native service. */

import "@testing-library/jest-dom/vitest";
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { act, cleanup, render, screen, waitFor, within, type BoundFunctions, type queries } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { Workspace } from "../../src/app/Workspace";
import { repositoryClient } from "../../src/platform/RepositoryClient";
import { buildNativeJourney, NativeJourney, type JourneyFixtureInfo, type JourneySafety } from "../support/nativeJourney";
import { installVirtualLayout } from "../support/virtualLayout";

// Only the host transport is replaced: all DTOs and Git results come from RepositoryService.
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const nativeWait = { timeout: 15_000 };
const journeyTimeout = 60_000;
let journey: NativeJourney;
let fixture: JourneyFixtureInfo;
let restoreVirtualLayout: () => void;

beforeAll(buildNativeJourney, 610_000);
beforeEach(async () => {
  localStorage.clear();
  restoreVirtualLayout = installVirtualLayout({ viewportHeight: 480, viewportWidth: 640, clientHeight: 480 });
  journey = await NativeJourney.start();
  fixture = await journey.request<JourneyFixtureInfo>("fixture_info");
  vi.mocked(invoke).mockImplementation(<T,>(command: string, args?: InvokeArgs) => journey.request<T>(command, args as Record<string, unknown>));
}, 30_000);

afterEach(async () => {
  cleanup();
  restoreVirtualLayout();
  try {
    if (journey) {
      const safety = await journey.request<JourneySafety>("fixture_verify");
      expect(safety).toEqual({ repositoriesIntact: true, gitStateUnchanged: true, workingBytesExpected: true });
    }
  } finally {
    try { if (journey) await journey.close(); }
    finally { vi.restoreAllMocks(); localStorage.clear(); }
  }
}, 30_000);

function sidebar() {
  return within(screen.getByRole("complementary", { name: "Workspace sidebar" }));
}

async function showRepositories(user: UserEvent) {
  const selector = screen.getByRole("button", { name: /^Current repository:/ });
  if (selector.getAttribute("aria-expanded") !== "true") await user.click(selector);
}

async function selectRepository(user: UserEvent, label: string) {
  await showRepositories(user);
  await user.click(await screen.findByRole("button", { name: `${label} ${fixture.mainBranch}` }, nativeWait));
}

function changedFiles() {
  return within(screen.getByRole("complementary", { name: "Files" }));
}

async function expandPath(user: UserEvent, scope: BoundFunctions<typeof queries>, path: string) {
  const segments = path.split("/");
  for (let depth = 1; depth < segments.length; depth++) {
    const directory = segments.slice(0, depth).join("/");
    const toggle = await scope.findByRole("button", {
      name: (name) => name === `Expand ${directory}` || name === `Collapse ${directory}`,
    }, nativeWait);
    if (toggle.getAttribute("aria-expanded") === "false") await user.click(toggle);
  }
}

async function openRepository(user: UserEvent, repository: "main" | "other" | "cancel") {
  await journey.request("fixture_choose", { repository });
  await showRepositories(user);
  await user.click(screen.getByRole("button", { name: "Open repository" }));
}

async function mountMain() {
  const user = userEvent.setup();
  const view = render(<Workspace />);
  await openRepository(user, "main");
  await selectRepository(user, fixture.mainLabel);
  await expandPath(user, within(await screen.findByRole("complementary", { name: "Files" }, nativeWait)), fixture.workingPath);
  await changedFiles().findByRole("button", { name: `Review ${fixture.workingPath}` }, nativeWait);
  return { user, view };
}

async function reviewWorkingFile(user: UserEvent) {
  const files = within(await screen.findByRole("complementary", { name: "Files" }, nativeWait));
  await expandPath(user, files, fixture.workingPath);
  await user.click(await files.findByRole("button", { name: `Review ${fixture.workingPath}` }, nativeWait));
}

async function expectWorkingText(text: string) {
  if (process.platform === "win32") {
    expect(await screen.findByRole("heading", { name: "Preview unsupported" }, nativeWait)).toBeVisible();
    expect(screen.queryByRole("region", { name: "Read-only file comparison" })).not.toBeInTheDocument();
  } else {
    const pane = await screen.findByRole("region", { name: "New source hunks" }, nativeWait);
    await waitFor(() => expect(pane).toHaveTextContent(text.trim()), nativeWait);
  }
}

async function activeEntryId() {
  const snapshot = await repositoryClient.snapshot();
  expect(snapshot.activeContextId).not.toBeNull();
  return snapshot.activeContextId!;
}

async function repositoryAction(user: UserEvent, label: string, action: "Rename" | "Remove from sidebar") {
  await showRepositories(user);
  const trigger = screen.getByRole("button", { name: `Actions for ${label}` });
  await waitFor(() => expect(trigger).toBeEnabled(), nativeWait);
  await user.click(trigger);
  await user.click(await screen.findByRole("menuitem", { name: action }, nativeWait));
}

test("admission, cancellation, switching, display-name rename and sidebar removal survive a native service restart", async () => {
  const { user, view } = await mountMain();
  await openRepository(user, "cancel");
  expect(screen.getByRole("button", { name: `Current repository: ${fixture.mainLabel}` })).toBeVisible();
  await openRepository(user, "other");
  await selectRepository(user, fixture.otherLabel);
  expect(await screen.findByRole("button", { name: `Current repository: ${fixture.otherLabel}` }, nativeWait)).toBeVisible();
  expect(await screen.findByRole("heading", { name: "Clean" }, nativeWait)).toBeVisible();
  await selectRepository(user, fixture.mainLabel);
  await expandPath(user, within(await screen.findByRole("complementary", { name: "Files" }, nativeWait)), fixture.workingPath);
  await changedFiles().findByRole("button", { name: `Review ${fixture.workingPath}` }, nativeWait);
  await repositoryAction(user, fixture.mainLabel, "Rename");
  const name = screen.getByRole("textbox", { name: "Display name" });
  await user.clear(name);
  await user.type(name, "My review repository{Enter}");
  expect(await screen.findByRole("button", { name: "Current repository: My review repository" }, nativeWait)).toBeVisible();
  await repositoryAction(user, fixture.otherLabel, "Remove from sidebar");
  await waitFor(() => expect(screen.queryByRole("button", { name: `${fixture.otherLabel} ${fixture.mainBranch}` })).not.toBeInTheDocument(), nativeWait);
  view.unmount();
  await journey.request("fixture_restart");
  render(<Workspace />);
  expect(await screen.findByRole("button", { name: "Current repository: My review repository" }, nativeWait)).toBeVisible();
  await showRepositories(user);
  expect(await screen.findByRole("button", { name: `My review repository ${fixture.mainBranch}` }, nativeWait)).toHaveAttribute("aria-current", "true");
  expect(screen.queryByRole("button", { name: `${fixture.otherLabel} ${fixture.mainBranch}` })).not.toBeInTheDocument();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
}, journeyTimeout);

test("staged, unstaged and untracked review displays their actual Git endpoints without mixing bytes", async () => {
  const { user } = await mountMain();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  expect(screen.getByRole("button", { name: "Unstaged comparison" })).toHaveAttribute("aria-pressed", "true");
  if (process.platform !== "win32") {
    expect(screen.getByRole("region", { name: "Old source hunks" })).toHaveTextContent(fixture.indexText.trim());
    expect(screen.getByText("Old · Index")).toBeVisible();
    expect(screen.getByText("New · Working files")).toBeVisible();
  }
  await user.click(screen.getByRole("button", { name: "Staged comparison" }));
  await waitFor(() => expect(screen.getByRole("region", { name: "New source hunks" })).toHaveTextContent(fixture.indexText.trim()), nativeWait);
  expect(screen.getByRole("region", { name: "Old source hunks" })).toHaveTextContent(fixture.headText.trim());
  expect(screen.getByText("Old · HEAD")).toBeVisible();
  expect(screen.getByText("New · Index")).toBeVisible();
  expect(screen.queryByText(fixture.workingText.trim())).not.toBeInTheDocument();
  await expandPath(user, changedFiles(), fixture.untrackedPath);
  await user.click(changedFiles().getByRole("button", { name: `Review ${fixture.untrackedPath}` }));
  await expectWorkingText(fixture.untrackedText);
  expect(screen.getByRole("button", { name: "Untracked comparison" })).toHaveAttribute("aria-pressed", "true");
  if (process.platform !== "win32") {
    expect(screen.getByText("Old · Absent (new file)")).toBeVisible();
    expect(screen.getByRole("region", { name: "Old source hunks" })).not.toHaveTextContent(fixture.indexText.trim());
  }
}, journeyTimeout);

test("same-status external edits refresh the selected comparison and render hostile source as inert text", async () => {
  const { user } = await mountMain();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  const entryId = await activeEntryId();
  const before = await repositoryClient.observeSelectedContext(entryId);
  expect(before.kind).toBe("ready");
  const hostile = '<img src=x onerror="globalThis.journeyInjected=true">';
  await act(async () => { await journey.request("fixture_edit", { text: `${hostile}\n` }); });
  await expectWorkingText(hostile);
  expect(changedFiles().getByRole("button", { name: `Review ${fixture.workingPath}` })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Unstaged comparison" })).toHaveAttribute("aria-pressed", "true");
  const review = screen.getByRole("region", { name: "Selected file review" });
  expect(review.querySelector("img")).toBeNull();
  expect(screen.queryByText(fixture.workingText.trim())).not.toBeInTheDocument();
  const after = await repositoryClient.observeSelectedContext(entryId);
  expect(after).toEqual(before);
}, journeyTimeout);

test("merge parent review, history pagination and root review return safely to live working content", async () => {
  const { user } = await mountMain();
  const merge = await screen.findByRole("button", { name: new RegExp(`^${fixture.mergeSubject}, Commit `) }, nativeWait);
  await user.click(merge);
  await expandPath(user, within(await screen.findByRole("region", { name: /^Changed files for commit / }, nativeWait)), "src/topic.txt");
  await user.click(await screen.findByRole("button", { name: "Review src/topic.txt" }, nativeWait));
  expect(await screen.findByText("topic contribution", {}, nativeWait)).toBeVisible();
  const parent = screen.getByRole("combobox", { name: "Comparison parent" });
  await user.selectOptions(parent, within(parent).getAllByRole("option")[1]);
  await expandPath(user, within(await screen.findByRole("region", { name: /^Changed files for commit / }, nativeWait)), "src/main.txt");
  await user.click(await screen.findByRole("button", { name: "Review src/main.txt" }, nativeWait));
  expect(await screen.findByText("main contribution", {}, nativeWait)).toBeVisible();
  expect(screen.queryByText("topic contribution")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: new RegExp(`^${fixture.rootSubject}, Commit `) })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Load more" }));
  const root = await screen.findByRole("button", { name: new RegExp(`^${fixture.rootSubject}, Commit `) }, nativeWait);
  expect(merge).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("main contribution")).toBeVisible();
  await user.click(root);
  expect(screen.queryByRole("combobox", { name: "Comparison parent" })).not.toBeInTheDocument();
  const expansion = await screen.findByRole("region", { name: /^Changed files for commit / }, nativeWait);
  await expandPath(user, within(expansion), fixture.workingPath);
  await user.click(await within(expansion).findByRole("button", { name: `Review ${fixture.workingPath}` }, nativeWait));
  expect(await screen.findByText(fixture.headText.trim(), {}, nativeWait)).toBeVisible();
  expect(screen.getByText("Old · Empty tree (root commit)")).toBeVisible();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  expect(screen.getByRole("button", { name: "Unstaged comparison" })).toHaveAttribute("aria-pressed", "true");
}, journeyTimeout);

test("view-only branch browsing and linked-worktree navigation preserve HEAD, index, refs and working bytes", async () => {
  const { user } = await mountMain();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  const mainEntryId = await activeEntryId();
  await user.click(screen.getByRole("button", { name: `View branch or worktree: ${fixture.mainBranch}` }));
  await user.click(await screen.findByRole("button", { name: `View branch ${fixture.topicBranch}` }, nativeWait));
  expect(screen.getByRole("button", { name: `View branch or worktree: ${fixture.topicBranch}` })).toBeVisible();
  expect(await screen.findByRole("button", { name: /^Journey topic, Commit / }, nativeWait)).toBeVisible();
  await waitFor(() => expect(screen.queryByRole("button", { name: new RegExp(`^${fixture.mergeSubject}, Commit `) })).not.toBeInTheDocument(), nativeWait);
  await expectWorkingText(fixture.workingText);
  expect(await activeEntryId()).toBe(mainEntryId);
  expect(await journey.request<JourneySafety>("fixture_verify")).toEqual({ repositoriesIntact: true, gitStateUnchanged: true, workingBytesExpected: true });
  await user.click(screen.getByRole("button", { name: `View branch or worktree: ${fixture.topicBranch}` }));
  await user.type(screen.getByRole("searchbox", { name: "Search branches and worktrees" }), "journey-linked");
  await user.click(await screen.findByRole("button", { name: "Open worktree journey-linked for branch journey-linked" }, nativeWait));
  expect(await screen.findByRole("button", { name: "View branch or worktree: journey-linked" }, nativeWait)).toBeVisible();
  expect(await screen.findByRole("heading", { name: "Clean" }, nativeWait)).toBeVisible();
  expect(await activeEntryId()).not.toBe(mainEntryId);
  await selectRepository(user, fixture.mainLabel);
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  expect(await activeEntryId()).toBe(mainEntryId);
}, journeyTimeout);

test("repository browsing previews an unchanged file through native-issued authority and returns to changed-file review", async () => {
  const { user } = await mountMain();
  expect(changedFiles().queryByRole("button", { name: `Review ${fixture.unchangedPath}` })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand sidebar" }));
  if (process.platform !== "darwin" && process.platform !== "linux") {
    expect(await sidebar().findByRole("alert", {}, nativeWait)).toBeVisible();
    expect(sidebar().queryByRole("button", { name: `Review ${fixture.unchangedPath}` })).not.toBeInTheDocument();
    return;
  }
  await expandPath(user, sidebar(), fixture.unchangedPath);
  const unchanged = await sidebar().findByRole("button", { name: `Review ${fixture.unchangedPath}` }, nativeWait);
  await user.click(unchanged);
  expect(await screen.findByText(fixture.unchangedText.trim(), {}, nativeWait)).toBeVisible();
  expect(unchanged).toHaveAttribute("aria-pressed", "true");
  await user.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  expect(screen.getByText(fixture.unchangedText.trim())).toBeVisible();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  expect(screen.queryByText(fixture.unchangedText.trim())).not.toBeInTheDocument();
}, journeyTimeout);

test("a saved repository missing at restart stays unavailable and recovers without replacing user data with empty success", async () => {
  const { user, view } = await mountMain();
  await reviewWorkingFile(user);
  await expectWorkingText(fixture.workingText);
  view.unmount();
  await journey.request("fixture_hide");
  let restored = false;
  try {
    await journey.request("fixture_restart");
    render(<Workspace />);
    const retry = await screen.findByRole("button", { name: "Check again" }, nativeWait);
    expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
    expect(screen.queryByText(fixture.workingText.trim())).not.toBeInTheDocument();
    const missing = await repositoryClient.snapshot();
    expect(missing.entries).toEqual([expect.objectContaining({ repositoryLabel: fixture.mainLabel, availability: "unavailable" })]);
    await journey.request("fixture_restore");
    restored = true;
    await user.click(retry);
    await reviewWorkingFile(user);
    await expectWorkingText(fixture.workingText);
    expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: `Current repository: ${fixture.mainLabel}` }, nativeWait)).toBeVisible();
  } finally {
    if (!restored) await journey.request("fixture_restore");
  }
}, journeyTimeout);
