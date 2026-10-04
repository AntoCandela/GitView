/** Exercises real locale selection, pending resolution and session-only persistence through the menu. */
import "@testing-library/jest-dom/vitest";
import { afterEach, expect, test } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LanguageMenu } from "../../src/app/LanguageMenu";
import { createLocaleStore, installLocaleStoreForTests } from "../../src/i18n";
import { deferred } from "../support/deferred";

let restore: (() => void) | undefined;
afterEach(() => { cleanup(); restore?.(); restore = undefined; });

function fixture(writeFails = false) {
  const values = new Map<string, string>();
  const persistence = { writeFails };
  const store = createLocaleStore({
    storage: () => ({
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { if (persistence.writeFails) throw new Error("unavailable"); values.set(key, value); },
      removeItem: (key: string) => { if (persistence.writeFails) throw new Error("unavailable"); values.delete(key); },
    }),
    document,
  });
  restore = installLocaleStoreForTests(store);
  return { store, values, persistence };
}

test("language menu retains manual intent after a delayed System choice and restores focus", async () => {
  const { store, values } = fixture();
  const pending = deferred<readonly string[]>();
  await store.initialize(() => Promise.resolve(["en-GB"]));
  await store.setChoice("en-GB");
  await store.initialize(() => pending.promise);
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  await user.click(screen.getByRole("radio", { name: "System" }));
  expect(screen.getByRole("status")).toHaveTextContent("Resolving system language");
  expect(screen.getByRole("radio", { name: "English (United Kingdom)" })).toBeChecked();
  await user.click(screen.getByRole("radio", { name: "Português (Portugal)" }));
  await act(async () => pending.resolve(["it-IT"]));
  expect(document.documentElement.lang).toBe("pt-PT");
  expect(JSON.parse(values.get("gitview.locale")!)).toEqual({ version: 1, choice: "pt-PT" });
  await user.keyboard("{Escape}");
  expect(screen.getByRole("button", { name: "Idioma" })).toHaveFocus();
});

test("language choice remains usable and explicitly session-only when storage rejects it", async () => {
  const { store } = fixture(true);
  await store.initialize(async () => ["en-US"]);
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  await user.click(screen.getByRole("radio", { name: "Italiano" }));
  expect(document.documentElement.lang).toBe("it");
  expect(screen.getByRole("radio", { name: "Italiano" })).toBeChecked();
  expect(screen.getByRole("alert")).toHaveTextContent("sessione");
});

test.each(["pointer", "keyboard"] as const)("%s activation of the current manual language cancels pending System resolution", async (input) => {
  const { store, values } = fixture();
  const pending = deferred<readonly string[]>();
  await store.setChoice("en-GB");
  await store.initialize(() => pending.promise);
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  await user.click(screen.getByRole("radio", { name: "System" }));
  const manual = screen.getByRole("radio", { name: "English (United Kingdom)" });
  if (input === "pointer") await user.click(manual);
  else {
    act(() => manual.focus());
    await user.keyboard(" ");
  }
  await act(async () => pending.resolve(["it-IT"]));
  expect(document.documentElement.lang).toBe("en-GB");
  expect(JSON.parse(values.get("gitview.locale")!)).toEqual({ version: 1, choice: "en-GB" });
  expect(manual).toBeChecked();
});

test.each(["pointer", "keyboard"] as const)("%s reactivation of checked System resolves current native preferences again", async (input) => {
  const { store, values } = fixture();
  const pending = deferred<readonly string[]>();
  let languages = Promise.resolve<readonly string[]>(["en-US"]);
  await store.initialize(() => languages);
  languages = pending.promise;
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  const system = screen.getByRole("radio", { name: "System" });
  expect(system).toBeChecked();
  if (input === "pointer") await user.click(system);
  else {
    act(() => system.focus());
    await user.keyboard(" ");
  }
  expect(screen.getByRole("status")).toHaveTextContent("Resolving system language");
  expect(document.documentElement.lang).toBe("en-US");
  await act(async () => pending.resolve(["it-IT"]));
  expect(document.documentElement.lang).toBe("it");
  expect(system).toBeChecked();
  expect(values.has("gitview.locale")).toBe(false);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});

test("reactivating a session-only manual choice retries saving without changing the language", async () => {
  const { store, values, persistence } = fixture(true);
  await store.initialize(async () => ["en-US"]);
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  const italian = screen.getByRole("radio", { name: "Italiano" });
  await user.click(italian);
  expect(screen.getByRole("alert")).toBeVisible();
  persistence.writeFails = false;
  await user.click(italian);
  expect(document.documentElement.lang).toBe("it");
  expect(italian).toBeChecked();
  expect(JSON.parse(values.get("gitview.locale")!)).toEqual({ version: 1, choice: "it" });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("reactivating session-only System retries removing the saved override", async () => {
  const { store, values, persistence } = fixture();
  await store.setChoice("en-GB");
  await store.initialize(async () => ["en-US"]);
  persistence.writeFails = true;
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.click(screen.getByRole("button", { name: "Language" }));
  const system = screen.getByRole("radio", { name: "System" });
  await user.click(system);
  expect(system).toBeChecked();
  expect(screen.getByRole("alert")).toBeVisible();
  expect(JSON.parse(values.get("gitview.locale")!)).toEqual({ version: 1, choice: "en-GB" });
  persistence.writeFails = false;
  await user.click(system);
  expect(document.documentElement.lang).toBe("en-US");
  expect(values.has("gitview.locale")).toBe(false);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

test("keyboard navigation applies locale choices and dismisses back to the translated trigger", async () => {
  const { store } = fixture();
  await store.setChoice("en-GB");
  await store.initialize(async () => ["es-MX"]);
  const user = userEvent.setup();
  render(<LanguageMenu />);
  await user.tab();
  await user.keyboard(" ");
  expect(screen.getByRole("radio", { name: "English (United Kingdom)" })).toHaveFocus();
  await user.keyboard("{ArrowUp}");
  expect(screen.getByRole("radio", { name: "English (United States)" })).toHaveFocus();
  expect(document.documentElement.lang).toBe("en-US");
  await user.keyboard("{Home}");
  expect(screen.getByRole("radio", { name: "Sistema" })).toHaveFocus();
  expect(document.documentElement.lang).toBe("es");
  await user.keyboard("{Escape}");
  expect(screen.getByRole("button", { name: "Idioma" })).toHaveFocus();
});
