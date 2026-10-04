/** Exercises shared repository selection without granting admission or management controls. */

import "@testing-library/jest-dom/vitest";
import { useRef, useState, type ComponentProps } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RepositoryEntry } from "../../src/contracts/repositories";
import { RepositoryBrowser, RepositorySelector } from "../../src/features/repositories";

const entries: RepositoryEntry[] = [
  { id: "atlas", kind: "working_tree", repositoryLabel: "Atlas", locationLabel: "/work/atlas",
    head: { kind: "branch", name: "main" }, availability: "available" },
  { id: "ledger", kind: "working_tree", repositoryLabel: "Ledger", locationLabel: "/work/ledger",
    head: { kind: "branch", name: "topic" }, availability: "available" },
];

beforeEach(() => {
  // jsdom has no layout; stable dimensions let the virtualizer mount both rows.
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.classList.contains("repository-list") ? 480 : 44;
  });
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(296);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

function SelectionBrowser({ selectionDisabled = false, actionsDisabled = false, onSelect }: {
  selectionDisabled?: boolean;
  actionsDisabled?: boolean;
  onSelect: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  return <RepositoryBrowser query={query} onQueryChange={setQuery} list={{
    entries: entries.filter((entry) => entry.repositoryLabel.toLowerCase().includes(query.toLowerCase())),
    emptyMessage: "No matches", selectedId: "atlas", onSelect, actionsDisabled, selectionDisabled,
  }} />;
}

test("selection-only browsing searches and selects admitted repositories without management or admission controls", async () => {
  const user = userEvent.setup();
  const selected: string[] = [];
  render(<SelectionBrowser onSelect={(id) => selected.push(id)} />);
  expect(screen.queryByRole("button", { name: "Open repository" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Actions for/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Atlas main" })).toHaveAttribute("aria-current", "true");
  expect(screen.getByRole("button", { name: "Atlas main" })).toHaveAccessibleDescription("/work/atlas");
  await user.type(screen.getByRole("searchbox", { name: "Search repositories" }), "ledger");
  expect(screen.queryByRole("button", { name: "Atlas main" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Ledger topic" }));
  expect(selected).toEqual(["ledger"]);
  await user.click(screen.getByRole("button", { name: "Clear search" }));
  expect(screen.getByRole("button", { name: "Atlas main" })).toBeInTheDocument();
});

test("selection locking prevents pointer and keyboard requests until unlocked", async () => {
  const user = userEvent.setup();
  const selected: string[] = [];
  const onSelect = (id: string) => selected.push(id);
  const view = render(<SelectionBrowser onSelect={onSelect} />);
  const ledger = screen.getByRole("button", { name: "Ledger topic" });
  ledger.focus();
  view.rerender(<SelectionBrowser selectionDisabled onSelect={onSelect} />);
  expect(ledger).toBeDisabled();
  await user.keyboard("{Enter} ");
  await user.click(ledger);
  fireEvent.click(ledger);
  expect(selected).toEqual([]);
  view.rerender(<SelectionBrowser onSelect={onSelect} />);
  await user.click(ledger);
  expect(selected).toEqual(["ledger"]);
});

test("disabled management actions do not lock ordinary repository selection", async () => {
  const user = userEvent.setup();
  const selected: string[] = [];
  render(<SelectionBrowser actionsDisabled onSelect={(id) => selected.push(id)} />);
  const atlas = screen.getByRole("button", { name: "Atlas main" });
  atlas.focus();
  await user.keyboard("{ArrowDown}");
  expect(screen.getByRole("button", { name: "Ledger topic" })).toHaveFocus();
  expect(selected).toEqual([]);
  await user.keyboard("{Enter}");
  expect(selected).toEqual(["ledger"]);
});

function Selector({ disabled = false, onEscape }: { disabled?: boolean; onEscape?: () => void }) {
  const [open, setOpen] = useState(false);
  const controlsRef = useRef<HTMLDivElement>(null);
  return <div onKeyDown={(event) => { if (event.key === "Escape") onEscape?.(); }}>
    <RepositorySelector active={entries[0]} open={open} onOpenChange={setOpen} controlsRef={controlsRef} disabled={disabled}>
      <SelectionBrowser onSelect={() => {
        setOpen(false);
        controlsRef.current?.querySelector<HTMLButtonElement>(".repository-selector")?.focus();
      }} />
    </RepositorySelector>
    <button type="button">Outside selector</button>
  </div>;
}

test("selector Escape restores its trigger without dismissing an enclosing surface", async () => {
  const user = userEvent.setup();
  const outerEscape = vi.fn();
  render(<Selector onEscape={outerEscape} />);
  const trigger = screen.getByRole("button", { name: "Current repository: Atlas" });
  await user.click(trigger);
  await user.click(screen.getByRole("searchbox", { name: "Search repositories" }));
  await user.keyboard("{Escape}");
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
  expect(outerEscape).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(outerEscape).toHaveBeenCalledTimes(1);
});

test("outside dismissal retains outside focus and caller-owned selection focus restoration remains usable", async () => {
  const user = userEvent.setup();
  render(<Selector />);
  const trigger = screen.getByRole("button", { name: "Current repository: Atlas" });
  await user.click(trigger);
  const outside = screen.getByRole("button", { name: "Outside selector" });
  await user.click(outside);
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(outside).toHaveFocus();
  await user.click(trigger);
  await user.click(screen.getByRole("button", { name: "Ledger topic" }));
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  expect(trigger).toHaveFocus();
});

test("disabled selector cannot open repository choices", async () => {
  const user = userEvent.setup();
  render(<Selector disabled />);
  const trigger = screen.getByRole("button", { name: "Current repository: Atlas" });
  expect(trigger).toBeDisabled();
  await user.click(trigger);
  fireEvent.click(trigger);
  expect(screen.queryByRole("searchbox")).not.toBeInTheDocument();
});

test("each selector exposes its own disclosure relationship", () => {
  const onOpenChange = vi.fn<ComponentProps<typeof RepositorySelector>["onOpenChange"]>();
  render(<>
    <RepositorySelector active={entries[0]} open onOpenChange={onOpenChange}><button>Atlas choice</button></RepositorySelector>
    <RepositorySelector active={entries[1]} open onOpenChange={onOpenChange}><button>Ledger choice</button></RepositorySelector>
  </>);
  const atlas = screen.getByRole("button", { name: "Current repository: Atlas" });
  const ledger = screen.getByRole("button", { name: "Current repository: Ledger" });
  const atlasDisclosure = document.getElementById(atlas.getAttribute("aria-controls")!);
  const ledgerDisclosure = document.getElementById(ledger.getAttribute("aria-controls")!);
  expect(atlasDisclosure).not.toBe(ledgerDisclosure);
  expect(within(atlasDisclosure!).getByRole("button", { name: "Atlas choice" })).toBeInTheDocument();
  expect(within(ledgerDisclosure!).getByRole("button", { name: "Ledger choice" })).toBeInTheDocument();
});
