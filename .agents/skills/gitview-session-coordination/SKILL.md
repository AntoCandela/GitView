---
name: gitview-session-coordination
description: Mandatory GitView session-start gate before editing or launching workers; inspect managed work and checkout state, then select independent, collaborative or dependent coordination with explicit ownership.
---

# Session-start coordination

Run this gate in every new session before editing, preparing a worker or launching runtime commands. Read-only scoping and inspection are allowed. Repeat it after switching task/checkouts, receiving a handoff, or learning about concurrent work. [AGENTS.md](../../../AGENTS.md) requires the gate; [worker isolation](../gitview-worker-isolation/SKILL.md) owns runtime tooling and [task delegation](../gitview-task-delegation/SKILL.md) owns the task contract.

## Inspect without claiming ownership

From the actual checkout, run these read-only commands:

```sh
git rev-parse --show-toplevel
git rev-parse --path-format=absolute --git-common-dir
git rev-parse --verify HEAD
git status --porcelain=v1 --untracked-files=all
git worktree list --porcelain
```

1. Record the canonical checkout, exact committed revision and changed/untracked paths relevant to the request. Preserve all existing changes. Dirty state describes files, not the identity, task or activity of their editor.
2. Use the returned **shared Git common directory**, not an assumed checkout-local `.git` directory. Read its `gitview-workers/` directory with the file-reading tool. A missing directory means no registered managed workers were found, not that no other sessions exist. Do not create a registry merely to inspect it.
3. Read each top-level `<24-hex-worker-id>.json` record. `ports/` contains reservations, not extra workers. Compare schema version, ID/filename, common directory, root and revision with the registered Git worktree and its `.verification/worker/ownership.json`. Do not follow unverified records into arbitrary source or app-data directories. Missing, inconsistent, symlinked or unreadable metadata is **unknown/unverified**, not a disposable stale worker.
4. For a verified worker, resolve its Git admin directory with `git -C WORKER_ROOT rev-parse --absolute-git-dir` and inspect whether `gitview-worker-active` exists. A lock means **active or interrupted**; it is not proof of a live agent. A prepared unlocked worker still represents managed work that may require coordination. Neither the registry nor this lock records a task contract, integration owner or file claims.
5. Separate the current managed checkout from other managed workers; do not count it twice as another session. Inspect the current task/delegation conversation and any owner-provided handoff for explicit ownership. Report unknown owners as unknown. Never infer them from dirty files, timestamps, process names or an absent lock. Do not kill processes, clear locks, reserve ports or remove workers during inspection.

Keep private checkout paths and registry/runtime payloads local. The registry does not discover arbitrary sessions sharing the same checkout, and this gate is not a file-locking protocol or security sandbox.

## Decide before editing

If another managed worker exists, concurrent work is explicitly reported, or unresolved registry metadata prevents a safe decision, briefly state the observed facts and **offer all three paths below**. Ask for the selection and missing ownership/baseline decision together. If the user already explicitly selected a path for this task, restate the three options and apply that selection without asking again; still resolve its prerequisites before editing.

Example choice presentation:

- **Independent feature** — separate worker from an explicit committed baseline; disjoint owned files and one integration owner.
- **Collaborate on existing work** — join its coordination contract; agree files/interfaces with the integration owner. Small changes may share the checkout when ownership is explicitly disjoint.
- **Dependent feature** — wait for a reviewed committed prerequisite handoff; then prepare a separate worker from that exact commit.

A worktree is not mandatory for every small change. Conversely, calling a change small does not establish permission to race another editor. Do not substitute a dirty parent tree for a committed baseline.

When no managed or reported concurrent work is found, proceed in the current checkout within the user's requested scope. State the detection limit rather than claiming exclusivity. If requested edits overlap pre-existing changes whose ownership/intent cannot be resolved from context, pause those edits and ask the user or integration owner; unrelated dirty paths alone do not force a concurrency prompt.

## Independent feature

Read the worker-isolation skill and obtain an explicit committed revision suitable for the task. Use its existing `prepare --root ROOT --path FRESH_EXTERNAL_PATH --rev EXACT_COMMIT --port PORT` command. Install dependencies in the new worker, then edit only the agreed disjoint paths. Creating the worker does not include unfinished parent edits.

Record the task label, baseline, owned files, shared interfaces and integration owner using the existing delegation contract. If required APIs exist only in unfinished work, this path has an unmet prerequisite: choose collaboration or a dependent handoff instead of copying that work or manufacturing a commit. Integration and commits remain human-controlled.

## Collaborate on existing work

Obtain the existing task/coordination contract and identify its integration owner. Agree the new session's exact files, shared interfaces and verification responsibility before editing. Separate feature names are not evidence of disjoint files.

A small change may run in the same checkout when the integration owner confirms disjoint ownership and coordinates shared files. The owner also controls shared build/runtime resources and the consolidated checks; do not launch competing builds or repurpose another worker's port/cache. A shared-file edit requires an explicit handoff or reservation from that owner, not concurrent patches.

If the owner or contract is unknown, do not invent either. Ask for the contract/owner or select an independent committed baseline. Existing worker runtime locks protect child lifecycle, not source-file ownership.

## Dependent feature

Name the prerequisite and its provider/integration owner. Wait for a **reviewed committed handoff** containing the exact commit, available interfaces, acceptance evidence and owned files. Until then, read-only preparation is allowed, but do not edit dependent code, start a dependent worker from an unsuitable revision, or stage/commit/copy unfinished prerequisite changes.

After receiving the handoff, re-run inspection, verify the commit is available and suitable, then prepare the worker from that exact commit with the existing isolation tool. A model's completion message, a dirty checkout or a currently passing local check is not the committed handoff.

## Record the outcome and limits

Before proceeding, state the chosen path, task label, exact baseline, checkout/worker, owned files, integration owner and relevant unresolved ownership. Keep the contract in the task/delegation conversation; do not create a competing product backlog or pretend the existing registry stores these claims. Re-run the gate if those facts change.

These are mandatory repository instructions for compliant agents. No session-launch hook is installed: a guaranteed prompt at session creation requires a supported launcher integration. Do not modify global agent configuration or claim automatic coordination. Maintainer scope approval still applies independently of the selected path. Auto-K review requirements apply only to explicitly authorized maintainer-private Auto-K work; public contributions need no private graph access.
