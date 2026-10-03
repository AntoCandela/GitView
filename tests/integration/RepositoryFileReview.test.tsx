/** Exercises full working-file previews and rejects superseded native listing completions. */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { RepositoryFileResult, RepositoryFileSelection } from "../../src/contracts/browsing";
import { RepositoryFileReview } from "../../src/features/diff";
import { Workbench } from "../../src/app/workbench/Workbench";
import { deferred } from "../support/deferred";
import { readyObservation, reviewClient, textReview } from "../support/review";

afterEach(cleanup);
const selection: RepositoryFileSelection = { id: "readme", listingId: "listing-one", displayPath: "README.txt", segments: ["README.txt"] };
const props = { entryId: "one", selectionGeneration: 0, contextLabel: "Sample repository", selection };
function text(content: string, file = selection): RepositoryFileResult {
  return { kind: "text", entryId: "one", listingId: file.listingId, fileId: file.id, displayPath: file.displayPath, content };
}

test("unchanged working file shows full contents with wrapping and no change-mode choice", async () => {
  const client = reviewClient();
  client.reviewRepositoryFile = async () => text("unchanged working contents\n");
  render(<RepositoryFileReview {...props} client={client} />);
  expect(await screen.findByText("unchanged working contents")).toBeVisible();
  expect(screen.queryByRole("radiogroup", { name: "Code view" })).not.toBeInTheDocument();
  await userEvent.setup().click(screen.getByRole("radio", { name: "Wrap" }));
  expect(screen.getByText("unchanged working contents")).toBeVisible();
});

test("listing replacement immediately hides old bytes and discards a late previous listing read", async () => {
  const client = reviewClient();
  const late = deferred<RepositoryFileResult>();
  const next = { ...selection, listingId: "listing-two" };
  client.reviewRepositoryFile = (_entry, listing) => listing === "listing-one" ? late.promise : Promise.resolve(text("current listing bytes\n", next));
  const { rerender } = render(<RepositoryFileReview {...props} client={client} />);
  rerender(<RepositoryFileReview {...props} selection={next} client={client} />);
  expect(await screen.findByText("current listing bytes")).toBeVisible();
  await act(async () => late.resolve(text("obsolete listing bytes\n")));
  expect(screen.queryByText("obsolete listing bytes")).not.toBeInTheDocument();
  expect(screen.getByText("current listing bytes")).toBeVisible();
});

test("selection generation change clears verified content while the next read is pending", async () => {
  const client = reviewClient();
  const pending = deferred<RepositoryFileResult>();
  client.reviewRepositoryFile = async () => text("old generation bytes\n");
  const { rerender } = render(<RepositoryFileReview {...props} client={client} />);
  expect(await screen.findByText("old generation bytes")).toBeVisible();
  client.reviewRepositoryFile = () => pending.promise;
  rerender(<RepositoryFileReview {...props} selectionGeneration={1} client={client} />);
  expect(screen.queryByText("old generation bytes")).not.toBeInTheDocument();
  await act(async () => pending.resolve(text("new generation bytes\n")));
  expect(await screen.findByText("new generation bytes")).toBeVisible();
});

test("a mismatched native file identity never renders its content", async () => {
  const client = reviewClient();
  client.reviewRepositoryFile = async () => ({ ...text("wrong file bytes\n"), fileId: "another-file" } as RepositoryFileResult);
  render(<RepositoryFileReview {...props} client={client} />);
  expect(await screen.findByRole("button", { name: "Retry preview" })).toBeVisible();
  expect(screen.queryByText("wrong file bytes")).not.toBeInTheDocument();
});

test("live-file activation dismisses the browsing override and restores the live comparison", async () => {
  const client = reviewClient(async () => textReview("live comparison bytes"));
  client.reviewRepositoryFile = async () => text("browse-only bytes\n");
  function BrowsingWorkbench() {
    const [file, setFile] = useState<RepositoryFileSelection | null>(selection);
    return <Workbench client={client} entryId="one" selectionGeneration={0} contextLabel="Sample repository"
      observation={readyObservation()} repositoryFile={file} onRepositoryFileDismiss={() => setFile(null)} />;
  }
  render(<BrowsingWorkbench />);
  expect(await screen.findByText("browse-only bytes")).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Expand all" }));
  await userEvent.setup().click(screen.getByRole("button", { name: "Review src/example.ts" }));
  expect(await screen.findByText("live comparison bytes")).toBeVisible();
  expect(screen.queryByText("browse-only bytes")).not.toBeInTheDocument();
});

test("an expired listing shows no bytes and requires renewed file selection", async () => {
  const client = reviewClient();
  client.reviewRepositoryFile = async () => ({ kind: "stale_selection" });
  render(<RepositoryFileReview {...props} client={client} />);
  expect(await screen.findByRole("heading", { name: "File selection expired" })).toBeVisible();
  expect(screen.queryByRole("region", { name: "Read-only file comparison" })).not.toBeInTheDocument();
});

test("retry reads current working bytes after a recoverable unavailable result", async () => {
  const client = reviewClient();
  let available = false;
  client.reviewRepositoryFile = async () => available ? text("recovered working bytes\n") : { kind: "unavailable", code: "inaccessible" };
  render(<RepositoryFileReview {...props} client={client} />);
  const retry = await screen.findByRole("button", { name: "Retry preview" });
  available = true;
  await userEvent.setup().click(retry);
  expect(await screen.findByText("recovered working bytes")).toBeVisible();
});
