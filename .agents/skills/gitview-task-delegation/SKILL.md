---
name: gitview-task-delegation
description: Split substantial GitView work between agents, define file ownership and interfaces, and integrate independently verified results without scope drift.
---

# Contract-first delegation

Use one integration owner and self-contained worker contracts. Delegate independent substantial slices, not trivial edits or a task already understood by one agent. Read the existing owners and callers before drawing slice boundaries. [CODE-STYLE.md](../../../CODE-STYLE.md) defines implementation boundaries; [worker isolation](../gitview-worker-isolation/SKILL.md) defines runtime ownership.

## Before spawning

Complete the [session-start coordination gate](../gitview-session-coordination/SKILL.md) first. Registry records do not supply an integration owner or file claims; obtain those from the task contract, not dirty files. For dependent slices, require a reviewed committed handoff before preparing the worker.

- Preserve the user's complete request and acceptance criteria. Identify shared prerequisites inline; parallelize only independent slices.
- For public contributions, cite the GitHub issue/PR or the user's request with a stable local label and carry the maintainer-approved scope and acceptance criteria in the contract. No Auto-K account or private graph access is required. Use an existing Auto-K task ID only for explicitly authorized maintainer-private work; keep private identifiers out of public handoffs. Do not invent an Auto-K ID, create product truth, or duplicate private requirements into a local tracker.
- Record the exact baseline revision and whether the parent has uncommitted changes. A fresh worktree contains committed state only: workers must not assume current dirty edits are present. Transfer only specifically approved prerequisites; never auto-commit, stash or copy the whole parent tree.
- Assign relative owned file paths and explicit shared interfaces. Separate conceptual features can still collide in the same file. Reserve shared files for the integration owner.
- Give workers the relevant skills, existing APIs, non-goals and source evidence. Parent conversation is not inherited automatically.
- Assign named dependencies and the party who resolves each. Send interface clarifications directly to affected workers rather than serializing independent tasks.

## Worker contract

Include this structure in each delegation message; replace descriptions with the actual task facts:

```text
Task: existing task ID or stable user-request label
Baseline: exact revision; parent dirty state; transferred prerequisites
Goal: required observable behavior, preserving the full user intent
Owns: exact relative files/directories; integration owner for shared files
Inputs: inspected code/API references and applicable local skills
Interface: exact types/functions/CLI arguments/files consumed by other slices
Non-goals: behavior or files that must not change
Acceptance: observable outcomes and edge cases, not merely compilation
Verification: exact commands and real scenario; identify integration-owner runs
Dependencies: named prerequisite/provider; blocker resolution owner
Handoff: changed files, observed evidence, risks, remaining blockers
```

Acceptance criteria belong to the shared user task and, for public contributions, its maintainer-approved issue/PR discussion—not independently invented product goals or inaccessible private planning artifacts. Clarify genuinely unresolved intent with the user or maintainer; repository/tool facts are research, not questions for the user. Maintainers retain scope approval. The authorized private Auto-K approval rules in [AGENTS.md](../../../AGENTS.md) still apply when Auto-K is explicitly requested.

## Execution and integration

- Each worker edits only its owned paths. Unexpected user edits stay intact. A shared-file change requires the integration owner's coordination; do not race another agent's patch.
- The worker reports observed facts separately from inference, including failed checks and unavailable evidence. A model's `done` statement is not proof.
- The integration owner reads the actual resulting contract and integrates all callers without compatibility shims. Reconcile overlapping findings and inspect the combined behavior, not just per-worker success.
- Do not have every worker launch the same builds, formatters or suites concurrently. Workers provide focused scenario instructions; the integration owner runs consolidated checks once after integration. Separate worker-owned runtime fixtures remain isolated.
- Request [independent review](../gitview-independent-review/SKILL.md) for substantial or risky changes, then exercise the actual changed surface and use the [shared verifier](../gitview-verification/SKILL.md).
- Only the integration owner reports completion, after every named acceptance criterion and reachable blocker is resolved. No agent automatically stages, commits, merges, promotes Auto-K statuses, or removes someone else's worktree.

A worktree separates files and app state; it is not a security sandbox. Do not delegate arbitrary untrusted code execution or grant broader filesystem/product access than the task requires.
