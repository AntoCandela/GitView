/** Exercises native hierarchy and filename-driven review choices through the compact file sidebar. */

import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import type { ChangedPath, ObservationSnapshot } from "../../src/contracts/changes";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { Workbench as WorkbenchFeature } from "../../src/app/workbench/Workbench";
import type { ObservationView } from "../../src/features/changes/useObservation";
import { reviewClient, reviewIdentity, textReview } from "../support/review";

afterEach(cleanup);
const defaultClient = reviewClient(async (entryId, _revision, pathId, category) => textReview("preview", { entryId, pathId, category }));
function Workbench({ observation, client = defaultClient }: { observation: ObservationView; client?: RepositoryClient }) {
  return <WorkbenchFeature client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository" observation={observation} />;
}
const partial: ChangedPath = {
  pathId: "revision-one", stablePathId: "stable-one", displayPath: "src/deep/example.ts",
  segments: ["src", "deep", "example.ts"], staged: "added", unstaged: "modified",
  untracked: false, conflict: false, unsupportedKind: null,
};
const ready = (files: ChangedPath[], observationRevision = 1): ObservationSnapshot => ({ entryId: "one", observationRevision, kind: "ready", files });

test("directories collapse and expand without duplicating a partially staged filename", async () => {
  const user = userEvent.setup();
  render(<Workbench observation={ready([partial])} />);
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.getAllByRole("button", { name: "Review src/deep/example.ts" })).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "Collapse src" }));
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand src" }));
  await user.click(screen.getByRole("button", { name: "Collapse src/deep" }));
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand src/deep" }));
  expect(screen.getByRole("button", { name: "Review src/deep/example.ts" })).toBeVisible();
});

test("expand all opens nested directories even when their ancestor is collapsed", async () => {
  const user = userEvent.setup();
  const other = { ...partial, stablePathId: "other", displayPath: "docs/guide.md", segments: ["docs", "guide.md"] };
  render(<Workbench observation={ready([partial, other])} />);
  expect(screen.queryByRole("button", { name: /^Review / })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.getByRole("button", { name: "Review src/deep/example.ts" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Review docs/guide.md" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Collapse src/deep" })).toHaveAttribute("aria-expanded", "true");
});

