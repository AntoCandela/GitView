/** Guards radio keyboard selection, controlled state and layout-disclosure focus boundaries. */

import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RadioGroup } from "../../src/ui/RadioGroup";
import { WorkbenchLayoutMenu } from "../../src/app/workbench/WorkbenchLayoutMenu";
import { defaultWorkbenchLayout, type WorkbenchLayoutId } from "../../src/app/workbench/workbenchLayout";

afterEach(cleanup);

const choices = [
  { value: "first", label: "First" },
  { value: "unavailable", label: "Unavailable", disabled: true },
  { value: "last", label: "Last" },
] as const;

function ControlledChoices({ label }: { label: string }) {
  const [value, setValue] = useState<string>("first");
  return <RadioGroup label={label} value={value} onChange={setValue} options={choices} />;
}

test("arrows skip disabled choices, wrap, and Home, End and Space select an enabled choice", async () => {
  const user = userEvent.setup();
  render(<ControlledChoices label="Choices" />);
  const first = screen.getByRole("radio", { name: "First" });
  const last = screen.getByRole("radio", { name: "Last" });
  first.focus();
  await user.keyboard("{ArrowRight}");
  expect(last).toHaveFocus();
  expect(last).toBeChecked();
  expect(screen.getByRole("radio", { name: "Unavailable" })).toBeDisabled();
  await user.keyboard("{ArrowDown}");
  expect(first).toHaveFocus();
  expect(first).toBeChecked();
  await user.keyboard("{ArrowLeft}");
  expect(last).toHaveFocus();
  await user.keyboard("{Home}");
  expect(first).toHaveFocus();
  expect(first).toBeChecked();
  await user.keyboard("{End}");
  expect(last).toHaveFocus();
  expect(last).toBeChecked();
  first.focus();
  await user.keyboard(" ");
  expect(first).toBeChecked();
  expect(last).not.toBeChecked();
});

test("selection stays controlled and independently mounted groups do not uncheck each other", async () => {
  const user = userEvent.setup();
  const onChange = vi.fn();
  const { rerender } = render(
    <RadioGroup label="Controlled choices" value="first" options={choices} onChange={onChange} />,
  );
  await user.click(screen.getByRole("radio", { name: "Last" }));
  expect(onChange).toHaveBeenCalledWith("last");
  expect(screen.getByRole("radio", { name: "First" })).toBeChecked();
  expect(screen.getByRole("radio", { name: "Last" })).not.toBeChecked();
  rerender(
    <>
      <ControlledChoices label="One" />
      <ControlledChoices label="Two" />
    </>,
  );
  const one = within(screen.getByRole("radiogroup", { name: "One" }));
  const two = within(screen.getByRole("radiogroup", { name: "Two" }));
  await user.click(one.getByRole("radio", { name: "Last" }));
  expect(one.getByRole("radio", { name: "Last" })).toBeChecked();
  expect(two.getByRole("radio", { name: "First" })).toBeChecked();
});

test("layout radios keep the chooser open, reopen on the selected choice, and dismiss with Escape or outside click", async () => {
  const user = userEvent.setup();
  function LayoutChoices() {
    const [value, setValue] = useState<WorkbenchLayoutId>(defaultWorkbenchLayout);
    return (
      <>
        <WorkbenchLayoutMenu value={value} onChange={setValue} />
        <button type="button">Outside</button>
      </>
    );
  }
  render(<LayoutChoices />);
  const trigger = screen.getByRole("button", { name: "Workbench layout" });
  await user.click(trigger);
  const group = await screen.findByRole("radiogroup", { name: "Panel arrangement" });
  const first = within(group).getByRole("radio", { name: "Preview above, Files bottom left, Graph bottom right" });
  await waitFor(() => expect(first).toHaveFocus());
  const last = within(group).getByRole("radio", { name: "Graph above, Preview bottom left, Files bottom right" });
  await user.keyboard("{End}");
  expect(last).toHaveFocus();
  expect(last).toBeChecked();
  expect(first).not.toBeChecked();
  expect(group).toBeInTheDocument();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("radiogroup")).toBeNull());
  await waitFor(() => expect(trigger).toHaveFocus());
  await user.click(trigger);
  const selected = await screen.findByRole("radio", { name: "Graph above, Preview bottom left, Files bottom right" });
  await waitFor(() => expect(selected).toHaveFocus());
  expect(selected).toBeChecked();
  await user.click(screen.getByRole("button", { name: "Outside" }));
  await waitFor(() => expect(screen.queryByRole("radiogroup")).toBeNull());
});
