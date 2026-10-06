/** Exercises the authoritative preference bridge and storage-free compact presentation. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { CompanionPresentationProvider, CompanionPresentationPublisher } from "../../src/app/CompanionPresentation";
import type { PresentationInput, PresentationSnapshot } from "../../src/contracts/companion";
import { useAppearanceTheme, useReviewChoices } from "../../src/features/appearance";
import { createLocaleStore, installLocaleStoreForTests, locales, setLocaleChoice, translate, useLocale } from "../../src/i18n";
import { catalogs, createTranslator } from "../../src/i18n/messages";
import { IconThemeProvider, useIconTheme } from "../../src/ui/file-icons/IconThemeProvider";
import { TreeEntryIcon } from "../../src/ui/file-icons/TreeEntryIcon";
import { fileIcon } from "../../src/ui/file-icons/fileIconLookup";
import { deferred } from "../support/deferred";

let restoreLocale: (() => void) | undefined;
afterEach(() => { cleanup(); restoreLocale?.(); restoreLocale = undefined; vi.restoreAllMocks(); });

function isolatedLocale() {
  localStorage.removeItem("gitview.locale");
  const store = createLocaleStore({ storage: () => localStorage });
  restoreLocale = installLocaleStoreForTests(store);
  return store;
}
function presentation(overrides: Partial<PresentationSnapshot> = {}): PresentationSnapshot {
  return { revision: 1, locale: "it", appearanceTheme: "graphite", iconTheme: "material",
    review: { mode: "full", theme: "github-dark", lineMode: "wrap" }, persistenceError: false,
    menuLabels: { openGitView: "Apri GitView", quit: "Esci" }, ...overrides };
}
function Reading() {
  const locale = useLocale();
  const appearance = useAppearanceTheme();
  const review = useReviewChoices();
  const icons = useIconTheme();
  const [selection, select] = useState("src/first.ts:staged");
  return <><button onClick={() => select("src/second.ts:unstaged")}>{selection}</button>
    <output>{`${locale.locale}:${appearance.theme}:${icons?.theme}:${review.mode}:${review.theme}:${review.lineMode}`}</output>
    <TreeEntryIcon name="sample.ts" kind="file" /></>;
}
function Choices() {
  const appearance = useAppearanceTheme();
  const review = useReviewChoices();
  const icons = useIconTheme();
  return <><button onClick={() => {
    appearance.setTheme("midnight");
    review.change({ mode: "full", theme: "github-dark", lineMode: "wrap" });
    icons?.setTheme("material");
    void setLocaleChoice("pt-PT");
  }}>Change presentation</button><button onClick={() => icons?.setTheme("catppuccin")}>Catppuccin</button></>;
}

test("publishes nothing before locale initialization resolves", async () => {
  const store = isolatedLocale();
  const languages = deferred<readonly string[]>();
  const publishPresentation = vi.fn(async (input: PresentationInput) => ({ ...input, revision: 1 }));
  render(<IconThemeProvider><CompanionPresentationPublisher client={{ publishPresentation }} /></IconThemeProvider>);
  let initialization: Promise<void>;
  act(() => { initialization = store.initialize(() => languages.promise); });
  expect(publishPresentation).not.toHaveBeenCalled();
  await act(async () => { languages.resolve(["it"]); await initialization!; });
  await waitFor(() => expect(publishPresentation).toHaveBeenCalledWith(expect.objectContaining({ locale: "it", menuLabels: { openGitView: "Apri GitView", quit: "Esci" } })));
});

test("failed saves still publish authoritative session locale, palettes, review and both icon themes while hidden", async () => {
  const store = isolatedLocale();
  await store.initialize(async () => ["en-US"]);
  const published: PresentationInput[] = [];
  const client = { async publishPresentation(input: PresentationInput) { published.push(input); return { ...input, revision: published.length }; } };
  render(<div hidden><IconThemeProvider><CompanionPresentationPublisher client={client} /><Choices /></IconThemeProvider></div>);
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("denied"); });
  act(() => screen.getByText("Change presentation").click());
  await waitFor(() => expect(published.at(-1)).toMatchObject({ locale: "pt-PT", appearanceTheme: "midnight", iconTheme: "material", review: { mode: "full", theme: "github-dark", lineMode: "wrap" }, persistenceError: true, menuLabels: { openGitView: "Abrir GitView", quit: "Sair" } }));
  act(() => screen.getByText("Catppuccin").click());
  await waitFor(() => expect(published.at(-1)?.iconTheme).toBe("catppuccin"));
});

test("a missing native presentation exposes no guessed language, content, icons or storage authority", () => {
  const storage = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("must not read"); });
  render(<CompanionPresentationProvider presentation={null}><Reading /></CompanionPresentationProvider>);
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
  expect(document.querySelector('[aria-busy="true"]')).toBeInTheDocument();
  expect(document.body).not.toHaveTextContent("Checking");
  expect(storage).not.toHaveBeenCalled();
});

test.each(locales)("%s consumes native presentation without persistence and preserves file/category identity", async (locale) => {
  const user = userEvent.setup();
  const storage = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("must not read"); });
  const save = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("must not save"); });
  const view = render(<CompanionPresentationProvider presentation={presentation()}><Reading /></CompanionPresentationProvider>);
  const selection = screen.getByRole("button");
  await user.click(selection);
  view.rerender(<CompanionPresentationProvider presentation={presentation({ revision: 2, locale, iconTheme: "catppuccin", appearanceTheme: "midnight", persistenceError: true })}><Reading /></CompanionPresentationProvider>);
  expect(screen.getByRole("button", { name: "src/second.ts:unstaged" })).toBe(selection);
  expect(selection).toHaveFocus();
  expect(screen.getByRole("status")).toHaveTextContent(`${locale}:midnight:catppuccin:full:github-dark:wrap`);
  expect(document.documentElement.lang).toBe(locale);
  expect(document.documentElement.dataset.appearance).toBe("midnight");
  expect(document.querySelector(".tree-entry-icon")).toHaveAttribute("src", fileIcon("catppuccin", "sample.ts"));
  expect(storage).not.toHaveBeenCalled();
  expect(save).not.toHaveBeenCalled();
  expect(translate(locale, "companion.openGitView")).not.toBe("");
});

test.each([
  ["pt-BR", "Abrir GitView", "Sair"],
  ["pt-PT", "Abrir GitView", "Sair"],
  ["it", "Apri GitView", "Esci"],
  ["es", "Abrir GitView", "Salir"],
  ["en-US", "Open GitView", "Quit"],
  ["en-GB", "Open GitView", "Quit"],
] as const)("%s publishes only the fixed translated native menu vocabulary", async (locale, openGitView, quit) => {
  const store = isolatedLocale();
  await store.setChoice(locale);
  const publishPresentation = vi.fn(async (input: PresentationInput) => ({ ...input, revision: 1 }));
  render(<IconThemeProvider><CompanionPresentationPublisher client={{ publishPresentation }} /></IconThemeProvider>);
  await waitFor(() => expect(publishPresentation).toHaveBeenCalledWith(expect.objectContaining({ locale, menuLabels: { openGitView, quit } })));
});

test("a missing companion translation is a defect rather than an English fallback", () => {
  const missingTranslation = createTranslator({ "en-US": catalogs["en-US"], it: {} });
  expect(() => missingTranslation("it", "companion.openGitView")).toThrow();
});
