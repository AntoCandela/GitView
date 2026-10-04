# Development workflow

Use this guide when developing, debugging or verifying GitView. It explains how to use the local diagnostics, test infrastructure, Git hooks and native-preview evidence together. Public contribution scope and acceptance criteria come from maintainer-approved issue/PR discussion; no Auto-K account or private planning access is required. Maintainers may use Auto-K for explicitly authorized private planning. This guide is an operational reference, not a parallel product specification.

Follow [CODE-STYLE.md](CODE-STYLE.md) for code and tests, [DESIGN-RULES.md](DESIGN-RULES.md) for frontend changes, [README.md](README.md) for platform prerequisites, and [module ownership](docs/VERIFICATION-EVIDENCE.md#module-ownership) for implementation boundaries.

Agents can use the repository-local [.agents toolkit](.agents/README.md): SQL diagnostics/instrumentation, verification, isolated workers, explicit delegation, bounded independent review, documentation maintenance and outcome-based agent evaluations. These adapt the existing native/verification APIs rather than introducing another logger or runner.

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes and [SECURITY.md](SECURITY.md) before reporting a vulnerability. Preserve upstream licensing and notices as described in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## 1. Start a development session

Coding agents must complete the [session-start coordination gate](.agents/skills/gitview-session-coordination/SKILL.md) before edits or runtime startup. Inspect the checkout and shared managed-worker registry, then choose independent committed-baseline work, explicit collaboration (including small disjoint changes in one checkout), or a reviewed committed prerequisite handoff when concurrent work exists. Unknown ownership stays unknown; these instructions do not install a session-launch hook.

From the repository root:

```sh
npm ci
npm run tauri dev
```

`npm ci` installs the locked frontend dependencies and installs the versioned hooks outside CI. If hook installation was skipped, run:

```sh
npm run hooks:install
```

The installer preserves custom `core.hooksPath` settings and active default hooks. If it reports existing ownership, resolve that with the owner; do not replace or bypass their hooks silently.

Use `npm run dev` for browser-only frontend work. It does not provide native Git execution, the folder picker or native SQLite capture; use `tauri dev` to exercise those boundaries.

For concurrent agents, use the [worker isolation](.agents/skills/gitview-worker-isolation/SKILL.md) and [delegation](.agents/skills/gitview-task-delegation/SKILL.md) skills. Worktrees start from an explicit committed revision, not the parent's uncommitted edits. Assign disjoint files and shared interfaces, reserve shared files for one integration owner, and keep worktree runtime data/ports separate. Never automatically stage, commit, stash or merge to transfer a prerequisite.

## 2. Choose verification by the changed boundary

| Command | Use |
| --- | --- |
| `npm run test:unit` | Frontend, native and infrastructure unit checks. |
| `npm run test:integration` | Frontend integration and native service/Git/filesystem/SQLite/host scenarios. |
| `npm run policy` | Test placement, architecture imports and protected-artifact rules. |
| `npm run smoke:browser -- --scenario baseline` | Built Chromium renderer, production adapter and disposable real native service; working-file endpoints and teardown. |
| `npm run smoke:browser -- --scenario organization` | The same real-service boundary plus all review modes, licensed embedded GLSL, worker assets, Tree/List, refresh anchors, preferences and layout continuity. |
| `npm run verify` | Both builds, both test categories, executable agent documentation, policy and strict publication-license/source provenance checks; the same required checks used by CI and pre-push. |

Run focused checks while working, then run full verification before handing off a change. For example:

```sh
npm exec -- vitest run tests/unit/RepositoryClient.test.ts
cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --test diagnostics
npm run verify
```

`tests/integration/NativeJourneys.test.tsx` uses the production frontend adapter against a disposable real `RepositoryService` subprocess. Its support driver builds the registered `journey_bridge` Cargo example once per test process in `.verification/native-target`, so this Vitest file requires the native build prerequisites and Git even when run directly. It owns its repositories, workspace file and subprocess cleanup. The substituted picker/transport does not prove native Tauri IPC, window authorization, host diagnostic capture or packaged-WebView behavior.

The browser smoke is an explicit changed-surface check, separate from `npm run verify`. It requires Node's erasable-TypeScript support (the command uses `--experimental-strip-types`), native build prerequisites and Playwright Chromium:

```sh
npm exec -- playwright install chromium
npm run build
npm run smoke:browser -- --scenario baseline
npm run smoke:browser -- --scenario organization
```

Each run serves `dist` through an owned Vite child on an ephemeral loopback port, installs a page-scoped allowlisted transport before bootstrap and creates disposable repositories through `tests/support/nativeJourney.ts`. Picker choices are fixed fixture choices, never arbitrary paths. The organization fixture adds fixed TypeScript content, C++/Ruby files with embedded GLSL and 768 root files. Runs assert exact source rows, real grammar/theme/engine loading, distinct shader syntax/comment colors, inert source, fixture safety and browser/service/server teardown; ignored `.verification/browser-smoke` holds sanitized summaries and synthetic screenshots. This proves neither native host diagnostic capture/window authorization nor a packaged WebView or native picker.

The runner owns SIGINT/SIGTERM before starting resources and cancels/reaps its private native build or startup when interrupted. Vite runs separately because its own signal handler exits its process. Refresh smoke requires a successful read under the replacement listing's authority, unchanged scroll position and no listing alert; retained old rows alone cannot prove a successful refresh.

The native verifier owns `.verification/native-target`, independently of Tauri development and packaging targets. Keep `--target-dir` in focused native reruns so they use the same verification artifacts.

The shared verifier isolates unit/integration child processes from inherited `GIT_*` overrides and global/system Git configuration. Repository-local configuration remains active, including intentional filter-rejection fixtures. This does not change the invoking shell or the application's fail-closed handling of unsupported user filters. Direct Cargo/Vitest commands inherit the shell environment; use the shared runner when validating fixture isolation.

Native deadline fixtures use a paused Tokio clock held by an outstanding blocking task, with real-time notifications for subprocess gates. They must not busy-spin while waiting for OS work: that competes with the child processes under parallel CI load. Explicit clock advancement still determines the operation timeout; the separate 15-second wall-clock stall guard remains unchanged.

Vitest 5 JSON reports are files, not stdout. The shared runner passes a unique private temporary `--outputFile`, limits each report read to 2 MiB, interprets only the complete JSON document and removes the directory after success, failure or cancellation. Diagnostics retain their separate existing output bound and cannot be mistaken for JSON. Only allowlisted summaries reach the manifest; missing, malformed, empty, skipped and TODO reports cannot certify a pass, and a process failure takes precedence. Direct `vitest --reporter=json` invocations instead use the ignored `.vitest/` directory; do not publish those raw reports.

TypeScript 7 owns compilation; its unstable compiler APIs are not used by policy or evidence collection. Both source consumers use the directly declared Rolldown public `parseSync` API through `scripts/source-ast.mjs`, preserving TypeScript import types, literal-only test titles and parentheses. Parse failures produce unavailable evidence rather than empty successful inventories. Workflow-evaluation bindings include this parser and the npm manifests so changing its source or selected implementation invalidates a prepared challenge.

A passing test suite is not a substitute for exercising the changed path. Launch the real application or CLI, reproduce the relevant action, and observe its outcome. Browser fixtures and Tauri MockRuntime can prove their respective boundaries, not native-picker interaction or packaged-platform readiness.

For file-panel changes, exercise the compact header and independently scrolling rows at wide and narrow widths. At ordinary pane widths, its height and control styling match the Git graph toolbar; at minimum widths the controls wrap rather than clip. The single icon-and-text view button toggles between **Tree** and **List** with mouse, Enter or Space. The count represents changed files in the ready snapshot, not directories, visible leaves or staged/unstaged categories; unavailable observations do not claim zero files. Tree is the initial view. The double-arrow folder button offers **Collapse all** when every current directory is expanded and **Expand all** whenever any directory is collapsed. Both actions include nested directories, preserve the selected comparison and leave root-level files visible. Removed directories must not affect the next action. List displays flat filename rows with full paths in accessible labels and hover tooltips. Switching views must preserve the selected comparison, status markers and collapsed directories; expansion state must not leak between repository contexts. The folder button is disabled in List view and when no directories are available.

For workbench-resize changes, exercise all six panel arrangements at wide and narrow widths. The connected T-junction has a 16×16px pointer target and a two-axis move cursor; dragging it resizes both splitters under their existing independent bounds, including after the pointer leaves the target. Release, pointer cancellation and lost capture end the drag. The junction supports arrow keys for individual axes, Shift for larger steps, and Home/End for both axes' limits. Away from the junction, the row and column dividers remain single-axis controls. Preserve mounted panes and file selection throughout resizing and layout changes.

Divider feedback stays line-only: hover and dragging tint the existing 1px strokes without filling the junction target or turning the splitter gutters into thick bars. Keyboard focus retains a thin visible outline; pointer dragging suppresses that outline until the drag ends.

The old/new divider retains its proportion for the session across file and review remounts. It shares a 6px center-snap zone with both workspace splitters and their junction: the two usable panes are equal after subtracting the 5px divider. While dragging at the midpoint, the existing stroke uses the accent color and a small perpendicular tick; leaving the snap zone or releasing clears the cue. Measure against the live comparison/workbench rectangle, not window dimensions: verify both ends of the row divider with the sidebar open (including an empty listing), wrapped controls, and any header/banner space above the workbench.

The repository-files sidebar resizes from 180–480px while reserving at least 320px for the desktop workbench. At the existing narrow-window overlay breakpoint it reserves 40px outside the sidebar and lowers the minimum only when necessary. Collapse/reopen retains its preferred width. **Reset layout**, immediately left of Appearance, restores the default panel arrangement and row height, opens the sidebar at its 280px preferred width, and sets both the bottom panels and source panes to 50/50 splits. Measure the bottom split against the final workbench width after restoring the sidebar, excluding the divider. Startup sizing and sidebar visibility are unchanged. Reset must not remount the preview or clear file/repository selection, reading preferences or themes.

All buttons, actionable icons, switches and folder/file rows use the shared tooltip rather than native `title` bubbles. Verify an opaque palette background above headers, menus and sticky rows, hover on disabled controls, and Escape dismissing both the tooltip and its enclosing menu. Folder tooltips aggregate descendant statuses independently of expansion; repository sidebar summaries use the full ready status observation, not only progressively loaded files. Unavailable observations must report unavailable status rather than claim no changes.

For history references, exercise short and long local/remote-tracking names (including non-origin remotes), tags and zero/one/many refs. The summary keeps one primary reference plus an exact count of all others at both narrow and wide widths. Verify HEAD/viewed/main precedence, truthful attached/detached/bare/no-ref HEAD context, grouped hover/focus previews, click-to-keep-open inventories without overlapping tooltips, Escape/outside/Tab dismissal and virtualized row focus. Refresh or switch context while open to discard stale names. Reference controls must not change commit expansion, comparison or Git state. Inspect every interface palette and confirm that full commit hashes appear in the concise commit tooltip, not among branch names. Reference groups, overflow counts, context and actions follow the selected interface language; actual names and hashes stay unchanged.

For localization, exercise all six languages in empty and active workspaces, at ordinary and narrow widths. Switch with an existing error/tooltip and selected file visible; verify live accessible-name changes, unchanged source/filenames/branch/commit data and retained focus/scroll. Test manual reload, System pending/commit, failed saves and both pointer/Space reactivation of the current manual language while System is pending. The old native reply must not replace that renewed manual intent. `node scripts/check-locales.mjs` runs the same catalog gate used by the frontend build and full verifier. Human linguistic review and actual native OS/picker/package observations remain separate requirements.

Reactivate checked System with pointer and Space to resolve native preferences again; reactivate a session-only manual or System choice to retry persistence. With a branch-picker card, reference inventory or Appearance disclosure tooltip visible, change locale and verify the complete action text updates while names, focus and expansion remain intact. The flat picker keeps upstream ordering, colored state icons and the distinction between viewing history and opening an existing worktree.

For wrapped source, scroll several visual lines into one long source row, then switch to Scroll or widen the pane. The same source line must still contain the viewport's leading edge in both panes; a now-invalid intra-row offset must not move the reader into later source lines.

For long file-tree scrolling, verify that each current parent folder stays sticky below its ancestors in 28px rows. A folder's own section bounds its sticky row, so sibling and top-level folders replace the previous chain when scrolling reaches the next section. Sticky folders remain clickable, use opaque backgrounds matching their pane and keep keyboard-focused rows visible below the chain. This behavior is scoped to the working-file Tree view; flat List rows and inline committed-file trees retain their existing scrolling. Check bounded mounted rows, sequential Tab/Shift+Tab across virtual gaps, collapse/expand, resize and refresh without losing the reading position. For history, check Home/End, crossing graph lanes, retained offscreen merge-parent/folder choices, and a large inline tree using the history scroller. Expand a middle commit, scroll beyond it and select a later commit: the old expansion must release its height without revisiting it.

For the repository-file sidebar, verify the leftmost expand/collapse button, centered repository selector and branch chooser remaining in the Git graph header. The sidebar contains files only; repository search/admission/rename/removal remain in the header disclosure. Exercise unchanged and ignored files as well as status-marked leaves, shared Tree/List and folder controls, icon/palette changes, collapse/reopen with preserved folder and preview selection, explicit refresh, and late listing/read replies after repository changes. Check partial counts, serial continuation, paused collapsed branches, empty directories and List-view completion. At narrow widths the sidebar overlays rather than squeezing the workbench; the repository disclosure stays inside the viewport. Validate native-issued listing/directory/cursor/file authority with real repository-service reads, including a directory beyond the former whole-list limits, retained earlier-page selections and an index larger than the ordinary Git-output budget.

For branch/worktree-picker changes, verify a flat list of all local branches, with the current worktree's branch first and marked by a checkmark. Branches checked out elsewhere use a worktree icon and navigate directly to that existing worktree on click or Enter; verify both working files and history switch only after native confirmation. Unchecked-out branches use a branch icon and change history only, preserving the active directory, HEAD, index, refs and working bytes. The current marker must remain distinct from the viewed branch. No row chevrons, nested groups or separate folder actions remain. Detached worktrees stay accessible below a divider with explicit detached labels; also preserve destinations whose local ref is unavailable or whose branch is checked out in multiple worktrees, preferring the current checkout for the branch row. Missing navigation capability disables navigation rows rather than falling back to history browsing. Search matches branch names and checkout labels without navigation; verify reopening, late context replies, Home/End text editing, arrow/Enter activation, Escape and outside dismissal. Check the 8px gap below search, equal horizontal insets, unclipped focus ring and semibold truncated names at narrow widths. Compare picker colors with the graph's same named local references in light/dark palettes; snapshot-absent and detached choices stay neutral. Bare repositories expose local branches and actual linked worktrees, never a synthetic current worktree. Browser fixtures prove renderer interaction, not native IPC or packaged-platform behavior.

Verify picker ordering by type (current branch, unchecked-out branches, branches checked out elsewhere), alphabetically within each type, including after filtering. Other-worktree rows keep current-checkout precedence, attached destinations before detached destinations, and alphabetical labels. Hover and keyboard-focus cards must expose full names, checkout state, labeled destination details and a separate action/effect section; unavailable navigation must say so. Check long names and card placement at narrow widths in light/dark palettes, and ensure the toolbar tooltip stays hidden while the picker is open.

On same-repository refresh, the current tree stays mounted while one replacement listing loads the root and currently expanded branches. Verify the leading native-segment row survives both multi-page and shorter replacements; old rows must keep their old listing authority until the atomic cutover. Collapsed branches do not delay replacement, errors preserve the displayed listing with partial status, and a changed client/repository/selection scope must discard previous presentation.

`npm run agents:docs` checks local toolkit links/frontmatter and compiles/runs the SQL skill's Rust examples against isolated real SQLite. Its standalone Cargo driver derives the SQLite requirement from the owning package and reapplies the registered, audited native path patches; a populated cache must not hide missing patch configuration. Workflow-evaluation source bindings include those vendored sources. Full verification includes this check; unit-only hooks do not run native documentation examples. [Independent review](.agents/skills/gitview-independent-review/SKILL.md) adds read-only findings and a bounded correction loop, never permission to override failed checks.

Use [workflow evaluations](.agents/skills/gitview-agent-evaluation/SKILL.md) to measure actual agent outcomes on disposable diagnostic, instrumentation and invalid-evidence challenges. Infrastructure tests exercise deterministic grader boundaries; they do not claim an agent/model ran in CI. Keep submitted code trusted or externally sandboxed: worktree isolation is not a security sandbox.

The full verifier's `publication_licenses` check validates the committed inventory, bundled notices, licensed grammar replacement and every vendored file against authenticated upstream archives. It requires network access to the recorded public package/asset sources; a fetch failure is not a pass. Run `node scripts/check-licenses.mjs --refresh` after changing covered inputs, review the resulting provenance/notices, then run `node scripts/check-licenses.mjs --check`. Cargo advisory scanners omit path dependencies, so a clean advisory result cannot replace this source-verification gate. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for local patch scope and redistribution limits.

### Put tests in the existing infrastructure

- Frontend unit/integration bodies: `tests/unit` and `tests/integration`.
- Frontend reusable support: `tests/support`.
- Verification-script tests: `tests/infrastructure`.
- Rust unit/integration bodies: `src-tauri/tests/unit` and `src-tauri/tests/integration`.
- Rust reusable support: `src-tauri/tests/support`.

Private Rust scenarios remain registered through their owning module's test-only `#[path]` declarations; external integration targets are registered in Cargo. Do not expose production internals merely to move tests. Reuse isolated temporary fixtures, never a user's repositories, workspace data or diagnostic store.

## 3. Diagnose a runtime failure

1. Obtain explicit authorization for the local diagnostic store you will read. Agent access to the repository does not authorize reading every application-data directory.
2. Reproduce one action in the native application and note its time. Prefer a disposable repository fixture when reproducing potentially disruptive behavior.
3. Inspect capture health. The main-webview `diagnostic_health` command, exposed through the platform adapter's `diagnosticHealth()` method, returns state, accepted/written/dropped counts and a fixed error code.
4. Query recent error and warning records using the CLI below.
5. Follow the returned operation and parent IDs across layers before attributing the failure.

### Locate and query the store

The native host creates `diagnostics/diagnostics.sqlite` under Tauri's per-user application-data directory. It is separate from `workspace.json`, observed repositories and browser storage.

On macOS, after authorizing this specific path:

```sh
DB="$HOME/Library/Application Support/com.gitview.app/diagnostics/diagnostics.sqlite"

cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" schema
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --level error --limit 20
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --level warn --limit 20
```

On Windows/Linux, use the application's actual per-user app-data location and your shell's variable syntax. Do not assume the macOS path or search unrelated users' stores.

The CLI opens the selected store read-only, validates its schema and does not create missing stores or repair incompatible ones. `--level` is an exact filter: querying `error` does not also include `warn`.

Copy a returned canonical UUID into `OPERATION_ID` to inspect that operation without the severity filter:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --locked --bin gitview-diagnostics -- --database "$DB" events --operation-id "$OPERATION_ID" --limit 200
```

Other filters are `--component`, `--event`, `--session-id` and `--since-ms` (nonnegative Unix milliseconds). Limits are 1–200. If `has_more` is true, the result is incomplete; narrow the time, session, operation or component instead of claiming you inspected the entire history.

### Interpret the evidence

- Request UUIDs connect renderer, IPC, application, Git and subprocess facts. Background work has its own IDs and parent links; query child operations separately when following a parent chain.
- Session IDs separate application runs. Records survive restart, but their presence does not establish that current capture is healthy.
- `started` without a terminal row is not proof of success or failure. Capture may be dropped or interrupted.
- `cancelled` and `superseded` are distinct from domain failure. Cleanup has separate completion/failure facts.
- A nonzero Git exit can be expected, such as an absent reference. Use the Git/application classification, not the process exit alone.
- Duration and byte counts describe timing/output size, not repository contents.

Capture is bounded, not lossless: the queue holds 1,024 entries and write-time retention is 20,000 events/seven days. Accepted records may still be awaiting commit. Degraded health, dropped counts or storage errors invalidate assumptions of complete capture even if the repository operation succeeded.

The writer commits at most 64 immediately queued records per full-synchronous transaction, without crossing a flush or shutdown barrier. Written counts advance only after commit; a failed batch rolls back its rows and retention changes and counts every consumed record as dropped. Batching does not increase the queue, retention limits or flush deadlines.

If the store is missing, verify that the native host ran and that the authorized path is correct. If schema/storage validation fails, preserve existing bytes and report the fixed code to the maintainer. Do not delete, migrate or replace the database automatically, and do not treat unavailable diagnostics as a successful investigation.

Native capture startup handles a compatible interrupted rollback journal separately: it locks the source, validates durable undo-record boundaries/checksums, recovers a bounded private snapshot, validates the exact approved schema and SQLite integrity, then allows SQLite to roll back the original. Invalid, unsafe or concurrently written originals are preserved. Schema/integrity checks alone cannot detect SQLite silently skipping truncated or checksum-corrupt undo records. The CLI never performs this recovery; a read-only `schema` error can indicate a pending hot journal, not necessarily a changed schema. Preserve both database and journal before any maintainer-directed investigation or repair.

### Keep instrumentation safe

Reuse `DiagnosticRecord`, `OperationContext`, `OperationTrace` and the existing closed vocabulary rather than adding another logger. The renderer uses the platform adapter; it must not gain SQL, arbitrary-message logging or caller-supplied Git execution.

Record only fixed codes, lifecycle facts, opaque IDs, durations, exit status and byte counts. Never persist repository names/paths, branch or file contents, process arguments/output, credentials, environment values, exception text or stack traces. Logging failures must not replace a domain result or change Git behavior. Preserve explicit parent scopes for spawned work and cancellation/cleanup ownership.

Keep `traced_ipc` a non-async constructor: box the operation future before the returned async state machine captures it. Boxing inside an `async fn` still embeds the concrete argument future and can multiply native admission stack frames. Use the existing native admission regression; do not hide the failure with larger worker stacks.

## 4. Diagnose a verification or hook failure

The runner writes a sanitized `manifest.json` to its output directory. Default verification uses `.verification/`; hooks use `.verification/pre-commit` and `.verification/pre-push`.

Inspect the failed check's ID, code, safe behavior identifiers, aggregate counts, revision/dirty state, runtime metadata and `rerun` command. Execute the supplied rerun locally for detailed output rather than attaching raw failure payloads to shared reports. An empty identifier list does not mean no failure occurred; compile errors or abnormal termination may have no framework test summary.

Missing, skipped, TODO or failed required checks cannot certify readiness. Diagnose the cause; do not turn a failed gate green by reducing parallelism, increasing stack limits, deleting meaningful tests or suppressing exceptions. Verification uses an isolated Cargo target, but target isolation alone does not establish the cause of a runtime failure.

### Hook behavior

- **Pre-commit:** policy plus unit verification; failure blocks the commit.
- **Pre-push:** full shared verification; failure blocks the push.

To exercise the installed gates without creating a commit or pushing:

```sh
git hook run pre-commit
git hook run pre-push
```

Hooks check the current worktree, not a reconstructed staged index or every pushed ref. Review partial staging separately. Hooks remain bypassable and do not replace CI, which checks the checked-out revision. They do not stage, stash, auto-fix or commit your changes.

## 5. Keep native-preview evidence separate

Build an actual unsigned artifact:

```sh
npm run tauri build
```

Record actual human observations for that exact artifact and platform, then assess the artifact-bound report:

```sh
npm run preview:evidence -- --artifact path/to/preview --platform macos-arm64 --report path/to/observation.json
```

The supported evidence platform IDs are `macos-arm64`, `windows-x64` and `ubuntu-24.04-x64`. [README.md](README.md#native-preview-evidence) describes the required report fields and scenarios; `scripts/preview-evidence.mjs` defines the validation contract.

The assessment hashes the artifact and rejects missing, blocked, failed or not-run required observations. Without a human report it remains unverified and exits nonzero. Never fill `observed: true` from a browser fixture, MockRuntime, compilation or another platform's result. A validated human report is not independent certification; signing, notarization and public-release readiness remain separate.

## 6. Hand off with evidence

Report the changed boundary, the checks and actual scenarios exercised, capture-health limitations, and anything still unverified in the public issue/PR or task conversation. Keep temporary data and generated evidence out of commits; stop only services you own. Only for explicitly authorized maintainer-private Auto-K work, attach implementation evidence to that task without promoting protected review statuses or changing product intent; never put private task identifiers or payloads in public handoffs.
