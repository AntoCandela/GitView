/** Exercises persisted and session-only presentation choices across review mounts. */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ReviewControls } from "../../src/features/diff/ReviewControls";
import { useReviewChoices } from "../../src/features/appearance";

function Consumer({ label }: { label: string }) {
  const choices = useReviewChoices();
  return <section aria-label={label}>
    <ReviewControls choices={choices} />
    <button onClick={() => choices.change({ theme: "catppuccin-latte" })}>Use Latte</button>
    <output aria-label="Current presentation" data-session-only={choices.persistenceError}>
      {choices.mode} / {choices.theme} / {choices.lineMode}
    </output>
  </section>;
}
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

test("saved review mode, syntax and line view restore together across mounts", () => {
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "full", theme: "solarized-light", lineMode: "wrap" }));
  const first = render(<Consumer label="First file" />);
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: "gitview.code-review" })));
  expect(screen.getByRole("radio", { name: "Full file" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Wrap" })).toBeChecked();
  expect(screen.getByLabelText("Current presentation")).toHaveTextContent("full / solarized-light / wrap");

  first.unmount();
  render(<Consumer label="Second file" />);
  expect(screen.getByRole("radio", { name: "Full file" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Wrap" })).toBeChecked();
  expect(screen.getByLabelText("Current presentation")).toHaveTextContent("full / solarized-light / wrap");
});

test("partial preference updates reach every consumer and survive complete unmount without storage", () => {
  localStorage.setItem("gitview.code-review", JSON.stringify({ mode: "changes", theme: "github-light", lineMode: "scroll" }));
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage denied"); });
  const { unmount } = render(<><Consumer label="Live review" /><Consumer label="Historical review" /></>);
  act(() => window.dispatchEvent(new StorageEvent("storage", { key: "gitview.code-review" })));
  const live = within(screen.getByRole("region", { name: "Live review" }));
  expect(live.getByRole("radio", { name: "Changes" })).toBeChecked();
  expect(live.getByRole("radio", { name: "Scroll" })).toBeChecked();
  fireEvent.click(live.getByRole("radio", { name: "Full file" }));
  fireEvent.click(live.getByRole("radio", { name: "Wrap" }));
  fireEvent.click(live.getByRole("button", { name: "Use Latte" }));
  const history = within(screen.getByRole("region", { name: "Historical review" }));
  expect(history.getByRole("radio", { name: "Full file" })).toBeChecked();
  expect(history.getByRole("radio", { name: "Wrap" })).toBeChecked();
  expect(history.getByLabelText("Current presentation")).toHaveTextContent("full / catppuccin-latte / wrap");
  expect(history.getByLabelText("Current presentation")).toHaveAttribute("data-session-only", "true");
  unmount();
  render(<Consumer label="Next selected file" />);
  expect(screen.getByRole("radio", { name: "Full file" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Wrap" })).toBeChecked();
  expect(screen.getByLabelText("Current presentation")).toHaveTextContent("full / catppuccin-latte / wrap");
});
