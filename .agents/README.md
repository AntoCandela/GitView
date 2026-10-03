# GitView agent toolkit

Repository-local skills and utilities for coding agents. This folder travels with the repository; nothing needs to be installed globally. `.agents/skills` is the canonical skill location. Agents without automatic skill discovery must read the applicable `SKILL.md` through the links in `AGENTS.md`.

Public contributors use [CONTRIBUTING.md](../CONTRIBUTING.md), GitHub issues/PRs and the existing npm/shared verifier. An Auto-K account, private graph access and a configured MCP connector are not prerequisites. Maintainers provide scope approval and contributor-visible acceptance criteria in the issue/PR discussion. Authorized maintainer-only Auto-K work follows the private approval rules in [AGENTS.md](../AGENTS.md); do not publish product identifiers or account configuration.

## Skills

| Skill | Use when |
| --- | --- |
| [Session coordination](skills/gitview-session-coordination/SKILL.md) | Mandatory startup inspection and independent, collaborative or dependent coordination before editing; small shared-checkout changes require agreed ownership. |
| [SQL diagnostics](skills/gitview-sql-diagnostics/SKILL.md) | Querying authorized SQLite logs, following operation IDs, checking capture health, or adding typed native/frontend instrumentation. Includes Rust and SQL examples. |
| [Verification and evidence](skills/gitview-verification/SKILL.md) | Selecting tests, investigating failed checks, using commit/push hooks, reading manifests, or assessing native-preview claims. |
| [Worker isolation](skills/gitview-worker-isolation/SKILL.md) | Preparing concurrent worktrees, running isolated web/native instances, and safely cleaning owned runtime resources. |
| [Task delegation](skills/gitview-task-delegation/SKILL.md) | Splitting substantial work with explicit file ownership, shared interfaces and a single integration owner. |
| [Independent review](skills/gitview-independent-review/SKILL.md) | Evidence-based read-only review and a bounded correction/re-review loop. |
| [Documentation maintenance](skills/gitview-doc-maintenance/SKILL.md) | Checking links, skill contracts and executable examples; proposing scoped drift fixes. |
| [Agent workflow evaluation](skills/gitview-agent-evaluation/SKILL.md) | Running disposable challenges and grading actual diagnostic, instrumentation and evidence outcomes. |

These skills adapt the existing implementation, not generic logging recipes. Persist fixed event/code values and bounded metadata, never arbitrary messages, JSON payloads or exception text. Do not add another logger or SQL endpoint.

Every new session starts with the coordination gate in `AGENTS.md`. The shared worker registry reveals managed worktrees, not all agents or owners of dirty files. These instructions do not install a session-launch hook.

## Agent utilities

Run from the repository root:

```sh
node .agents/scripts/verification-report.mjs --manifest .verification/manifest.json
node .agents/scripts/verification-report.mjs --manifest .verification/pre-push/manifest.json
```

The reporter prints compact JSON with recorded status, check outcomes, safe aggregate counts and current rerun commands. It deliberately omits raw failure descriptions, supplied commands, environment values and arbitrary manifest fields. Exit codes: `0` recorded pass, `1` recorded failure, `2` invalid/unreadable report or arguments. It neither runs checks nor certifies that an old manifest matches the current worktree.

### Worker, documentation and evaluation commands

```sh
npm run agents:worker -- prepare --root "$PWD" --path /tmp/gitview-worker-a --rev HEAD --port 15321
npm run agents:worker -- run --root "$PWD" --path /tmp/gitview-worker-a --mode web
npm run agents:worker -- remove --root "$PWD" --path /tmp/gitview-worker-a
npm run agents:docs
npm run agents:evaluate -- prepare --output .verification/agent-evaluation
# Hand the prepared HANDOFF.md to an actual agent, then grade its submission:
npm run agents:evaluate -- grade --run .verification/agent-evaluation
```

Prepare fresh worktree/run paths; install dependencies separately inside each worker with `npm ci`. Stop the foreground worker before removal. Worktrees contain committed state only; uncommitted toolkit/source changes do not transfer automatically. Runtime supervision is POSIX-only and rejects Windows execution; worktree preparation/ownership and other utilities have their own documented boundaries. These tools do not invoke a model, schedule unattended agents or merge code. Read each linked skill before using its command.

Full shared verification now includes executable documentation. Deterministic infrastructure tests cover worker ownership/process cleanup, documentation boundaries and workflow grading; actual agent attempts remain separate evidence. A native launch was observed creating a schema-validated SQLite store under its unique worker identifier on macOS; this does not certify native picker or packaged-release behavior.

Existing tools remain the implementation owners:

- `gitview-diagnostics`: explicitly authorized, validated read-only SQLite schema/event queries; see the SQL skill.
- `npm run test:unit`, `npm run test:integration`, `npm run verify`: shared verification, including isolated native build artifacts.
- `npm run policy`: test-placement, import-boundary and artifact policy.
- `npm run hooks:install`: preserves existing hook ownership while installing supported hooks.
- `npm run preview:evidence`: checks artifact-bound human observations, not browser/mock readiness.

The reporter's privacy/failure boundaries are covered by `tests/infrastructure/agent-report.test.mjs`, discovered by the shared infrastructure suite. The SQL skill's Rust snippets were compiled and exercised against an isolated real SQLite store; its CLI and bound-parameter SQL examples retrieved the persisted failure and child-correlation evidence. Re-exercise examples when their referenced API changes.

See [DEVELOPMENT.md](../DEVELOPMENT.md) for the daily workflow and [CODE-STYLE.md](../CODE-STYLE.md) for code/test conventions. Root [AGENTS.md](../AGENTS.md) remains authoritative. Public task scope and acceptance criteria belong in the maintainer-approved issue/PR discussion; authorized private Auto-K planning remains conditional and does not gate public contributions.

## Adding another local skill or script

Add a skill at `skills/<name>/SKILL.md` with `name`/`description` frontmatter, clear triggers, existing API references, runnable examples and verification limits. Add scripts under `scripts`, reuse root helpers where appropriate, and exercise the real command before handoff. Update this index and `AGENTS.md` so agents can find it. Keep scripts focused; do not duplicate existing native or verification APIs. Do not create consumer-specific copies of skills or overwrite another agent's configuration.