test("global folder control collapses and expands every level without losing the selected comparison", async () => {
  const user = userEvent.setup();
  const root = { ...partial, stablePathId: "root", displayPath: "root.txt", segments: ["root.txt"] };
  render(<Workbench observation={ready([partial, root])} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/deep/example.ts" }));
  expect(await screen.findByText("preview")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.getByRole("button", { name: "Expand src" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Review root.txt" })).toBeVisible();
  expect(screen.getByText("2 files")).toBeVisible();
  expect(screen.getByText("preview")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Expand src" }));
  expect(screen.getByRole("button", { name: "Expand src/deep" })).toHaveAttribute("aria-expanded", "false");
  const expand = screen.getByRole("button", { name: "Expand all" });
  expand.focus();
  await user.keyboard("{Enter}");
  expect(screen.getByRole("button", { name: "Review src/deep/example.ts" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Collapse src/deep" })).toHaveAttribute("aria-expanded", "true");
  await user.keyboard(" ");
  expect(screen.getByRole("button", { name: "Expand src" })).toHaveAttribute("aria-expanded", "false");
});

test("global folder action ignores collapsed paths removed by a newer observation", async () => {
  const user = userEvent.setup();
  const { rerender } = render(<Workbench observation={ready([partial])} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Collapse src" }));
  const other = { ...partial, stablePathId: "other", displayPath: "docs/guide.md", segments: ["docs", "guide.md"] };
  rerender(<Workbench observation={ready([other], 2)} />);
  await user.click(screen.getByRole("button", { name: "Collapse all" }));
  expect(screen.getByRole("button", { name: "Expand docs" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("button", { name: "Review docs/guide.md" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.getByRole("button", { name: "Review docs/guide.md" })).toBeVisible();
});

test("flat view preserves selection, status and collapsed directories through keyboard switching", async () => {
  const user = userEvent.setup();
  const duplicate = { ...partial, stablePathId: "other", displayPath: "docs/example.ts", segments: ["docs", "example.ts"], staged: null, unstaged: null, untracked: true };
  render(<Workbench observation={ready([partial, duplicate])} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/deep/example.ts" }));
  expect(await screen.findByText("preview")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Collapse src/deep" }));
  const toggle = screen.getByRole("button", { name: "Switch to list view" });
  toggle.focus();
  await user.keyboard("{Enter}");
  expect(screen.getByRole("button", { name: "Switch to tree view" })).toHaveTextContent("List");
  expect(screen.queryByRole("button", { name: /^Collapse / })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeDisabled();
  const selected = screen.getByRole("button", { name: "Review src/deep/example.ts" });
  expect(selected).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByLabelText("Staged Added, Unstaged Modified")).toHaveTextContent("MM");
  await user.click(screen.getByRole("button", { name: "Review docs/example.ts" }));
  expect(screen.getByRole("button", { name: "Untracked comparison" })).toHaveAttribute("aria-pressed", "true");
  screen.getByRole("button", { name: "Switch to tree view" }).focus();
  await user.keyboard(" ");
  expect(screen.getByRole("button", { name: "Expand src/deep" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByRole("button", { name: "Review docs/example.ts" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Untracked comparison" })).toHaveAttribute("aria-pressed", "true");
});

test("file count follows snapshots rather than visible leaves or staged categories", async () => {
  const user = userEvent.setup();
  const { rerender } = render(<Workbench observation={ready([partial])} />);
  expect(screen.getByText("1 file")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Collapse src" }));
  expect(screen.getByText("1 file")).toBeVisible();
  rerender(<Workbench observation={ready([partial, { ...partial, stablePathId: "root", displayPath: "root.txt", segments: ["root.txt"] }], 2)} />);
  expect(screen.getByText("2 files")).toBeVisible();
  rerender(<Workbench observation={ready([], 3)} />);
  expect(screen.getByText("0 files")).toBeVisible();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeDisabled();
  rerender(<Workbench observation={{ kind: "transport_unavailable" }} />);
  expect(screen.queryByText("0 files")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand all" })).toBeDisabled();
});

test("expanded directories do not leak into a new repository context", async () => {
  const user = userEvent.setup();
  const { rerender } = render(<WorkbenchFeature client={defaultClient} entryId="one" selectionGeneration={0} contextLabel="Sample" observation={ready([partial])} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  rerender(<WorkbenchFeature client={defaultClient} entryId="two" selectionGeneration={1} contextLabel="Other" observation={{ ...ready([partial]), entryId: "two" }} />);
  expect(screen.getByRole("button", { name: "Expand src" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
});

test("filename activation defaults to unstaged and exposes partially staged alternatives only in the preview", async () => {
  const user = userEvent.setup();
  render(<Workbench observation={ready([partial])} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  expect(screen.queryByRole("button", { name: "Staged comparison" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Review src/deep/example.ts" }));
  expect(await screen.findByText("preview")).toBeVisible();
  expect(screen.getByRole("button", { name: "Unstaged comparison" })).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: "Staged comparison" })).toHaveAttribute("aria-pressed", "false");
  await user.click(screen.getByRole("button", { name: "Staged comparison" }));
  expect(screen.getByRole("button", { name: "Staged comparison" })).toHaveAttribute("aria-pressed", "true");
});

test("snapshot replacement removes old paths and introduces native-segment nested leaves", async () => {
  const { rerender } = render(<Workbench observation={ready([partial])} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  rerender(<Workbench observation={ready([{ ...partial, pathId: "new-revision", stablePathId: "new-file", displayPath: "docs/guide.txt", segments: ["docs", "guide.txt"], staged: null, unstaged: null, untracked: true }], 2)} />);
  expect(screen.queryByRole("button", { name: "Review src/deep/example.ts" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Collapse src" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Collapse docs" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Review docs/guide.txt" })).toBeVisible();
});

test("native filename segments are not split by display separators and default untracked comparison is available", async () => {
  render(<Workbench observation={ready([{ ...partial, displayPath: "native\\name.txt", segments: ["native\\name.txt"], staged: null, unstaged: null, untracked: true }])} />);
  expect(screen.queryByRole("button", { name: /^Collapse/ })).not.toBeInTheDocument();
  await userEvent.setup().click(screen.getByRole("button", { name: "Review native\\name.txt" }));
  expect(screen.getByRole("button", { name: "Untracked comparison" })).toHaveAttribute("aria-pressed", "true");
});

test.each(["rename_or_copy", "submodule", "type_change"] as const)("unsupported %s filenames remain selectable for a truthful preview state", async (unsupportedKind) => {
  const client = reviewClient(async (entryId, _revision, pathId, category) => ({ kind: "unsupported", reason: unsupportedKind, identity: { ...reviewIdentity, entryId, pathId, category } }));
  render(<Workbench client={client} observation={ready([{ ...partial, unsupportedKind }])} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  await userEvent.setup().click(screen.getByRole("button", { name: "Review src/deep/example.ts" }));
  expect(await screen.findByRole("heading", { name: "Preview unsupported" })).toBeVisible();
  expect(screen.queryByRole("region", { name: "Read-only file comparison" })).not.toBeInTheDocument();
});

test("conflict filenames without ordinary category flags still reach the explicit conflict outcome", async () => {
  const client = reviewClient(async (entryId, _revision, pathId, category) => ({ kind: "unsupported", reason: "conflict", identity: { ...reviewIdentity, entryId, pathId, category } }));
  render(<Workbench client={client} observation={ready([{ ...partial, staged: null, unstaged: null, conflict: true }])} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  await userEvent.setup().click(screen.getByRole("button", { name: "Review src/deep/example.ts" }));
  expect(await screen.findByRole("heading", { name: "Preview unsupported" })).toBeVisible();
  expect(screen.queryByRole("heading", { name: "No remaining changes" })).not.toBeInTheDocument();
});

test.each([
  { entryId: "one", observationRevision: 1, kind: "checking" },
  { entryId: "one", observationRevision: 1, kind: "bare" },
  { entryId: "one", observationRevision: 1, kind: "unavailable", errorCode: "inaccessible" },
  { kind: "transport_unavailable" },
] as const)("non-ready state $kind never implies a clean working tree", (observation) => {
  render(<Workbench observation={observation} />);
  expect(screen.queryByRole("heading", { name: "Clean" })).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
});
