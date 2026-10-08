/** Synthetic PR snapshots and a closed client for review lifecycle fixtures. */
import { vi } from "vitest";
import type { PrOpenResult, PullRequestClient } from "../../src/contracts/pullRequests";

export function prSnapshot(sessionId = "session", revision = 1): Extract<PrOpenResult, { kind: "snapshot" }> {
  const empty = { items: [], totalCount: 0, nextCursor: null, completeness: "complete" as const, observedRevision: revision };
  return {
    kind: "snapshot", sessionId, revision, prId: "pr", observedAt: 1000, freshness: "fresh", availability: null,
    overview: {
      number: 42, baseRepository: { id: "repo", host: "github.com", owner: "fixture", name: "project", url: "https://github.com/fixture/project" },
      headRepository: null, headRef: "topic", headOid: `head-${revision}`, baseRef: "main", baseOid: "base",
      title: "Fixture PR", url: "https://github.com/fixture/project/pull/42", createdAt: "2026-01-01T00:00:00Z", updatedAt: "2026-01-01T00:00:00Z",
      closedAt: null, mergedAt: null, counts: { commits: 1, files: 1, additions: 1, deletions: 0 }, body: { kind: "empty" }, author: null,
      lifecycle: "open", draft: false, reviewDecision: null, reviewers: empty, labels: empty,
    },
    sections: { commits: "not_loaded", timeline: "not_loaded", threads: "not_loaded", threadComments: "not_loaded", reviewers: "available", labels: "available" },
  };
}

export function prClient(): PullRequestClient {
  const unavailable = () => Promise.resolve({ kind: "unavailable" as const, code: "integration_unavailable" as const });
  return {
    status: vi.fn(unavailable), associations: vi.fn(unavailable), mapHead: vi.fn(unavailable), choose: vi.fn(unavailable),
    open: vi.fn(async () => prSnapshot()), refresh: vi.fn(async () => prSnapshot("session", 2)),
    page: vi.fn(unavailable), compare: vi.fn(unavailable), filesPage: vi.fn(unavailable), resolveAnchor: vi.fn(unavailable),
    file: vi.fn(unavailable), release: vi.fn(async () => ({ kind: "released" as const })), openLink: vi.fn(unavailable),
  };
}
