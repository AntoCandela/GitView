/** Exercises tree selection, native outcomes and live revisions through the read-only review surface. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { ReviewResult } from "../../src/contracts/diff";
import type { RepositoryClient } from "../../src/contracts/repositories";
import { Workbench } from "../../src/app/workbench/Workbench";
import { useObservation } from "../../src/features/changes";
import { FileReview } from "../../src/features/diff";
import { deferred } from "../support/deferred";
import { changedFile, readingSelection, readyObservation, reviewClient, reviewIdentity, textReview } from "../support/review";

afterEach(() => { cleanup(); vi.useRealTimers(); });

test("staged and unstaged choices show their live endpoints, escaped source, line kinds and newline markers", async () => {
  const read: RepositoryClient["reviewFile"] = async (entryId, _revision, pathId, category) => ({
    ...reviewIdentity, entryId, pathId, category, from: category === "staged" ? "HEAD" : "index",
    to: category === "staged" ? "index" : "working_files", kind: "text",
    fromContent: `${"prefix\n".repeat(7)}unchanged\nold line`,
    toContent: `${"prefix\n".repeat(7)}unchanged\n<img src=x onerror=alert(1)>`,
    hunks: [{ oldStart: 8, oldCount: 2, newStart: 8, newCount: 2, lines: [
      { kind: "context", text: "unchanged" },
      { kind: "removal", text: "old line", noFinalNewline: true },
      { kind: "addition", text: "<img src=x onerror=alert(1)>", noFinalNewline: true },
    ] }],
  });
  const client = reviewClient(read);
  const user = userEvent.setup();
  render(<Workbench client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository" observation={readyObservation()} />);
  await user.click(screen.getByRole("button", { name: "Expand all" }));
  await user.click(screen.getByRole("button", { name: "Review src/example.ts" }));
  await user.click(screen.getByRole("button", { name: "Staged comparison" }));
  const review = await screen.findByRole("region", { name: "Selected file review" });
  expect(within(review).getByLabelText(/^Old context line /)).toHaveTextContent("unchanged");
  expect(within(review).getByLabelText(/^Old removal line /)).toHaveTextContent("old line");
  expect(within(review).getByLabelText(/^New addition line /)).toHaveTextContent("<img src=x onerror=alert(1)>");
  expect(review.querySelector("img")).toBeNull();
  expect(within(review).getByText("No final newline (old side)")).toBeVisible();
  expect(within(review).getByText("No final newline (new side)")).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Unstaged/ }));
});


test("a surviving choice remaps tokens, refreshes content and remains visible when every changed file disappears", async () => {
  const next = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockResolvedValueOnce(textReview("original")).mockReturnValueOnce(next.promise);
  const client = reviewClient(read);
  const props = { client, entryId: "one", selectionGeneration: 0, contextLabel: "Sample repository" };
  const { rerender } = render(<Workbench {...props} observation={readyObservation()} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  await userEvent.setup().click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("original")).toBeVisible();
  rerender(<Workbench {...props} observation={readyObservation([{ ...changedFile, pathId: "path-2" }], 2)} />);
  expect(screen.getByText("original")).toBeVisible();
  expect(screen.getByRole("region", { name: "Last verified file comparison" })).toBeVisible();
  expect(screen.getByText("Updating comparison…")).toBeVisible();
  expect(screen.queryByRole("region", { name: "Read-only file comparison" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^Unstaged/ })).toHaveAttribute("aria-pressed", "true");
  await act(async () => next.resolve(textReview("fresh", { pathId: "path-2" })));
  expect(screen.getByText("fresh")).toBeVisible();
  expect(screen.getByRole("region", { name: "Read-only file comparison" })).toBeVisible();
  expect(screen.queryByRole("region", { name: "Last verified file comparison" })).not.toBeInTheDocument();
  expect(read).toHaveBeenLastCalledWith("one", 2, "path-2", "unstaged");
  rerender(<Workbench {...props} observation={readyObservation([], 3)} />);
  expect(screen.getByRole("heading", { name: "Clean" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "No remaining changes" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "src/example.ts" })).toBeVisible();
  expect(screen.queryByText("fresh")).not.toBeInTheDocument();
});

test("unchanged observation polls and delayed byte rereads preserve source and reading position without checking flashes", async () => {
  vi.useFakeTimers();
  const first = deferred<ReviewResult>();
  const edited = deferred<ReviewResult>();
  const read = vi.fn<RepositoryClient["reviewFile"]>().mockReturnValueOnce(first.promise).mockReturnValueOnce(edited.promise);
  const client = reviewClient(read);
  function LiveReview() {
    const observation = useObservation(client, "one", 0);
    return observation ? <FileReview client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository"
      observation={observation} selection={readingSelection} categories={["unstaged"]} onCategoryChange={() => {}} /> : null;
  }
  render(<LiveReview />);
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(read).toHaveBeenCalledTimes(1);
  await act(async () => first.resolve(textReview("verified source")));
  expect(screen.getByText("verified source")).toBeVisible();
  expect(screen.queryByText("Checking comparison…")).not.toBeInTheDocument();
  const comparison = screen.getByRole("region", { name: "Read-only file comparison" });
  const oldSource = within(comparison).getByRole("region", { name: "Old source hunks" });
  const newSource = within(comparison).getByRole("region", { name: "New source hunks" });
  oldSource.scrollLeft = 71;
  newSource.scrollLeft = 93;
  await act(async () => { await vi.advanceTimersByTimeAsync(6000); });
  expect(read).toHaveBeenCalledTimes(2);
  expect(screen.getByText("verified source")).toBeVisible();
  expect(screen.queryByText("Checking comparison…")).not.toBeInTheDocument();
  expect(screen.queryByText("Updating comparison…")).not.toBeInTheDocument();
  expect(within(comparison).getByRole("region", { name: "Old source hunks" })).toBe(oldSource);
  expect(within(comparison).getByRole("region", { name: "New source hunks" })).toBe(newSource);
  expect(oldSource.scrollLeft).toBe(71);
  expect(newSource.scrollLeft).toBe(93);
  await act(async () => edited.resolve(textReview("externally edited source")));
  expect(screen.getByText("externally edited source")).toBeVisible();
  expect(screen.queryByText("verified source")).not.toBeInTheDocument();
  expect(oldSource.scrollLeft).toBe(71);
  expect(newSource.scrollLeft).toBe(93);
});

test("unsupported, native unavailable and transport loss never retain source or imply no remaining changes", async () => {
  vi.useFakeTimers();
  let reply: ReviewResult = textReview("previous source");
  let offline = false;
  const client = reviewClient(async () => { if (offline) throw new Error("Private failure"); return reply; });
  const props = { client, entryId: "one", selectionGeneration: 0, contextLabel: "Sample repository" };
  render(<Workbench {...props} observation={readyObservation()} />);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Expand all" })); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Review src/example.ts" })); });
  expect(screen.getByText("previous source")).toBeVisible();
  reply = { kind: "unsupported", reason: "binary", identity: reviewIdentity };
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByText("Preview unsupported")).toBeVisible();
  expect(screen.queryByText("previous source")).not.toBeInTheDocument();
  reply = { kind: "unavailable", code: "changed_during_read", identity: reviewIdentity };
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByText("Comparison unavailable")).toBeVisible();
  offline = true;
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(screen.getByText("Desktop connection interrupted")).toBeVisible();
  expect(screen.queryByText("Private failure")).not.toBeInTheDocument();
  expect(screen.queryByText("No remaining changes")).not.toBeInTheDocument();
});

test("client replacement clears the local reading choice immediately", async () => {
  const client = reviewClient(async () => textReview("old source"));
  const props = { entryId: "one", selectionGeneration: 0, contextLabel: "Sample repository", observation: readyObservation() };
  const { rerender } = render(<Workbench {...props} client={client} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  await userEvent.setup().click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("old source")).toBeVisible();
  rerender(<Workbench {...props} client={reviewClient()} />);
  expect(screen.queryByRole("region", { name: "Selected file review" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Review src/example.ts" })).toHaveAttribute("aria-pressed", "false");
});
