---
name: gitview-agent-evaluation
description: Prepare provider-independent disposable GitView agent workflow challenges and independently grade real submitted diagnosis, instrumentation and verification-report evidence. Use for evaluating agent behavior, not ordinary application tests or release readiness.
---

# GitView agent workflow evaluation

## Separate the runner from the agent

`.agents/scripts/evaluate-workflow.mjs` provides deterministic challenge preparation and independent grading. It never invokes a model, installs a provider, consumes credentials or pretends a mocked response is an agent. Infrastructure tests test grader boundaries only. An actual agent evaluation requires an operator to launch a real agent against a prepared handoff and retain its actual submissions.

Read `CODE-STYLE.md`, `.agents/evaluations/HANDOFF.md`, and the SQL diagnostics skill before instrumenting or querying. Reuse the real typed logger and verification reporter. Worktree isolation is not a security sandbox; candidate Rust is trusted local executable code, so review its provenance before grading.

## Prepare and hand off

From the repository root, with Rust/Cargo, locally available locked native dependencies, Git and Node available (no external SQLite executable is required):

```sh
node .agents/scripts/evaluate-workflow.mjs prepare --output .verification/agent-evaluation/run-01
```

The output directory must not exist. Preparation generates a private isolated store through the real executable documentation harness, a malformed verification manifest, a revision/source-bound challenge, buggy candidate instrumentation and `HANDOFF.md`. It authorizes only that isolated DB. It never discovers or reads user application data. Outputs must be fresh directories under ignored repository `.verification/` or an explicitly supplied absolute path under the canonical OS temporary directory; symlink escapes and arbitrary external/application-data destinations are rejected. `.verification/` is recommended for operator runs. Disposable OS-temp fixtures remain private and must not be committed.

Launch the provider/agent of your choice separately. Give it the prepared `HANDOFF.md` and run directory. Allow read-only repository access, read-only bounded queries to the authorized DB, the real reporter command, and writes only to `answer.json`, `record-save-failure.rs`, and `spawn-scan.rs` in the run directory. No repository/global edits, model credentials, graph mutations or automatic commits/merges are part of this workflow.

The exact closed answer schema is documented in the handoff. It binds the submission to `challenge.runId` and `challenge.revision`, contains queried failure/child facts, identifies the two fixed Rust candidate files implicitly, and captures the actual reporter exit code/JSON response. Success/readiness booleans and arbitrary messages/data are not accepted.

## Independently grade

```sh
node .agents/scripts/evaluate-workflow.mjs grade --run .verification/agent-evaluation/run-01
```

Grading checks run/revision/source binding and fixture integrity, independently queries actual fixed-schema SQLite records, compiles/runs submitted instrumentation with the real diagnostic types and store, then independently invokes the actual reporter. It rejects missing failure facts, incorrect parent/child correlation, unbounded/private fields, malformed captures and reported success without runtime proof. Changing evaluated sources/revision invalidates the run; prepare again rather than editing the challenge.

Exit 0 means all three bounded tasks passed, exit 1 means a scenario failed, and exit 2 means the run/arguments/runtime prerequisite was invalid. `grade.json` and CLI output contain only bounded scenario IDs, fixed outcome/error codes, run UUID and revision; raw child output, submitted private fields and compiler errors are not published. Temporary instrumentation builds/databases are cleaned after grading. The prepared DB and candidate submissions remain local until the operator removes the disposable run directory.

A passing result is **workflow-evaluation-not-release-readiness**. It does not prove general model reliability, full repository verification, native picker behavior, packaged-platform observations or release readiness. Report actual agent identity/run context separately only when genuinely observed; do not label deterministic CI grader tests as real agent/model runs.
