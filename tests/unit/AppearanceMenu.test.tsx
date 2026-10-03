/** Exercises preset composition, explicit overrides and session-only appearance continuity. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { AppearanceMenu } from "../../src/features/appearance";
import { IconThemeProvider } from "../../src/ui/file-icons/IconThemeProvider";

function mount() {
  localStorage.setItem("gitview.app-theme", "cream");
  localStorage.setItem("gitview.icon-theme", "classic");
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "changes", theme: "match", lineMode: "scroll" }));
  const view = render(<IconThemeProvider><AppearanceMenu /></IconThemeProvider>);
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: null })));
  return view;
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("a dark preset coordinates both palettes and icon artwork while an explicit syntax override survives interface changes", async () => {
  const user = userEvent.setup();
  mount();
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  await user.click(screen.getByRole("button", { name: "Midnight" }));
  expect(localStorage.getItem("gitview.app-theme")).toBe("midnight");
  expect(localStorage.getItem("gitview.icon-theme")).toBe("material");
  expect(JSON.parse(localStorage.getItem("gitview.code-review")!).theme).toBe("match");
  expect(screen.getByRole("img", { name: /Midnight interface, GitHub Dark syntax, Material icons/ })).toBeVisible();
  expect(screen.getByText("Syntax").closest("details")).not.toHaveAttribute("open");
  await user.click(screen.getByText("Syntax"));
  expect(screen.getByText("Syntax").closest("details")).toHaveAttribute("open");
  await user.click(within(screen.getByRole("radiogroup", { name: "Syntax palette" })).getByRole("radio", { name: "GitHub Light" }));
  await user.click(screen.getByText("Interface"));
  await user.click(within(screen.getByRole("radiogroup", { name: "Interface palette" })).getByRole("radio", { name: "Mist" }));
  expect(screen.getByRole("img", { name: /Mist interface, GitHub Light syntax, Material icons/ })).toBeVisible();
  expect(screen.getByText("Custom")).toBeVisible();
  expect(JSON.parse(localStorage.getItem("gitview.code-review")!).theme).toBe("github-light");
  expect(localStorage.getItem("gitview.icon-theme")).toBe("material");
});

test("a rejected interface save stays visible through unmount and reports session-only state", async () => {
  const user = userEvent.setup();
  mount();
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage denied"); });
  await user.click(screen.getByText("Interface"));
  await user.click(within(screen.getByRole("radiogroup", { name: "Interface palette" })).getByRole("radio", { name: "Graphite" }));
  expect(screen.getByRole("img", { name: /Graphite interface, GitHub Dark syntax/ })).toBeVisible();
  expect(screen.getByRole("status")).toHaveTextContent("session");
  cleanup();
  render(<IconThemeProvider><AppearanceMenu /></IconThemeProvider>);
  await user.click(screen.getByRole("button", { name: "Appearance" }));
  expect(screen.getByRole("img", { name: /Graphite interface, GitHub Dark syntax/ })).toBeVisible();
});
