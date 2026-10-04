/** Exercises adapter correlation and promise isolation when diagnostic delivery is delayed or fails. */

import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { RepositoryCommand } from "../../src/contracts/Diagnostics";
import type { RepositoryClient, WorkspaceSnapshot } from "../../src/contracts/repositories";
import { repositoryClient } from "../../src/platform/RepositoryClient";
import { deferred } from "../support/deferred";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const firstId = "00000000-0000-4000-8000-000000000001";
const secondId = "00000000-0000-4000-8000-000000000002";
const snapshot: WorkspaceSnapshot = {
  revision: 1, entries: [], activeContextId: null, restoring: false, persistenceError: null,
};
const operations: { command: RepositoryCommand; start: (client: RepositoryClient) => Promise<unknown> }[] = [
  { command: "workspace_snapshot", start: (client) => client.snapshot() },
  { command: "open_chosen_repository", start: (client) => client.openChosenRepository("en-US") },
  { command: "select_context", start: (client) => client.selectContext("entry") },
  { command: "refresh_entry_availability", start: (client) => client.refreshEntryAvailability("entry") },
  { command: "observe_selected_context", start: (client) => client.observeSelectedContext("entry") },
  { command: "review_file", start: (client) => client.reviewFile("entry", 7, "opaque-path", "unstaged") },
  { command: "history_page", start: (client) => client.historyPage("entry", "opaque-cursor") },
  { command: "list_contexts", start: (client) => client.listContexts("entry") },
  { command: "select_worktree", start: (client) => client.selectWorktree("entry", "opaque-worktree") },
  { command: "commit_files", start: (client) => client.commitFiles("entry", "a".repeat(40), "b".repeat(40)) },
  { command: "review_commit_file", start: (client) => client.reviewCommitFile("entry", "a".repeat(40), "b".repeat(40), "opaque-file") },
  { command: "list_repository_files", start: (client) => client.listRepositoryFiles("entry", { listingId: null, directoryId: null, cursor: null }) },
  { command: "review_repository_file", start: (client) => client.reviewRepositoryFile("entry", "opaque-listing", "opaque-file") },
  { command: "rename_repository", start: (client) => client.renameRepository("entry", "Private display name") },
  { command: "remove_repository", start: (client) => client.removeRepository("entry") },
];

function installTransport(transport: (command: string, args?: InvokeArgs) => Promise<unknown>) {
  // The scenario owns each reply; Tauri's generic result type is erased at this test boundary.
  vi.mocked(invoke).mockImplementation(<T>(command: string, args?: InvokeArgs) => transport(command, args) as Promise<T>);
}

function terminalPayload(args?: InvokeArgs) {
  if (!args || !("diagnostic" in args)) throw new Error("Missing diagnostic envelope");
  return args.diagnostic;
}

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.spyOn(crypto, "randomUUID").mockReturnValueOnce(firstId).mockReturnValueOnce(secondId);
  vi.spyOn(performance, "now").mockReturnValue(0);
});
afterEach(() => vi.restoreAllMocks());

test.each(operations)("$command preserves rejection identity and emits only safe transport failure metadata", async ({ command, start }) => {
  const reply = deferred<unknown>();
  const terminal: unknown[] = [];
  const failure = { privatePath: "/private/repository", toString: () => { throw new Error("Do not inspect failure payloads"); } };
  installTransport((invokedCommand, args) => {
    if (invokedCommand === "record_renderer_diagnostic") {
      terminal.push(terminalPayload(args));
      return Promise.resolve();
    }
    return reply.promise;
  });

  const pending = start(repositoryClient);
  const rejection = expect(pending).rejects.toBe(failure);
  expect(terminal).toEqual([]);
  vi.mocked(performance.now).mockReturnValue(12.75);
  reply.reject(failure);
  await rejection;

  expect(terminal).toEqual([{ operationId: firstId, command, phase: "transport_failed", durationMs: 12 }]);
});

