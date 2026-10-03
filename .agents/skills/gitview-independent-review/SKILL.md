---
name: gitview-independent-review
description: Review substantial GitView changes independently for correctness, privacy, trace causality and maintained contracts, with bounded evidence-based feedback.
---

# Independent review

The implementing agent owns fixes. A separate reviewer is read-only and inspects the task, affected code and callers, not merely the implementer's summary. Review is not a test runner or authority to merge. Use [task delegation](../gitview-task-delegation/SKILL.md) for ownership and [verification](../gitview-verification/SKILL.md) for proof boundaries.

## Review input

Provide the original acceptance criteria, baseline revision, exact changed paths, preserved contracts, relevant skills and observed verification/smoke evidence. Explicitly identify uncommitted prerequisites and concurrent user changes. Scope the review to this task; do not classify unrelated work as an introduced regression.

Ask the reviewer to inspect:

- Observable outcomes and failure transitions, stale results, cancellation and complete child-process cleanup.
- Existing callers and ownership boundaries in [CODE-STYLE.md](../../../CODE-STYLE.md); avoid duplicate stores, logging APIs or compatibility paths.
- Diagnostic privacy, fixed vocabularies, bounded metadata and operation/parent correlation using [SQL diagnostics](../gitview-sql-diagnostics/SKILL.md).
- Whether the evidence covers the actual scenario and revision. Tests, browser mocks, recorded manifests and native observations establish different facts.

The reviewer does not edit files, launch competing builds, access unauthorized local stores, invent requirements, or demand refactoring unrelated to the change. Cite a precise implementation risk; no style-only findings or speculative feature requests.

## Finding contract

```text
Finding ID: stable local identifier
Severity: blocking | nonblocking
Location: relative path, symbol and current line range
Criterion: exact violated caller/user contract
Evidence: observed code/data/reproduction; distinguish inference
Impact: consumer-visible failure or privacy/ownership breach
Correction: minimum required behavior, not a prescribed unrelated redesign
Verification: concrete scenario/check that distinguishes fixed from broken
```

A clean review means no evidence-backed findings were identified within the reviewed scope. It is not a security certification or proof of unexamined native behavior. Missing evidence is reported explicitly, not converted into an approval.

## Bounded feedback loop

1. Obtain one independent review after coherent integration. The integration owner decides each finding's disposition using code and acceptance criteria: fix, disprove with evidence, or escalate a real intent/safety decision. Do not silently ignore a blocking finding.
2. The implementer makes the necessary corrections and runs the distinguishing scenario plus the affected shared checks. Record the actual command/outcome and the finding it resolves.
3. Request at most one focused re-review of unresolved findings and corrected paths. Avoid expanding scope or iterating until reviewers merely agree.
4. After two review rounds, unresolved blocking disagreements require a concrete human decision: identify the contract, opposing evidence and safe alternatives. Finish all reachable work; do not merge, claim completion or weaken gates while blocked. Nonblocking findings are dispositioned explicitly rather than left as implied follow-up work.

Only the integration owner delivers the final result. Maintainer scope approval, staging, committing and merging retain their existing human approval boundaries. Protected Auto-K promotions require approval only for explicitly authorized private Auto-K work; public reviews need no private graph access. Agent reviews never override deterministic failing checks or fabricate actual observations.
