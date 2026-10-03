# Repository Instructions

## Mandatory session-start gate

Before editing or starting a worker/runtime in every new session, MUST read [session coordination](.agents/skills/gitview-session-coordination/SKILL.md) and inspect the current checkout and shared managed-worker registry. Repeat the gate when the task, checkout or handoff changes.

When other managed work or explicitly reported concurrent work exists, MUST present these choices and establish the selected ownership contract before proceeding: **independent feature** in a separate worker from an explicit committed baseline; **collaborate on existing work** with its integration owner and agreed disjoint files (including small changes in the same checkout); or **dependent feature** waiting for a reviewed committed handoff.

Dirty files do not identify another agent or their owner; unknown ownership MUST remain unknown. Never auto-commit, stash or copy unfinished changes to create a baseline. This gate is an instruction for compliant agents, not an automatic session launcher or a guarantee that unrelated sessions are detected.

## Public contributions

Use [CONTRIBUTING.md](CONTRIBUTING.md), ordinary GitHub issues and pull requests, and the existing npm/shared verification workflow. No Auto-K account, connector, private product graph or global agent installation is required. Maintainers approve product scope through the issue/PR discussion; an outsider must not need access to private planning artifacts to understand the agreed change or acceptance criteria.

The mandatory session gate and explicit ownership contracts above apply to public contributions too. Keep private identifiers, account configuration, diagnostics and local paths out of issues, PRs and committed evidence. Follow [SECURITY.md](SECURITY.md) for vulnerabilities and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for community conduct.

## Auto-K (authorized maintainer work only)

Only when an authorized maintainer explicitly requests Auto-K work and supplies access, use the configured Auto-K MCP connector. Public contribution tasks proceed without it. If that private connector or authorization is unavailable, do not substitute guessed product decisions or grant broader access.

Use `search_tools` to discover Auto-K tools. Use `call_tool` when the required tool is not directly available.

Before product-graph work, load `auto-k://skills/autok-product-graph`. If MCP prompts are unavailable, discover `read_content` and read that URI.

List products before graph work. Resolve the selected product and its explicit `product_id` from maintainer-provided private context and the authorized product list; pass it to every product-scoped call. Keep product identifiers and account setup outside public documentation and contribution reports.

Never infer the selected product from the repository name alone. Never change a product graph until the product is unambiguous.

Before any product-graph mutation or downstream Auto-K product artifact authoring, use Grill The Idea. Get explicit approval of the problem, target users, product goals, scope, and success signals. Code can inform the proposal, but it cannot approve product intent or prove that an accepted brief is current.

If no matching product exists, do not create one from only an approved name. Confirm the full foundation first. Then create the product with the approved name and decision-brief description.

Creation requires organization Admin access. Include the confirmed target organization's UUID as `value.organization_id` when the home account has multiple eligible organizations or when creating in an organization owned by another account. Never guess the destination from an existing project's name.

## Development workflow

MUST follow [DEVELOPMENT.md](DEVELOPMENT.md) when developing, debugging or verifying GitView. It covers authorized local diagnostic queries, shared test commands, Git hooks, failure evidence and native-preview verification limits.

## Repository-local agent toolkit

Use [.agents/README.md](.agents/README.md) to discover repository-local skills and scripts; no global installation is required.

- MUST read [gitview-sql-diagnostics](.agents/skills/gitview-sql-diagnostics/SKILL.md) before querying local SQL logs, diagnosing capture health or adding instrumentation. Use typed events/codes and bounded metadata, not arbitrary messages or JSON payloads.
- MUST read [gitview-verification](.agents/skills/gitview-verification/SKILL.md) when choosing checks, investigating verification failures, using hooks or reporting native-preview evidence.
- MUST read [worker isolation](.agents/skills/gitview-worker-isolation/SKILL.md) and [task delegation](.agents/skills/gitview-task-delegation/SKILL.md) before coordinating concurrent agents. Assign file ownership and a single integration owner; worktrees contain committed state only.
- MUST read [independent review](.agents/skills/gitview-independent-review/SKILL.md) for substantial/risky changes. Reviewers are read-only; review feedback does not replace passing checks or actual smoke evidence.
- MUST read [documentation maintenance](.agents/skills/gitview-doc-maintenance/SKILL.md) when changing local skills/examples, and [agent evaluations](.agents/skills/gitview-agent-evaluation/SKILL.md) when evaluating agent workflows. Distinguish deterministic grader tests from actual agent runs.

## UI design

MUST follow [DESIGN-RULES.md](DESIGN-RULES.md) when changing the frontend.

## Code clarity

MUST follow [CODE-STYLE.md](CODE-STYLE.md) when changing handwritten Rust, frontend source or tests. Keep file headers focused on purpose and ownership, API documentation focused on caller contracts, and inline comments focused on rationale and invariants. Prefer clear names and code over redundant commentary; update comments and callers when contracts change.
