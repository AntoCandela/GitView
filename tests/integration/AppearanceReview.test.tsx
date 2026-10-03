/** Proves interface changes and explicit syntax overrides reach the real comparison surface. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { Workbench } from "../../src/app/workbench/Workbench";
import { AppearanceMenu } from "../../src/features/appearance";
import { IconThemeProvider } from "../../src/ui/file-icons/IconThemeProvider";
import { readyObservation, reviewClient, textReview } from "../support/review";

afterEach(cleanup);

test("a matched dark preset and then explicit light syntax keep the comparison readable independently of the interface", async () => {
  localStorage.setItem("gitview.app-theme", "cream");
  localStorage.setItem("gitview.icon-theme", "classic");
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "changes", theme: "match", lineMode: "scroll" }));
  const user = userEvent.setup();
  render(<IconThemeProvider><AppearanceMenu />
    <Workbench client={reviewClient(async () => textReview("const title = 'new';"))}
      entryId="one" selectionGeneration={0} contextLabel="Sample repository" observation={readyObservation()} />
  </IconThemeProvider>);
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: null })));
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  const comparison = await screen.findByRole("region", { name: "Read-only file comparison" });
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  const appearance = within(await screen.findByRole("dialog", { name: "Appearance" }));
  await user.click(appearance.getByRole("button", { name: "Midnight" }));
  expect(comparison).toHaveStyle({ "--code-bg": "#24292e", "--code-ink": "#e1e4e8" });
  expect(appearance.getByRole("img", { name: /Midnight interface, GitHub Dark syntax, Material icons/ })).toBeVisible();
  await user.click(appearance.getByText("Syntax"));
  await user.click(within(appearance.getByRole("radiogroup", { name: "Syntax palette" })).getByRole("radio", { name: "GitHub Light" }));
  expect(comparison).toHaveStyle({ "--code-bg": "#fff", "--code-ink": "#24292e", "--code-add-bg": "#e6ffec" });
  expect(appearance.getByRole("img", { name: /Midnight interface, GitHub Light syntax, Material icons/ })).toBeVisible();
});
