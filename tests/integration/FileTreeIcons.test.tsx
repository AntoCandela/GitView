/** Exercises icon choices on actual working and committed tree rows across remounts. */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { AppearanceMenu } from "../../src/features/appearance";
import { Workbench } from "../../src/app/workbench/Workbench";
import { HistoryGraph } from "../../src/features/history";
import { IconThemeProvider } from "../../src/ui/file-icons/IconThemeProvider";
import { fileIcon, folderIcon } from "../../src/ui/file-icons/fileIconLookup";
import type { IconTheme } from "../../src/ui/file-icons/iconThemes";
import { historyClient, historyCommit, historyOids, historyPage } from "../support/history";
import { changedFile, readyObservation } from "../support/review";
import { installVirtualLayout } from "../support/virtualLayout";

let restoreVirtualLayout: () => void;
beforeEach(() => { restoreVirtualLayout = installVirtualLayout(); });
afterEach(() => {
  cleanup();
  restoreVirtualLayout();
  vi.restoreAllMocks();
});

const workingFile = { ...changedFile, displayPath: "src/App.tsx", segments: ["src", "App.tsx"] };
const committedFile = { id: "committed-test", displayPath: "tests/widget.test.ts",
  segments: ["tests", "widget.test.ts"], kind: "added" as const };

function treeClient(): RepositoryClient {
  const client = historyClient(async (entryId) => ({ kind: "page", page: historyPage([
    historyCommit(historyOids.root, [], "Initial commit"),
  ], { entryId }) }));
  client.commitFiles = async (_entryId, commitOid) => ({ kind: "files", commitOid,
    parentOid: null, parents: [], files: [committedFile] });
  return client;
}

function IconWorkbench({ client }: { client: RepositoryClient }) {
  return <IconThemeProvider><AppearanceMenu />
    <Workbench client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository"
      observation={readyObservation([workingFile])}>
      {(comparison) => <HistoryGraph client={client} entryId="one" selectionGeneration={0} comparison={comparison} />}
    </Workbench>
  </IconThemeProvider>;
}

function expectTreeIcons(
  theme: Exclude<IconTheme, "classic">,
  workingTree: HTMLElement,
  committedTree: HTMLElement,
) {
  const working = within(workingTree);
  const committed = within(committedTree);
  expect(working.getByRole("button", { name: "Collapse src" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon(theme, "src", true));
  expect(working.getByRole("button", { name: "Review src/App.tsx" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", fileIcon(theme, "App.tsx"));
  expect(committed.getByRole("button", { name: "Collapse tests" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon(theme, "tests", true));
  expect(committed.getByRole("button", { name: "Review tests/widget.test.ts" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", fileIcon(theme, "widget.test.ts"));
}

test("switching icon packs updates both real trees and closed folders, and the choice restores on remount", async () => {
  localStorage.setItem("gitview.app-theme", "cream");
  localStorage.setItem("gitview.icon-theme", "classic");
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "changes", theme: "match", lineMode: "scroll" }));
  const user = userEvent.setup();
  const client = treeClient();
  const first = render(<IconWorkbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Initial commit, Commit ${historyOids.root}` }));
  const workingTree = screen.getByRole("list", { name: "Changed file hierarchy" });
  const committedTree = await screen.findByRole("list", { name: "Committed file hierarchy" });
  const working = within(workingTree);
  const committed = within(committedTree);
  expect(working.queryByRole("button", { name: "Review src/App.tsx" })).not.toBeInTheDocument();
  expect(committed.queryByRole("button", { name: "Review tests/widget.test.ts" })).not.toBeInTheDocument();
  await user.click(working.getByRole("button", { name: "Expand src" }));
  await user.click(committed.getByRole("button", { name: "Expand tests" }));
  expect(working.getByRole("button", { name: "Review src/App.tsx" }).querySelector("img.tree-entry-icon")).toBeNull();
  expect(committed.getByRole("button", { name: "Review tests/widget.test.ts" }).querySelector("img.tree-entry-icon")).toBeNull();
  expect(working.getByRole("button", { name: "Review src/App.tsx" }).querySelector("svg")).not.toBeNull();
  expect(committed.getByRole("button", { name: "Review tests/widget.test.ts" }).querySelector("svg")).not.toBeNull();

  const appearanceButton = screen.getByRole("button", { name: "Appearance" });
  await user.click(appearanceButton);
  const materialDialog = await screen.findByRole("dialog", { name: "Appearance" });
  const materialAppearance = within(materialDialog);
  await user.click(materialAppearance.getByText("File icons"));
  const choices = within(materialAppearance.getByRole("radiogroup", { name: "File icons" }));
  await user.click(choices.getByRole("radio", { name: "Material" }));
  expectTreeIcons("material", workingTree, committedTree);
  await user.click(working.getByRole("button", { name: "Collapse src" }));
  await waitFor(() => expect(materialDialog).not.toBeInTheDocument());
  expect(working.getByRole("button", { name: "Expand src" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon("material", "src", false));
  await user.click(committed.getByRole("button", { name: "Collapse tests" }));
  expect(committed.getByRole("button", { name: "Expand tests" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon("material", "tests", false));

  await user.click(appearanceButton);
  const catppuccinDialog = await screen.findByRole("dialog", { name: "Appearance" });
  const catppuccinAppearance = within(catppuccinDialog);
  await user.click(catppuccinAppearance.getByText("File icons"));
  await user.click(within(catppuccinAppearance.getByRole("radiogroup", { name: "File icons" }))
    .getByRole("radio", { name: "Catppuccin Latte" }));
  expect(working.getByRole("button", { name: "Expand src" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon("catppuccin", "src", false));
  expect(committed.getByRole("button", { name: "Expand tests" }).querySelector("img.tree-entry-icon"))
    .toHaveAttribute("src", folderIcon("catppuccin", "tests", false));
  await user.click(working.getByRole("button", { name: "Expand src" }));
  await waitFor(() => expect(catppuccinDialog).not.toBeInTheDocument());
  await user.click(committed.getByRole("button", { name: "Expand tests" }));
  expectTreeIcons("catppuccin", workingTree, committedTree);
  expect(localStorage.getItem("gitview.icon-theme")).toBe("catppuccin");

  first.unmount();
  render(<IconWorkbench client={client} />);
  await user.click(await screen.findByRole("button", { name: `Initial commit, Commit ${historyOids.root}` }));
  const restoredWorking = screen.getByRole("list", { name: "Changed file hierarchy" });
  const restoredCommitted = await screen.findByRole("list", { name: "Committed file hierarchy" });
  await user.click(within(restoredWorking).getByRole("button", { name: "Expand src" }));
  await user.click(within(restoredCommitted).getByRole("button", { name: "Expand tests" }));
  expectTreeIcons("catppuccin", restoredWorking, restoredCommitted);
});
