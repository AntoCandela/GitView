/** Guards menu keyboard handling and portaled event isolation from selectable rows. */

import "@testing-library/jest-dom/vitest";
import { afterEach, beforeAll, expect, test, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { KebabMenu } from "../../src/ui/KebabMenu";

afterEach(cleanup);
beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn();
});

test.each(["{Enter}", " "])(
  "menu navigation and activation with %s do not activate the enclosing row",
  async (activationKey) => {
    const user = userEvent.setup();
    const rowInteraction = vi.fn();
    const selected: string[] = [];
    render(
      <div
        onClick={rowInteraction}
        onPointerDown={rowInteraction}
        onPointerUp={rowInteraction}
        onKeyDown={rowInteraction}
        onKeyUp={rowInteraction}
      >
        <KebabMenu
          label="Repository actions"
          items={[
            { label: "Rename", onSelect: () => selected.push("rename") },
            {
              label: "Unavailable action",
              disabled: true,
              onSelect: () => selected.push("disabled"),
            },
            {
              label: "Remove",
              destructive: true,
              onSelect: () => selected.push("remove"),
            },
          ]}
        />
      </div>,
    );
    const trigger = screen.getByRole("button", { name: "Repository actions" });
    trigger.focus();
    await user.keyboard("{ArrowDown}");
    const rename = await screen.findByRole("menuitem", { name: "Rename" });
    const remove = screen.getByRole("menuitem", { name: "Remove" });
    const unavailable = screen.getByRole("menuitem", {
      name: "Unavailable action",
    });
    await waitFor(() => expect(rename).toHaveFocus());
    await user.keyboard("{End}");
    expect(remove).toHaveFocus();
    await user.keyboard("{Home}{ArrowDown}");
    expect(unavailable).toHaveFocus();
    expect(unavailable).toHaveAttribute("aria-disabled", "true");
    await user.keyboard(activationKey);
    expect(selected).toEqual([]);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{ArrowDown}");
    expect(remove).toHaveFocus();
    await user.keyboard(activationKey);
    expect(selected).toEqual(["remove"]);
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(rowInteraction).not.toHaveBeenCalled();
  },
);

test("pointer actions stay within the menu, and Escape or an outside click dismisses it", async () => {
  const user = userEvent.setup();
  const rowInteraction = vi.fn();
  const rename = vi.fn();
  render(
    <>
      <div onClick={rowInteraction} onPointerDown={rowInteraction}>
        <KebabMenu
          label="Repository actions"
          items={[{ label: "Rename", onSelect: rename }]}
        />
      </div>
      <button type="button">Outside</button>
    </>,
  );
  const trigger = screen.getByRole("button", { name: "Repository actions" });
  await user.click(trigger);
  await user.click(await screen.findByRole("menuitem", { name: "Rename" }));
  expect(rename).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  expect(rowInteraction).not.toHaveBeenCalled();

  await user.click(trigger);
  await screen.findByRole("menu");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  await waitFor(() => expect(trigger).toHaveFocus());

  await user.click(trigger);
  await screen.findByRole("menu");
  await user.click(screen.getByRole("button", { name: "Outside" }));
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  expect(rowInteraction).not.toHaveBeenCalled();
  expect(rename).toHaveBeenCalledTimes(1);
});
