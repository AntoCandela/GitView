/** Exercises opt-in intent, durable-save warnings and native reachability without workspace ownership. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { CompanionSettings } from "../../src/app/CompanionSettings";
import { CompanionPresentationPublisher } from "../../src/app/CompanionPresentation";
import type { CompanionState, PresentationInput } from "../../src/contracts/companion";
import { createLocaleStore, installLocaleStoreForTests } from "../../src/i18n";
import { IconThemeProvider } from "../../src/ui/file-icons/IconThemeProvider";
import { deferred } from "../support/deferred";

let restoreLocale: () => void;
beforeEach(async () => {
  const store = createLocaleStore({ storage: () => localStorage });
  restoreLocale = installLocaleStoreForTests(store);
  await store.setChoice("en-US");
});
afterEach(() => { cleanup(); restoreLocale(); });

function state(overrides: Partial<CompanionState> = {}): CompanionState {
  return { supported: true, enabled: false, available: false, visible: false, revision: 1,
    persistenceError: null, nativeError: null, ...overrides };
}
function client(outcome: Partial<CompanionState> = {}) {
  let current = state();
  return { state: async () => current, async setEnabled(enabled: boolean) {
    current = state({ enabled, available: enabled, revision: current.revision + 1, ...outcome });
    return { kind: current.nativeError ? "unavailable" as const : "applied" as const, state: current };
  } };
}
async function openSettings() {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Menu-bar companion" }));
  return user;
}

test("keyboard opt-in is discoverable and preserves the current reading context when enabled and disabled", async () => {
  function Workspace() {
    const [selected, select] = useState("Repository A / src/example.ts / staged");
    return <><button onClick={() => select("Repository B / src/other.ts / unstaged")}>{selected}</button><CompanionSettings client={client()} /></>;
  }
  render(<Workspace />);
  const user = await openSettings();
  const toggle = screen.getByRole("checkbox", { name: "Keep GitView in the menu bar" });
  toggle.focus();
  await user.keyboard(" ");
  expect(toggle).toBeChecked();
  expect(await screen.findByText("Available in the menu bar")).toBeVisible();
  expect(screen.getByRole("button", { name: "Repository A / src/example.ts / staged" })).toBeVisible();
  await user.keyboard(" ");
  expect(toggle).not.toBeChecked();
  expect(screen.getByRole("button", { name: "Repository A / src/example.ts / staged" })).toBeVisible();
});

test("failed saves retain effective availability but clearly report a session-only preference", async () => {
  render(<CompanionSettings client={client({ persistenceError: "save_failed" })} />);
  const user = await openSettings();
  await user.click(screen.getByRole("checkbox", { name: "Keep GitView in the menu bar" }));
  expect(await screen.findByText("Available in the menu bar")).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent("session only");
  expect(screen.queryByText("The menu-bar companion is unavailable. GitView remains open.")).not.toBeInTheDocument();
});

test("native activation failure never claims availability or durable-save failure", async () => {
  render(<CompanionSettings client={client({ nativeError: "tray_failed", available: false })} />);
  const user = await openSettings();
  await user.click(screen.getByRole("checkbox", { name: "Keep GitView in the menu bar" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("unavailable");
  expect(screen.queryByText("Available in the menu bar")).not.toBeInTheDocument();
  expect(screen.getByRole("checkbox")).toBeChecked();
  expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
});

test("protected preference bytes are explained separately from native availability", async () => {
  render(<CompanionSettings client={{ ...client(), state: async () => state({ persistenceError: "unsupported_version" }) }} />);
  await openSettings();
  expect(screen.getByRole("alert")).toHaveTextContent("saved preference could not be read");
  expect(screen.getByRole("alert")).toHaveTextContent("not be overwritten");
});

test("non-macOS surfaces offer no companion lifecycle control", async () => {
  await act(async () => {
    render(<CompanionSettings client={{ ...client(), state: async () => state({ supported: false }) }} />);
  });
  expect(screen.queryByRole("button", { name: "Menu-bar companion" })).not.toBeInTheDocument();
});

test("transport failures remain recoverable without claiming a successful toggle", async () => {
  render(<CompanionSettings client={{ ...client(), async setEnabled() { throw new Error("private path must not render"); } }} />);
  const user = await openSettings();
  await user.click(screen.getByRole("checkbox", { name: "Keep GitView in the menu bar" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("could not be updated");
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  expect(document.body).not.toHaveTextContent("private path");
});

test("retry preserves a failed disable intent while the native main window is unavailable", async () => {
  const requests: boolean[] = [];
  const settings = {
    state: async () => state({ enabled: true, available: true }),
    async setEnabled(enabled: boolean) {
      requests.push(enabled);
      return { kind: "unavailable" as const, state: state({ enabled: true, available: true, nativeError: "main_unavailable" }) };
    },
  };
  render(<CompanionSettings client={settings} />);
  const user = await openSettings();
  await user.click(screen.getByRole("checkbox", { name: "Keep GitView in the menu bar" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("main window could not be shown");
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(requests).toEqual([false, false]);
  expect(screen.getByRole("checkbox")).toBeChecked();
});

test("restored opt-in becomes available after publication completes without another focus or open action", async () => {
  let current = state({ enabled: true, available: false });
  const activation = deferred<void>();
  const settings = {
    ...client(),
    state: async () => current,
    async publishPresentation(input: PresentationInput) {
      await activation.promise;
      current = state({ enabled: true, available: true, revision: 2 });
      return { ...input, revision: 1 };
    },
  };
  render(<IconThemeProvider><CompanionPresentationPublisher client={settings} /><CompanionSettings client={settings} /></IconThemeProvider>);
  await openSettings();
  expect(await screen.findByRole("alert")).toHaveTextContent("unavailable");
  await act(async () => activation.resolve());
  expect(await screen.findByText("Available in the menu bar")).toBeVisible();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("checkbox")).toBeChecked();
});

test("a controlled native invalidation refreshes effective state without a second surface subscription", async () => {
  let current = state({ enabled: true, available: true });
  const settings = { ...client(), state: async () => current };
  const view = render(<CompanionSettings client={settings} refreshRevision={1} />);
  await openSettings();
  expect(await screen.findByText("Available in the menu bar")).toBeVisible();
  current = state({ enabled: true, available: false, nativeError: "panel_failed", revision: 2 });
  view.rerender(<CompanionSettings client={settings} refreshRevision={2} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("unavailable");
  expect(screen.queryByText("Available in the menu bar")).not.toBeInTheDocument();
});
