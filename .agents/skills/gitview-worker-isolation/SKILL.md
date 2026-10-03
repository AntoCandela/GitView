---
name: gitview-worker-isolation
description: Use when preparing independent Git worktrees for concurrent GitView workers, running isolated web/native/verification modes, or safely removing an owned worker.
---

# GitView worker isolation

Use this repository's `.agents/scripts/worker.mjs`; never change global Git or agent configuration. A worktree separates source and runtime state, **not permissions or secrets**: it is not a security sandbox. Assign one integration owner to reconcile reviewed changes into the parent checkout. Workers do not automatically commit, merge, stash or copy files from the parent.

Complete the mandatory [session-start coordination gate](../gitview-session-coordination/SKILL.md) before preparing or running a worker. Choose an independent, collaborative or dependent path first; a small explicitly coordinated shared-checkout change does not require a new worktree.

## Prepare a committed baseline

Choose an explicit revision, an existing external parent directory, two **fresh** worker paths, and distinct nonprivileged ports. The tool reserves each port within this repository's worker registry; an unrelated listener still causes a strict runtime refusal rather than automatic port selection or process termination.

```sh
node .agents/scripts/worker.mjs prepare --root "$PWD" --path /tmp/gitview-worker-a --rev HEAD --port 15321
node .agents/scripts/worker.mjs prepare --root "$PWD" --path /tmp/gitview-worker-b --rev HEAD --port 15322
(cd /tmp/gitview-worker-a && npm ci)
(cd /tmp/gitview-worker-b && npm ci)
```

The checkouts are detached at the resolved commit. **Dirty or untracked parent edits are not included.** When the tool or requested change is itself uncommitted, invoke the tool from the parent, but do not pretend the worker contains that source. For a disposable integration exercise, build a separate temporary Git fixture and commit only that fixture's copied input; never auto-commit, stash or stage the user's checkout. Ask the integration owner to supply a reviewed baseline when necessary.

Install dependencies in each worker first with `npm ci`. Never reuse the parent's writable `node_modules` or native target for ordinary parallel development. No automatic dependency installation occurs in `run`.

## Run only fixed modes

```sh
node .agents/scripts/worker.mjs run --root "$PWD" --path /tmp/gitview-worker-a --mode web
node .agents/scripts/worker.mjs run --root "$PWD" --path /tmp/gitview-worker-b --mode native
node .agents/scripts/worker.mjs run --root "$PWD" --path /tmp/gitview-worker-a --mode verify
```

`web` runs Vite on the prepared `127.0.0.1` port with `--strictPort`. `native` owns that same Vite child plus Tauri development, using an absolute per-worker configuration path; it disables Tauri's usual before-dev shell command so the runner owns the entire child lifecycle. `verify` runs the existing verifier's complete suite, with evidence under `.verification/worker/evidence/`; it does not introduce another verifier.

The native `running` notification establishes owned launcher/Vite startup, not a compiled native application. Tauri dev can remain alive watching files after a compiler failure. Observe actual native SQLite/capture health before claiming native readiness; use the fixed recorded Tauri/Cargo invocation locally to diagnose compilation rather than treating a live port as proof.

Runtime cache, Cargo home/target, Vite optimizer cache and evidence are worker-local beneath ignored `.verification/worker/`. Temporary files use a fresh private per-run OS-temporary directory outside Git repositories, preventing temporary Git fixtures from discovering the worker checkout; an OS temporary root inside Git is refused. The runner records that directory privately and removes it after reaping children. Existing verifier native targets remain local to that worktree as well. The ownership marker is `.verification/worker/ownership.json`, checked against repository-local Git metadata, not merely trusted because a file exists. Each run holds an exclusive Git-admin lock. A live process-group supervisor pins every owned child group until the runner has propagated termination, allowed graceful shutdown, killed remaining owned group members and reaped its supervisors. No stale PID file or port lookup authorizes a kill. Stop the foreground command with Ctrl-C or SIGTERM and wait for it to finish before cleanup. POSIX lifecycle support is explicit; Windows runtime execution is refused rather than claiming equivalent child-tree guarantees.

### Real native storage isolation

The generated `.verification/worker/tauri.conf.json` overrides **Tauri's identifier**, not a made-up storage environment variable. Every worker gets `com.gitview.worker.w<random-id>`, while the packaged `com.gitview.app` identifier/configuration remain unchanged. Tauri resolves `app_data_dir()` with that identifier, isolating both `workspace.json` and `diagnostics/diagnostics.sqlite`:

- macOS: `~/Library/Application Support/<worker-identifier>/`
- Linux: `${XDG_DATA_HOME:-$HOME/.local/share}/<worker-identifier>/`
- Windows: `%APPDATA%/<worker-identifier>/` (configuration facts only; this runner refuses Windows lifecycle execution)

Linux native runs pin `XDG_DATA_HOME` to the prepared worker's recorded data root; a later shell environment cannot redirect SQLite/workspace files to an unmanaged directory. Relative `XDG_DATA_HOME` values are ignored according to the XDG absolute-path contract.

The local, ignored `.verification/worker/runtime.json` records the exact identifier, port, app-data directory and database path for inspection. Runtime paths and arbitrary child output are not included in public evidence. Native launch pre-creates a private app-data ownership marker `.gitview-worker-owner.json`; existing unowned or symlinked native data is refused. Observe a real native launch and newly created SQLite before claiming native isolation; browser serving and generated configuration alone are not native proof.

## Remove without discarding work

```sh
node .agents/scripts/worker.mjs remove --root "$PWD" --path /tmp/gitview-worker-a
node .agents/scripts/worker.mjs remove --root "$PWD" --path /tmp/gitview-worker-b
```

Removal refuses unowned paths, active/interrupted locks, tracked modifications, untracked source, and ignored files outside known generated artifact directories. It never passes `--force` to Git. Review/integrate or deliberately preserve worker source before retrying. Successful removal also deletes the unique native app-data directory **only when its ownership marker matches and the directory/marker are not symlinks**; otherwise it refuses rather than deleting user data. Runtime admission rejects nested symlinks throughout existing cache/target trees before launching children. Generated dependencies/build/cache/evidence within the owned checkout are disposable.

Generated `src-tauri/permissions/autogenerated/` files remain disposable when Git ignores their parent directory. Other ignored permission files under that parent still prevent removal; the parent is not a broader deletion allowance.

A crashed runner may leave a conservative Git-admin `gitview-worker-active` lock. There is no automatic stale-PID recovery or unsafe stop command. Inspect and stop the actual worker children, then remove only that lock manually once the worker is demonstrably inactive. Never use a port/PID from stale metadata to terminate an unrelated process.

## Focused verification

```sh
node --test tests/infrastructure/agent-worker.test.mjs
```

The boundary scenarios use disposable real Git repositories, actual Vite serving on distinct ports, busy unrelated listeners, dirty/untracked preservation, ownership refusals and interrupted locks. Native storage still requires the actual native observation described above. The integration owner runs repository-wide checks after all concurrent slices land, not during half-finished edits.
