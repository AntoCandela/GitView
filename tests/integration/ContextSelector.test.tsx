/** Protects live picker localization without resetting search, focus or repository navigation identity. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { ContextSelector } from "../../src/features/history/ContextSelector";
import { createLocaleStore, installLocaleStoreForTests, setLocaleChoice, translate, useTranslation, type Locale } from "../../src/i18n";
import { reviewClient } from "../support/review";

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

const locales: Locale[] = ["en-US", "en-GB", "it", "es", "pt-BR", "pt-PT"];

function pickerClient() {
  const client = reviewClient();
  client.listContexts = vi.fn(async () => ({
    kind: "options" as const,
    branches: [{ name: "topic/linked" }, { name: "topic/viewed" }, { name: "main" }],
    worktrees: [
      { id: "linked-id", label: "Árvore original", branch: "topic/linked", current: false },
      { id: "current-id", label: "Project", branch: "main", current: true },
    ],
  }));
  return client;
}

function Picker(props: Omit<Parameters<typeof ContextSelector>[0], "description">) {
  const { t } = useTranslation();
  return <ContextSelector {...props} description={t("history.context.description")} />;
}

test.each(locales)("%s preserves open search, selected branch, focused tooltip and worktree identity on locale change", async (locale) => {
  await setLocaleChoice(locale === "en-US" ? "it" : "en-US");
  const user = userEvent.setup();
  const client = pickerClient();
  const onBranch = vi.fn();
  const onWorktree = vi.fn();
  render(<Picker client={client} entryId="one" branch="topic/viewed" refColors={new Map()}
    onBranch={onBranch} onWorktree={onWorktree} />);
  const trigger = screen.getByRole("button");
  await user.click(trigger);
  const group = await screen.findByRole("group");
  expect(within(group).getAllByRole("button").map((item) => item.textContent))
    .toEqual(["main", "topic/viewed", "topic/linked"]);
  const search = screen.getByRole("searchbox");
  await user.type(search, "topic/");
  const [selected, destination] = within(group).getAllByRole("button");
  await user.keyboard("{ArrowDown}{ArrowDown}");
  expect(destination).toHaveFocus();
  const tooltip = await screen.findByRole("tooltip");

  await act(async () => setLocaleChoice(locale));

  expect(screen.getByRole("searchbox", { name: translate(locale, "history.context.search") })).toBe(search);
  expect(search).toHaveValue("topic/");
  expect(screen.getByRole("dialog", { name: translate(locale, "history.context.choose") })).toBeVisible();
  expect(screen.getByRole("button", { name: translate(locale, "history.context.viewBranch", { branch: "topic/viewed" }) })).toBe(selected);
  expect(selected).toHaveAttribute("aria-pressed", "true");
  expect(screen.getByRole("button", { name: translate(locale, "history.context.openBranchWorktree", { label: "Árvore original", branch: "topic/linked" }) })).toBe(destination);
  expect(destination).toHaveFocus();
  expect(destination).toHaveTextContent("topic/linked");
  expect(screen.getByRole("tooltip")).toBe(tooltip);
  expect(tooltip).toHaveTextContent(translate(locale, "history.context.hint.checkedOutElsewhere"));
  expect(tooltip).toHaveTextContent(translate(locale, "history.context.hint.navigateDescription"));
  expect(tooltip).toHaveTextContent("Árvore original");
  expect(client.listContexts).toHaveBeenCalledTimes(1);
  expect(onWorktree).not.toHaveBeenCalled();
  await user.keyboard("{Enter}");
  expect(onWorktree).toHaveBeenCalledExactlyOnceWith("linked-id");
  expect(onBranch).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

test("a locale switch preserves view-only branch navigation when worktree navigation is unavailable", async () => {
  const user = userEvent.setup();
  const client = pickerClient();
  const onBranch = vi.fn();
  render(<Picker client={client} entryId="one" branch="topic/viewed" refColors={new Map()} onBranch={onBranch} />);
  const trigger = screen.getByRole("button");
  await user.click(trigger);
  const destination = await screen.findByRole("button", { name: "Open worktree Árvore original for branch topic/linked" });
  expect(destination).toBeDisabled();
  await user.keyboard("{ArrowDown}{ArrowDown}");
  const selected = screen.getByRole("button", { name: "View branch topic/viewed" });
  expect(selected).toHaveFocus();
  await act(async () => setLocaleChoice("pt-PT"));
  expect(selected).toHaveFocus();
  expect(destination).toBeDisabled();
  await user.keyboard("{Enter}");
  expect(onBranch).toHaveBeenCalledExactlyOnceWith("topic/viewed");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

test.each(locales)("%s retranslates an already-visible picker failure without reloading or exposing native prose", async (locale) => {
  await setLocaleChoice(locale === "en-US" ? "it" : "en-US");
  const user = userEvent.setup();
  const client = reviewClient();
  client.listContexts = vi.fn(async () => ({ kind: "error" as const, code: "inaccessible" as const, message: "Private native path /private/repository" }));
  render(<Picker client={client} entryId="one" branch="main" refColors={new Map()} onBranch={vi.fn()} />);
  await user.click(screen.getByRole("button"));
  const alert = await screen.findByRole("alert");
  const search = screen.getByRole("searchbox");
  await user.type(search, "original");

  await act(async () => setLocaleChoice(locale));

  expect(screen.getByRole("alert")).toBe(alert);
  expect(alert).toHaveTextContent(translate(locale, "history.error.inaccessible"));
  expect(alert).not.toHaveTextContent("/private/repository");
  expect(search).toHaveValue("original");
  expect(search).toHaveFocus();
  expect(client.listContexts).toHaveBeenCalledTimes(1);
});
