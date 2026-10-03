---
name: gitview-session-coordination
description: Mandatory GitView session-start gate before editing or launching workers; inspect managed work and checkout state, then select independent, collaborative or dependent coordination with explicit ownership.
---

# Choose how to work

First inspect the checkout and shared worker registry as required by [AGENTS.md](../../../AGENTS.md). If other work is active or ownership is unclear, present these three options before editing.

## 1. Independent work — separate worktree and branch

1. Create a separate worktree from an agreed committed baseline using [worker setup](../gitview-worker-isolation/SKILL.md), then create a branch inside it.
2. Reuse the issue provided at the start, or create one if none was provided.
3. Work and verify in that worktree.
4. Open a PR linked to the issue (`Closes #N`).

## 2. Collaborate — same branch and worktree

Use the existing branch and checkout/worktree for the same change. Agree who edits which files; one integration owner coordinates shared files, Git operations and verification. Work can happen at the same time on agreed disjoint files. Use the same issue and PR.

## 3. Dependent work — wait for another change

Wait for the prerequisite change to have a reviewed committed handoff. Then follow option 1 from that agreed commit, linking the prerequisite issue/PR as well. Never copy, stash or auto-commit someone else's unfinished work to create the baseline.