test("a resolved domain rejection is a completed transport and never waits for diagnostic delivery", async () => {
  const reply = deferred<unknown>();
  const delivery = deferred<unknown>();
  const terminal: unknown[] = [];
  const outcome = { kind: "rejected", code: "not_repository", snapshot };
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      terminal.push(terminalPayload(args));
      return delivery.promise;
    }
    return reply.promise;
  });

  const pending = repositoryClient.openChosenRepository("en-US");
  let settledResult: unknown;
  void pending.then((value) => { settledResult = value; });
  expect(terminal).toEqual([]);
  reply.resolve(outcome);
  await Promise.resolve();
  await Promise.resolve();
  expect(settledResult).toBe(outcome);
  await expect(pending).resolves.toBe(outcome);
  expect(terminal).toEqual([{ operationId: firstId, command: "open_chosen_repository", phase: "completed", durationMs: 0 }]);

  delivery.reject(new Error("Diagnostic bridge disconnected"));
  await Promise.resolve();
});

test("rejected diagnostic delivery cannot replace a successful repository result", async () => {
  const delivery = deferred<unknown>();
  installTransport((command) => command === "record_renderer_diagnostic"
    ? delivery.promise
    : Promise.resolve(snapshot));

  const pending = repositoryClient.snapshot();
  let settledResult: unknown;
  void pending.then((value) => { settledResult = value; });
  await Promise.resolve();
  await Promise.resolve();
  expect(settledResult).toBe(snapshot);
  await expect(pending).resolves.toBe(snapshot);
  delivery.reject(new Error("Diagnostic bridge disconnected"));
  await Promise.resolve();
  await expect(pending).resolves.toBe(snapshot);
});

test("rejected diagnostic delivery cannot replace the original repository failure", async () => {
  const reply = deferred<unknown>();
  const delivery = deferred<unknown>();
  const failure = { reason: "Original opaque rejection" };
  installTransport((command) => command === "record_renderer_diagnostic"
    ? delivery.promise
    : reply.promise);

  const pending = repositoryClient.snapshot();
  let settledFailure: unknown;
  void pending.catch((error: unknown) => { settledFailure = error; });
  const rejection = expect(pending).rejects.toBe(failure);
  reply.reject(failure);
  await Promise.resolve();
  await Promise.resolve();
  expect(settledFailure).toBe(failure);
  await rejection;
  delivery.reject(new Error("Diagnostic bridge disconnected"));
  await Promise.resolve();
  await expect(pending).rejects.toBe(failure);
});

test("a synchronous diagnostic submission failure leaves repository success unchanged", async () => {
  installTransport((command) => {
    if (command === "record_renderer_diagnostic") throw new Error("Bridge unavailable");
    return Promise.resolve(snapshot);
  });

  await expect(repositoryClient.snapshot()).resolves.toBe(snapshot);
});

test("concurrent operations retain their own UUID and elapsed time when completed in reverse order", async () => {
  const firstReply = deferred<unknown>();
  const secondReply = deferred<unknown>();
  const terminal: unknown[] = [];
  const requestedIds: unknown[] = [];
  const failure = { reason: "Second operation failed" };
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      terminal.push(terminalPayload(args));
      return Promise.resolve();
    }
    requestedIds.push(args && "operationId" in args ? args.operationId : undefined);
    return command === "workspace_snapshot" ? firstReply.promise : secondReply.promise;
  });

  vi.mocked(performance.now).mockReturnValue(10);
  const first = repositoryClient.snapshot();
  vi.mocked(performance.now).mockReturnValue(20);
  const second = repositoryClient.selectContext("entry");
  const rejection = expect(second).rejects.toBe(failure);
  expect(terminal).toEqual([]);
  vi.mocked(performance.now).mockReturnValue(25.9);
  secondReply.reject(failure);
  await rejection;
  vi.mocked(performance.now).mockReturnValue(50.9);
  firstReply.resolve(snapshot);
  await expect(first).resolves.toBe(snapshot);

  expect(requestedIds).toEqual([firstId, secondId]);
  expect(terminal).toEqual([
    { operationId: secondId, command: "select_context", phase: "transport_failed", durationMs: 5 },
    { operationId: firstId, command: "workspace_snapshot", phase: "completed", durationMs: 40 },
  ]);
});

