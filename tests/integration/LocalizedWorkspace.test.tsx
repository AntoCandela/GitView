/** Exercises live localized repository feedback without altering raw file identities or mounted selection. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { createLocaleStore, installLocaleStoreForTests, setLocaleChoice, useLocale, type Locale } from "../../src/i18n";
import { useWorkspace, workspaceErrorMessage } from "../../src/features/repositories";
import { ChangeTree, type ChangeTreeFile } from "../../src/ui/file-explorer/ChangeTree";
import { summarizeDirectoryChanges } from "../../src/ui/file-explorer/changeTreeRows";
import { reviewClient } from "../support/review";
import userEvent from "@testing-library/user-event";
import { FileExplorer } from "../../src/ui/file-explorer/FileExplorer";

let restoreLocale: () => void;
beforeEach(() => {
  const values = new Map<string, string>();
  restoreLocale = installLocaleStoreForTests(createLocaleStore({
    storage: () => ({
      getItem: (key) => values.get(key) ?? null,
      setItem: (key, value) => { values.set(key, value); },
      removeItem: (key) => { values.delete(key); },
    }),
    document,
  }));
});
afterEach(() => { cleanup(); restoreLocale(); });
it.each<[Locale, string]>([
  ["pt-BR", "Esta pasta não é um repositório Git."],
  ["pt-PT", "Esta pasta não é um repositório Git."],
  ["it", "Questa cartella non è un repository Git."],
  ["es", "Esta carpeta no es un repositorio Git."],
  ["en-US", "This folder is not a Git repository."],
  ["en-GB", "This folder is not a Git repository."],
])("%s retranslates an already visible coded rejection without repeating the request", async (locale, expected) => {
  await setLocaleChoice("it");
  const client = reviewClient();
  let requests = 0;
  client.openChosenRepository = async () => {
    requests++;
    return { kind: "rejected", code: "not_repository", snapshot: await client.snapshot() };
  };
  function Feedback() {
    const workspace = useWorkspace(client);
    const { locale } = useLocale();
    return <><button onClick={workspace.openRepository}>open</button>{workspace.error ? <p role="alert">{workspaceErrorMessage(workspace.error, locale)}</p> : null}</>;
  }
  render(<Feedback />);
  fireEvent.click(screen.getByRole("button", { name: "open" }));
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("non è un repository Git"));
  await act(async () => setLocaleChoice(locale));
  expect(screen.getByRole("alert")).toHaveTextContent(expected);
  expect(requests).toBe(1);
});

it("pt-PT keeps typed counts and raw selected filenames across grammar and locale changes", async () => {
  await setLocaleChoice("en-US");
  const files: ChangeTreeFile[] = [{ id: "raw-id", displayPath: "src/ação, original.ts", segments: ["src", "ação, original.ts"], statuses: [{ kind: "staged", change: "modified" }, { kind: "unstaged", change: "deleted" }], marker: "MM" }];
  const before = summarizeDirectoryChanges(files);
  render(<ChangeTree files={files} label="raw-tree" selectedId="raw-id" onSelect={() => undefined} />);
  fireEvent.click(screen.getByRole("button", { name: "Expand src" }));
  const selected = screen.getByRole("button", { name: "Review src/ação, original.ts" });
  act(() => selected.focus());
  await act(async () => setLocaleChoice("pt-PT"));
  expect(screen.getByRole("button", { name: "Rever src/ação, original.ts" })).toBe(selected);
  expect(selected).toHaveAttribute("aria-pressed", "true");
  expect(selected).toHaveFocus();
  expect(screen.getByText("ação, original.ts")).toBeVisible();
  expect(summarizeDirectoryChanges(files)).toEqual(before);
  expect([...before.values()][0]).toEqual([{ fact: { kind: "staged", change: "modified" }, count: 1 }, { fact: { kind: "unstaged", change: "deleted" }, count: 1 }]);
});

it("pt-BR updates the mounted folder summary without deriving counts from translated punctuation", async () => {
  await setLocaleChoice("en-US");
  const user = userEvent.setup();
  const files: ChangeTreeFile[] = [
    { id: "one", displayPath: "src/one.ts", segments: ["src", "one.ts"], statuses: [{ kind: "staged", change: "modified" }], marker: "M" },
    { id: "two", displayPath: "src/two.ts", segments: ["src", "two.ts"], statuses: [{ kind: "staged", change: "modified" }, { kind: "unstaged", change: "deleted" }], marker: "MM" },
  ];
  render(<ChangeTree files={files} label="raw-tree" />);
  await user.tab();
  const tooltip = await screen.findByRole("tooltip");
  expect(tooltip).toHaveTextContent("Staged Modified: 2 · Unstaged Deleted: 1");
  await act(async () => setLocaleChoice("pt-BR"));
  expect(screen.getByRole("tooltip")).toBe(tooltip);
  expect(tooltip).toHaveTextContent("Modificado no índice: 2 · Excluído fora do índice: 1");
  expect(screen.getByRole("button", { name: "Expandir src" })).toHaveFocus();
});

it.each<[Locale, string, string, string]>([
  ["pt-BR", "0 arquivos", "1 arquivo", "2 arquivos"],
  ["pt-PT", "0 ficheiros", "1 ficheiro", "2 ficheiros"],
  ["it", "0 file", "1 file", "2 file"],
  ["es", "0 archivos", "1 archivo", "2 archivos"],
  ["en-US", "0 files", "1 file", "2 files"],
  ["en-GB", "0 files", "1 file", "2 files"],
])("%s presents zero, one and many file counts", async (locale, zero, one, many) => {
  await setLocaleChoice(locale);
  const props = { files: [], treeLabel: "raw-tree", listLabel: "raw-list", onSelect: () => undefined };
  const { rerender } = render(<FileExplorer {...props} count={0} />);
  expect(screen.getByText(zero)).toBeVisible();
  rerender(<FileExplorer {...props} count={1} />);
  expect(screen.getByText(one)).toBeVisible();
  rerender(<FileExplorer {...props} count={2} />);
  expect(screen.getByText(many)).toBeVisible();
});