test.each([
  { startedAt: 10, completedAt: 9, expectedDuration: 0 },
  { startedAt: 10, completedAt: 86_400_011, expectedDuration: 86_400_000 },
])("terminal duration remains bounded for clock readings $startedAt to $completedAt", async ({ startedAt, completedAt, expectedDuration }) => {
  const reply = deferred<unknown>();
  const terminal: unknown[] = [];
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      terminal.push(terminalPayload(args));
      return Promise.resolve();
    }
    return reply.promise;
  });

  vi.mocked(performance.now).mockReturnValue(startedAt);
  const pending = repositoryClient.snapshot();
  vi.mocked(performance.now).mockReturnValue(completedAt);
  reply.resolve(snapshot);
  await pending;

  expect(terminal).toEqual([expect.objectContaining({ durationMs: expectedDuration })]);
});

test("successful review diagnostics contain no source, display paths, endpoint identity or domain payload", async () => {
  const diagnosticPayloads: unknown[] = [];
  const result = {
    kind: "text", entryId: "opaque-entry", pathId: "opaque-path", category: "unstaged",
    displayPath: "/private/source.ts", contextLabel: "Private repository", from: "index", to: "working_files",
    fromAbsent: false, toAbsent: false,
    hunks: [{ oldStart: 1, oldCount: 0, newStart: 1, newCount: 1, lines: [{ kind: "addition", text: "private credential content" }] }],
  };
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      diagnosticPayloads.push(terminalPayload(args));
      return Promise.resolve();
    }
    return Promise.resolve(result);
  });
  await expect(repositoryClient.reviewFile("opaque-entry", 1, "opaque-path", "unstaged")).resolves.toBe(result);
  expect(diagnosticPayloads).toEqual([{ operationId: firstId, command: "review_file", phase: "completed", durationMs: 0 }]);
});

test("history tracing omits commit OIDs, subjects, refs and opaque cursors from terminal diagnostics", async () => {
  const payloads: unknown[] = [];
  const result = {
    kind: "page", page: {
      entryId: "opaque-entry", cursor: "private-pinned-cursor", hasMore: true, completeness: "paged",
      commits: [{ oid: "a".repeat(40), subject: "Private commit subject", parents: [{ oid: "b".repeat(40), state: "outside_page" }], root: false }],
      refs: [{ kind: "local_branch", name: "private-feature", commitOid: "a".repeat(40) }],
      head: { scope: "worktree", state: "attached", branch: "private-feature", oid: "a".repeat(40) },
    },
  };
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      payloads.push(terminalPayload(args));
      return Promise.resolve();
    }
    return Promise.resolve(result);
  });
  await expect(repositoryClient.historyPage("opaque-entry", "private-pinned-cursor")).resolves.toBe(result);
  expect(payloads).toEqual([{ operationId: firstId, command: "history_page", phase: "completed", durationMs: 0 }]);
});

test.each(["list_contexts", "select_worktree", "commit_files"] as const)("%s diagnostics exclude branch, worktree and committed-file payloads", async (command) => {
  const payloads: unknown[] = [];
  const result = { privateBranch: "secret-topic", privatePath: "/private/worktree", privateCommit: "a".repeat(40) };
  installTransport((invokedCommand, args) => {
    if (invokedCommand === "record_renderer_diagnostic") {
      payloads.push(terminalPayload(args));
      return Promise.resolve();
    }
    return Promise.resolve(result);
  });
  const operation = operations.find((item) => item.command === command)!;
  await operation.start(repositoryClient);
  expect(payloads).toEqual([{ operationId: firstId, command, phase: "completed", durationMs: 0 }]);
});

test.each(["resolved", "rejected"] as const)("native language discovery remains outside diagnostics when %s", async (state) => {
  const payloads: unknown[] = [];
  const privateFailure = { environment: "private-desktop-preferences" };
  installTransport((command, args) => {
    if (command === "record_renderer_diagnostic") {
      payloads.push(terminalPayload(args));
      return Promise.resolve();
    }
    return state === "resolved"
      ? Promise.resolve({ languages: ["pt-PT", "en-GB"] })
      : Promise.reject(privateFailure);
  });
  const result = repositoryClient.preferredLanguages();
  if (state === "resolved") await result;
  else await expect(result).rejects.toBe(privateFailure);
  expect(payloads).toEqual([]);
  expect(crypto.randomUUID).not.toHaveBeenCalled();
});
