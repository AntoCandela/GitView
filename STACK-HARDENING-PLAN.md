# GitView stack hardening execution plan

Preserve React, Rust and Tauri while making subsystem ownership navigable and strengthening contracts, recovery, verification and measured performance. Group code that changes together; keep app composition above features and native hosting above repository services. Do not impose generic components/hooks/services tiers, a new router, new Rust crates or a framework rewrite. Execute through bounded subagent slices with one integration owner. This document authorizes no implementation by itself: source moves and hardening require maintainer execution approval. Markdown is the source of truth; regenerate STACK-HARDENING-PLAN.html after progress updates. Commands run from the repository root. Milestone 3 names current-to-proposed moves; later milestones use those proposed destination paths and cannot start until that handoff is complete.

## Status

- 2026-10-02 — Plan drafted against committed revision 031d907531b55b61d1be653b273fee2c60543a88. Existing modifications to DESIGN-RULES.md, src/features/changes/changes.scss and src/ui/FileExplorer.tsx have unknown ownership and are outside this planning task. No managed worker registry was found; this cannot detect unregistered sessions.
- 2026-10-02 — Prior read-only assessment ran npm ls --depth=0 and npm run policy successfully. node scripts/check-licenses.mjs --check reported no drift but refused publication clearance for @napi-rs/lzma-linux-x64-gnu@1.5.1, stackback@0.0.2 and shiki-embedded-glsl. These are historical observations, not current execution evidence. No advisory scan, performance benchmark or full verification run was performed for that review.
- 2026-10-02 — Implementation has not started; optional dependency adoption requires an explicit recorded decision. Independent frontend and native/tooling reviewers found no remaining defects in the original hardening plan. No application tests, full verifier, advisory scan or native smoke ran for this documentation-only task.
- 2026-10-02 — Later inspection found additional unowned changes across browsing/native authority, DTOs/adapter, history/tree rendering and tests, plus new browsing_authority.rs, useRepositoryFiles.ts, changeTreeRows.ts and related tests. Owners remain unknown; this session edits only the two plan files. Execution must re-scope against an agreed handoff rather than assume those uncommitted changes are part of the baseline. Current tree windowing and serial directory loading are already present in these uncommitted files and must not be planned as missing features.
- 2026-10-02 — The skill renderer generated STACK-HARDENING-PLAN.html successfully. Chromium inspection at 1440×900 confirmed 0/112 progress, pending milestones and working section navigation with no page errors. The renderer's narrow-width navigation overlaps content at 720px; reported to tool QA and left outside this repository task. Use Markdown or the desktop-width tracker; HTML is a derived view, not an implementation-evidence claim.
- 2026-10-02 — Added the behavior-preserving organization milestone and migrated later hardening ownership paths. Independent React and native reviews are reconciled; the native correction explicitly preserves the admission regression's subprocess test identity and requires child completion evidence. The renderer regenerated successfully; Chromium confirmed 14 milestones, 135 unchecked tasks, section navigation and expandable organization checklist with no page errors. Only the two plan files changed in this task; no source migration, application test or native smoke has run.
- 2026-10-02 — Maintainer authorized review followed by milestones 2 and 3, and explicitly selected takeover of the current checkout after other editors finished. The integration owner preserves all existing uncommitted changes on 031d907531b55b61d1be653b273fee2c60543a88; no automatic staging, commits or independent-worker baseline is implied. Remaining hardening milestones are not part of this execution.
- 2026-10-02 — CurrentNativeReview and CurrentSurfaceReview found no evidence-backed defects. CurrentTreeReview identified TREE-01: same-repository refresh unmounts the paged tree and loses its reading anchor. The pre-organization shared verifier passed all 15 checks. A controlled reproduction then failed three refresh scenarios (explicit refresh, ready revision and retained-listing failure) while eight related scenarios passed; correction remains in progress.
- 2026-10-02 — Collaborative batch: RefreshAnchorFix owns repository listing presentation/hook and focused refresh regressions; BrowserFoundation owns the real-service browser installer/CLI, NativeJourney lifecycle adaptation and fixed native fixture extensions. The owner retains shared manifests, policy, documentation and all verification. Both workers preserve the reviewed dirty baseline and skip checks; the owner controls failing-before/passing-after checkpoints. @playwright/test 1.63.0 and its Chromium runtime are installed.
- 2026-10-02 — TREE-01 correction passed 21 focused renderer scenarios. Manual real-service Chromium inspection retained scrollTop 20637 and the selected late-page TypeScript source across refresh; fixture safety and service teardown passed. The production build, finite baseline smoke and complete organization smoke passed before source moves, including working/staged/committed/browsed text, real worker assets, preferences, layout and refresh continuity. A smoke assertion was corrected to compare source rows independently of line-number gutters.
- 2026-10-02 — Organization batch uses the complete current-to-proposed move manifest in milestone 3 below. NativeOrganization owns src-tauri/src and affected src-tauri/tests, including the handed-back fixed browser fixture. ReactOrganization owns src and affected frontend tests plus the icon build-plugin import path; both retain the reviewed refresh correction. Public consumers use curated feature index.ts exports and the deliberate native run entry; native consumers move to workspace::persistence, git::process, git::status and diff::committed. Parent owns policy, manifests, license inventory inputs, browser CLI, documentation and all checks. No language server is configured, so workers use structural/reference-aware migration without LSP; no staging or compatibility aliases.
- 2026-10-02 — Native and React organization landed at the paths below; native, renderer, policy and browser reviewers report no remaining actionable findings. TREE-01 now retains the displayed listing and reading anchor until atomic replacement. Policy bypass regressions pass; smoke-runner interruption reaps compiler descendants, and injected refresh failure no longer passes as a successful replacement. Migrated production baseline/organization smoke and visual Chromium checks passed with fixture safety and teardown. Full native library execution passed 187 tests; the preceding shared run passed 14/15 checks but reported one history-IPC failure that did not reproduce in the unchanged 157-test integration or 187-test library runs. Its cause remains unestablished; final consolidated verification is pending.
- 2026-10-03 — Milestones 2 and 3 are complete. npm exec -- vitest run tests/unit tests/integration passed 297 tests; cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib passed 187 tests; npm run verify -- --output .verification/stack-hardening-handoff passed all 15 required checks, including every external Cargo target. Earlier failed manifests remain preserved; no timeout, concurrency or stack-size overrides were used. Browser baseline and organization scenarios passed against migrated production assets, with actual source text, worker loads, reading/selection continuity, fixture safety and complete teardown; Chromium visual inspection also covered highlighting, disabled-control tooltips and the narrow sidebar overlay. This is browser/service and mocked-host evidence, not packaged Tauri or native-picker certification.
- 2026-10-03 — The current dirty checkout is the reviewed handoff on baseline 031d907531b55b61d1be653b273fee2c60543a88; nothing was staged or committed. CODE-STYLE.md, DEVELOPMENT.md and docs/VERIFICATION-EVIDENCE.md describe the new owners. Legal inventory refresh/check reports no drift for 265 npm entries, 445 Cargo packages and 1,078 retained notice references; strict publication remains blocked by the existing lzma-linux-x64-gnu, stackback and original GLSL grant gaps. Later hardening milestones remain unimplemented and require their recorded decisions/authorization; the root tracker remains active for that unfinished plan.

## Milestones

### Milestone 1: Establish reproducible subagent execution contracts

Toc: Agent contracts

Goal: Establish ownership, sequencing and evidence rules that every subsequent milestone inherits.

Acceptance Criteria

- Before each batch, the Status section identifies the integration owner, exact reviewed baseline, participating agents, owned files, shared interfaces, prerequisites and verification owner; private checkout paths remain in local coordination messages.
- Every completed implementation milestone has named command outcomes, a real changed-path smoke observation, reconciled independent review and relevant documentation updates. Missing prerequisites remain blocked, not checked complete.
- No worker silently acquires shared manifests, another agent's source files, ports, caches or application data; no automatic commit, stash, merge or push occurs.

Checklist

- [x] Obtain maintainer authorization to execute this plan and record the authorized milestones in Status; retain decision-only scope for milestones 9, 13 and 14 until their decision tasks are accepted.
- [x] Re-run the gate in .agents/skills/gitview-session-coordination/SKILL.md; inspect checkout, shared Git common-directory registry and ownership markers without treating dirty files as evidence of another agent's identity.
- [x] Establish the selected independent, collaborative or dependent contract when concurrent work exists. Independent workers require explicit committed baselines; dependent workers wait for reviewed committed handoffs. Do not copy unfinished parent edits into workers.
- [x] Read DEVELOPMENT.md, CODE-STYLE.md and the task-delegation, worker-isolation, verification and independent-review skills. Frontend implementers also read DESIGN-RULES.md; instrumentation work also reads the SQL-diagnostics skill before accessing any authorized store.
- [x] Reserve package.json, package-lock.json, src-tauri/Cargo.toml, src-tauri/Cargo.lock, scripts/verify.mjs, .github/workflows/verification.yml, legal inventories, shared documentation and this plan for the integration owner. Workers submit exact proposed changes to those files; source-file overlap requires an explicit handoff.
- [ ] Complete milestone 2's real-browser fixture, then milestone 3's reviewed organization handoff before launching hardening slices for milestone 4 configuration work, milestone 6 renderer recovery and milestone 10 native parser properties. Keep milestone 4 source lint remediation with the integration owner after those slices land; shared manifest changes remain owner-only.
- [ ] Execute milestone 5 after the first hardening wave's dependency additions, then milestone 7 CSP and milestone 8 contracts sequentially because their host ownership intersects. Execute milestones 9 and 11 after contract integration. Execute milestones 12, 13 and 14 after their browser/native prerequisites. Repeat license/advisory checks after later dependency changes.
- [x] Give every subagent a self-contained contract containing milestone, exact baseline, owned existing/proposed files, preserved behavior, dependency handoff, concrete acceptance scenarios and prohibited changes. Independent substantial slices may run together; tiny cleanup stays with the owner.
- [x] Require workers to skip builds, linters, tests and formatters mid-flight. After coherent integration, the owner runs the milestone's named checks once and exercises the changed surface. Write regression cases before their fixes, but let the owner run the controlled failing-before/passing-after checkpoints.
- [x] For each batch, use a separate read-only reviewer to inspect actual changed code, callers and evidence. Resolve blocking findings before handoff; review does not replace checks. Record any accepted nonblocking limitations explicitly.
- [x] Run npm run verify -- --output .verification/stack-hardening after each integrated batch; preserve missing, failed and skipped evidence. Update DEVELOPMENT.md and relevant ownership/evidence documentation in the same behavioral handoff, without publishing raw logs or private repository data.
- [x] Record handoffs in Status with completed milestone IDs, owned changes, commands/outcomes, smoke boundary, unresolved blockers and the reviewed commit supplied by the maintainer. Regenerate the HTML with the design-docs-execution-plans skill's render_plan.py; only check tasks whose named evidence exists.

### Milestone 2: Provide the browser smoke prerequisite

Toc: Smoke prerequisite

Goal: Establish the minimal production-adapter browser fixture needed by renderer recovery before later geometry coverage is scheduled.

Acceptance Criteria

- The proposed npm run smoke:browser -- --scenario baseline command serves built frontend assets, opens a disposable real-service repository in Chromium and exits after fixture safety and subprocess teardown assertions.
- The browser bridge is test-only, installed before src/main.tsx runs and page-scoped; no arbitrary command, path input, production endpoint or runtime dependency is added.
- Milestone 3 uses this fixture for before/after organization smoke and milestone 6 consumes it for recovery; milestone 11 extends browser geometry coverage without creating a prerequisite cycle.

Checklist

- [x] Assign BrowserFoundation proposed tests/support/browserJourney.ts and scripts/browser-smoke.mjs, plus tests/support/nativeJourney.ts for lifecycle reuse. Existing uncommitted journey_bridge.rs changes require an agreed reviewed handoff before this work; do not overwrite them.
- [x] Have the owner add @playwright/test as a dev dependency and npm run smoke:browser mapped to node --experimental-strip-types scripts/browser-smoke.mjs for the Node 22 erasable-TypeScript support modules. Run npm exec -- playwright install chromium once in the owned environment; keep Playwright's later geometry suite in milestone 11.
- [x] Serve built dist through an owned loopback Vite preview server. Before main.tsx executes, install an invoke-compatible Playwright binding forwarding only allowlisted repository/diagnostic commands to that page's NativeJourney instance; substitute the native picker with fixed fixture selection, not arbitrary paths.
- [x] Preserve production RepositoryClient domain outcomes, rejected-promise behavior and best-effort diagnostic isolation. The bridge proves service semantics, not native host diagnostic capture or window authorization.
- [x] Implement the finite baseline scenario to open/select a fixture repository and display actual working text, then assert fixture_verify, close the browser, reap the service and stop the owned server even on failure.
- [x] Run npm run build and npm run smoke:browser -- --scenario baseline after integration; record commands and observed real text. Apply milestone 1 independent review/full verification before handing the prerequisite to Renderer.

### Milestone 3: Make subsystem ownership navigable without changing behavior

Toc: Subsystem layout

Goal: Put composition, feature sections and native subsystem internals in explicit homes before hardening workers edit them.

Acceptance Criteria

- src/main.tsx mounts app/Workspace.tsx; app/workbench/Workbench.tsx composes independent changes, history and diff features. Native lib.rs declares modules and exposes the supported run entry; host owns Tauri commands. No compatibility copies, stale imports, widened native authority or second state owner remain.
- Existing behavior suites and npm run policy pass at the migrated locations. Policy fixtures reject nested native Tauri leaks, UI-to-feature imports and cross-feature private imports; generated wire DTOs remain distinct from renderer-only presentation types.
- The same real-service browser organization scenario passes before and after moves, preserving selected content, Tree/List controls, layout/resize, preferences and worker syntax loading. Native host/service tests still exercise real command names, main-window rejection, serialized persistence and private parser cases; browser evidence does not certify packaged Tauri.

Checklist

- [x] Re-run the ownership gate and obtain an agreed baseline containing the current browsing-authority, serial-listing, virtualization and resize work. Assign NativeOrganization src-tauri/src plus affected src-tauri/tests, and ReactOrganization src plus affected frontend tests. Reserve scripts/check-policy.mjs, tests/infrastructure/policy.test.mjs, scripts/browser-smoke.mjs, shared manifests, documentation and this plan for the integration owner. Never move files still owned by another active slice.
- [x] Record the move manifest and supported import surfaces in Status before launching both independent slices together. Use language-server references for exported symbols when a server is configured; workers skip build/lint/test/format commands mid-flight. Do not interleave hardening behavior changes with moves, blanket formatting, filename-style changes or new dependencies.
- [x] Before source moves, have the owner extend scripts/browser-smoke.mjs with the finite organization scenario using milestone 2's production adapter and disposable real service. Select changed, committed and browsed working text, toggle Tree/List, resize/rearrange panes, change an appearance preference and observe worker-highlighted syntax. Add only fixed synthetic fixture cases in journey_fixture.rs/journey_bridge.rs needed for those observations, then run npm run build and npm run smoke:browser -- --scenario organization as the before checkpoint.
- [x] Extend tests/infrastructure/policy.test.mjs before policy changes with allowed and rejected imports at the proposed nested paths. Preserve contract/UI/platform boundaries; reject features importing src/app, cross-feature private paths and Tauri imports anywhere outside the native host. Keep workspace state free of filesystem/process execution while allowing its separately owned persistence child. Exercise actual checker results, not source-text snapshots.
- [x] Move workspace.rs to src-tauri/src/workspace/mod.rs and workspace_persistence.rs to workspace/persistence.rs. Move git.rs to git/mod.rs, process.rs to git/process.rs, status.rs to git/status/mod.rs and status_snapshot.rs to git/status/snapshot.rs. Preserve workspace's existing pure transition versus persistence boundary; group files without creating traits, wrappers or additional stores.
- [x] Move diff.rs to src-tauri/src/diff/mod.rs, diff_parser.rs to diff/parser.rs, diff_read.rs to diff/rooted_read.rs and committed_review.rs to diff/committed.rs. Move history.rs to history/mod.rs and history_git.rs to history/reader.rs. Keep inspection.rs separate because it owns both context/worktree authority and committed-file enumeration, not only graph rendering.
- [x] Move browsing.rs to src-tauri/src/browsing/mod.rs, browsing_walk.rs to browsing/walk.rs and browsing_authority.rs to browsing/authority.rs. Replace production sibling-file path overrides with normal child declarations; preserve target cfgs and descriptor-rooted authority. Retain the already cohesive diagnostics.rs plus diagnostics/ layout, application.rs, observation.rs and diagnostic_operation.rs rather than creating one-file folders or hiding high-level outcome mappings inside the low-level diagnostic store.
- [x] Extract native host implementation from lib.rs into proposed src-tauri/src/host/mod.rs with command dispatch in host/commands.rs and renderer metadata/terminal handling in host/renderer_diagnostics.rs. Keep picker, require_main_window, ipc_context and non-async traced_ipc on the host side; generate Tauri context exactly once. Expose run as the crate's deliberate entrypoint, not a legacy path alias. Register the existing tests/integration/host.rs as host::integration_tests from host/mod.rs; qualify sibling command macro paths and import crate::test_support explicitly, keeping handler/type visibility confined to host.
- [x] Migrate every Rust consumer to workspace::persistence, git::process, git::status and diff::committed, using the narrowest visibility that satisfies real callers. Preserve existing public subsystem APIs where the logical owner is unchanged. Update private test-only path registrations, the journey_bridge example, external Cargo test imports and executable documentation examples; retain autotests = false and existing target names. Do not leave forwarding root modules at obsolete paths.
- [x] Update the admission regression's string-based --exact child filter in src-tauri/tests/integration/host.rs to host::integration_tests::traced_native_admission_persists_choices_and_correlated_git_completion. Require a completion marker in a parent-owned temporary directory written only after the child's real admission, persistence, Git-preservation and diagnostic assertions; zero matched tests must not pass on exit status alone. Run cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib host::integration_tests::traced_native_admission_persists_choices_and_correlated_git_completion -- --exact after integration, without stack-size or concurrency overrides.
- [x] Move src/features/repositories/Workspace.tsx and WorkspaceSidebar.tsx to src/app/, together with workspace.scss and sidebarResize.scss. Keep useWorkspace in features/repositories as the repository-intent/snapshot coordinator exposed through a new curated index.ts; app owns screen composition and calls that API. There is no URL router in the current main.tsx bootstrap, so do not invent routes/.
- [x] Separate the two jobs currently in ChangedFiles.tsx: move review-mode/selection composition to proposed src/app/workbench/Workbench.tsx and extract the changed-tree/status presentation as features/changes/ChangedFileList.tsx. Put changedPathTreeFile in features/changes/changedPathTreeFile.ts, exported for repository browsing; keep useObservation and ObservationView in changes. Move ResizableWorkbench.tsx, WorkbenchLayoutMenu.tsx, workbenchLayout.ts, layoutMenu.scss and workbenchResize.scss to app/workbench/. Keep keys, generation guards, mounted panes and state lifetimes unchanged.
- [x] Move the renderer-only CommitReviewSelection declaration and comparisonSelection.ts callback contract to proposed features/diff/selection.ts, exported by diff/index.ts. History consumes that type-only public API instead of importing changes; diff may consume ObservationView from changes, but changes must no longer import diff or app. Do not put these presentation-only types in generated native DTOs.
- [x] Group repository-management files RepositoryBrowser.tsx, RepositoryList.tsx, RepositoryNameEditor.tsx and headLabel.ts in features/repositories/catalog/; group RepositoryFiles.tsx and useRepositoryFiles.ts in features/repositories/files/. Keep one feature because both sections share selected-repository lifecycle; expose only actual app consumers through its index.ts.
- [x] Group TextDiff.tsx and useSourceLayout.ts in features/diff/text/; group highlighting.ts, highlighting.worker.ts, useCodeHighlight.ts, languages.ts and codeThemeLoaders.ts in features/diff/highlighting/. Keep the three review adapters, useFileReview, controls and outcome labels at the feature root. Group HistoryGraph.tsx, layout.ts and colors.ts in features/history/graph/; keep ContextSelector, CommitFiles and useHistory with their existing feature owner. Leave the small appearance feature flat.
- [x] Group FileExplorer.tsx, ChangeTree.tsx and changeTreeRows.ts in src/ui/file-explorer/; group IconThemeProvider.tsx, TreeEntryIcon.tsx, iconThemes.ts, fileIconLookup.ts and fileIconThemes.d.ts in ui/file-icons/; group ResizeDivider.tsx, resizeGeometry.ts and panelLayout.ts in ui/resize/. Leave standalone generic controls and Brand at ui root. Preserve generic props and the ban on native contracts, feature fetching and platform imports in UI.
- [x] Have ReactOrganization migrate every consumer and test import, including worker URL construction, literal grammar/theme imports, icon build-plugin/type paths and CSS entrypoints. Use unstaged file moves for intact files under the maintainer's takeover contract; extract only where ownership changes. Add curated public exports, not export-star barrels or compatibility re-exports; module-internal code imports its siblings rather than its own barrel. Leave all test bodies in the existing dedicated tests/unit and tests/integration roots.
- [x] Split changes.scss by actual selector ownership: workbench layout goes beside app/workbench, changed-state presentation stays in changes and reusable explorer rules belong to ui/file-explorer. Update src/style.scss to emit every block once in the existing cascade order; preserve selectors/tokens and visual behavior rather than combining the move with a redesign.
- [x] Have the integration owner update scripts/check-policy.mjs for the new native owner paths and frontend app boundary, including normalized import/export/dynamic-import paths and .ts/.tsx/index resolution for cross-feature API checks. Do not add a parallel enforcement framework. Run node --test tests/infrastructure/policy.test.mjs followed by npm run policy.
- [x] Run npm exec -- vitest run tests/unit tests/integration after integration. Retain existing ChangedFiles.test.tsx scenarios against Workbench and the extracted list instead of replacing them with file-layout assertions. Run cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib and each registered external target workspace, read_only_open, observation, persistence, diagnostics and diagnostic_operations; the full verifier also checks the bridge/example build.
- [x] Run npm run build and npm run smoke:browser -- --scenario organization against the migrated production bundle. Observe real text for all three review modes, selected-pane continuity during layout/resize, preserved preferences, actual grammar/theme/engine loads and inert source text; require fixture_verify and complete child/server teardown. Do not claim import compilation alone proves asset URLs or native behavior.
- [x] Update CODE-STYLE.md, DEVELOPMENT.md and the module-ownership section of docs/VERIFICATION-EVIDENCE.md with the new path/dependency map; update affected local skills/examples and run node .agents/scripts/check-docs.mjs. Keep src/contracts and src/platform locations stable for the later generated-contract cutover, with generated DTOs under contracts/generated and the client interface handwritten.
- [x] Close through milestone 1 independent review and full verification. Record exact migrated paths and reviewed handoff before launching recovery, CSP, contracts, property-test or performance workers; reconcile every later plan reference against that handoff. Native and React moves may execute in parallel under disjoint ownership, but those downstream hardening edits must not race them.

### Milestone 4: Enforce correctness-focused static analysis

Toc: Static analysis

Goal: Add essential ESLint and Clippy checks plus useful reproducible formatting without replacing the existing verifier; estimated effort small to medium.

Acceptance Criteria

- The shared verifier executes typed frontend linting, React Hooks rules, Clippy and rustfmt checks; a failing required tool cannot yield a passing sanitized manifest.
- Existing architecture-policy checks remain enabled, intentional promise handling remains explicit, and no blanket disables conceal new violations.
- The chosen Rust toolchain is recorded and shared by local development and CI; formatting-only changes are separate from semantic fixes.

Checklist

- [ ] Assign the Tooling agent proposed eslint.config.mjs and rust-toolchain.toml plus tests/infrastructure/verify.test.mjs and hooks.test.mjs. Reserve production-source lint fixes and all shared runner/manifest changes for the integration owner after milestone 6 and milestone 10 handoffs.
- [ ] Extend tests/infrastructure/verify.test.mjs to exercise failing analysis processes, missing evidence and successful aggregate status using the existing runner seams, not source-text assertions.
- [ ] Add ESLint, typescript-eslint and eslint-plugin-react-hooks through owner-controlled dependency changes. Enable recommended type-aware correctness checks, rules-of-hooks, exhaustive-deps and discriminated-union exhaustiveness; triage intentional effects against their lifecycle invariants.
- [ ] Add the proposed npm run lint command. Integrate it, cargo clippy --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --all-targets -- -D warnings, and cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check into scripts/verify.mjs with stable check IDs.
- [ ] Pin a currently supported Rust toolchain compatible with the locked application dependencies and ts-rs requirements. Update CI to consume that pin; do not leave a separate floating stable toolchain overriding it.
- [ ] Assess noUncheckedIndexedAccess and exactOptionalPropertyTypes with a throwaway compiler run; record diagnostic counts and concrete affected boundaries in Status. Keep these flags decision-only until their remediation is explicitly scoped; retain existing strict mode regardless.
- [ ] Run npm run lint, both named Cargo checks and node --test tests/infrastructure/verify.test.mjs tests/infrastructure/hooks.test.mjs after integration. Exercise an isolated deliberately failing analysis command through the runner and observe a nonpassing manifest; remove the throwaway scenario afterward.
- [ ] Update DEVELOPMENT.md with the new fixed commands and toolchain requirement; close this milestone through the milestone 1 review, real-surface smoke for any semantic lint fixes, and full-verifier contract.

### Milestone 5: Enforce dependency and publication policy

Toc: Dependency policy

Goal: Make essential advisory and publication evidence explicit while preserving existing legal provenance tooling; wiring effort small, remediation effort depends on findings.

Acceptance Criteria

- npm and Cargo advisory checks produce explicit pass, fail or unavailable outcomes; network/database failure cannot become success. npm high/critical advisories block without exemptions; Cargo exceptions require reviewed applicability, a named owner and enforced expiry rather than automatic forced upgrades.
- The publication gate runs node scripts/check-licenses.mjs --check and remains blocked by unresolved legal evidence. Inventory consistency is never reported as publication clearance.
- The existing asset notices, archive identities and bundled legal resources remain authoritative; cargo-deny supplements them rather than replacing them.

Checklist

- [ ] Assign the Dependency agent proposed src-tauri/deny.toml, scripts/check-advisories.mjs and tests/infrastructure/advisories.test.mjs, plus tests/infrastructure/verify.test.mjs and agent-report.test.mjs after milestone 4 handoff. The owner controls dependency updates, scripts/check-licenses.mjs, scripts/verify.mjs, .agents/scripts/verification-report.mjs, licenses/, THIRD_PARTY_NOTICES.md and CI wiring.
- [ ] Recheck current official guidance at https://rustsec.org/, https://embarkstudios.github.io/cargo-deny/checks/index.html and https://docs.npmjs.com/cli/v11/commands/npm-audit/; pin cargo-deny as a tool, not an application runtime dependency.
- [ ] Implement bounded adapters in proposed scripts/check-advisories.mjs for npm audit --json --audit-level=high and cargo deny --format json --manifest-path src-tauri/Cargo.toml check advisories sources. Use fixed ecosystem arguments, explicit valid-success/advisory-failure/advisory-unavailable outcomes, bounded captured output and sanitized aggregate metadata; treat spawn/network/database/malformed-output failures as unavailable, never clean.
- [ ] Add tests/infrastructure/advisories.test.mjs scenarios for each outcome and expired Cargo exceptions before implementing the adapter. npm advisories have no gate-bypass exception; lower-severity findings remain triage output. Record registry metadata submission and the accepted threshold in DEVELOPMENT.md.
- [ ] Extend scripts/verify.mjs and .agents/scripts/verification-report.mjs together for the new suites, fixed advisory outcome codes, reporter summaries and rerun reconstruction. Extend tests/infrastructure/verify.test.mjs and agent-report.test.mjs to reject missing audit evidence, unknown codes and false clean reports.
- [ ] Trace @napi-rs/lzma-linux-x64-gnu@1.5.1, stackback@0.0.2 and shiki-embedded-glsl through licenses/inventory.json, licenses/asset-provenance.json and THIRD_PARTY_NOTICES.md. Produce authoritative grant evidence or a maintainer-approved removal/replacement proposal; never relabel unresolved evidence as cleared.
- [ ] Define suite membership explicitly: unit and integration retain their scopes; all remains repository-local and excludes online advisory/publication checks; advisories contains the two online adapters; publication contains all plus advisories plus the full license check. Map proposed npm run verify:advisories to node scripts/verify.mjs --suite advisories and npm run verify:publication to node scripts/verify.mjs --suite publication. Add CLI/suite-selection regression cases.
- [ ] Keep the current platform-matrix CI invocation of all and add a separately required online advisories invocation with its own sanitized output directory. Run publication for an explicitly requested release-readiness assessment; unresolved grants must not block ordinary development verification or be hidden from publication.
- [ ] Run npm run verify:advisories, node scripts/check-licenses.mjs --check, node --test tests/infrastructure/advisories.test.mjs tests/infrastructure/verify.test.mjs tests/infrastructure/agent-report.test.mjs and npm run verify:publication after integration. Record exact blocker outcomes without rerunning failures merely to reconfirm them; run node .agents/scripts/check-docs.mjs after updating agent-facing verification contracts.
- [ ] Update DEVELOPMENT.md and THIRD_PARTY_NOTICES.md with the actual gate and dispositions. Refresh inventory only after legitimate source/dependency changes; preserve the milestone as blocked until its publication acceptance criteria are met.

### Milestone 6: Contain renderer failures and prove worker recovery

Toc: Renderer recovery

Goal: Add essential render-error containment and useful worker/preference boundary coverage using existing React, Vitest and Testing Library; estimated effort small to medium.

Acceptance Criteria

- A review render/layout-effect failure produces sanitized scoped recovery UI while repository navigation remains available; a separate last-resort root fallback exists. Async/domain failures retain their existing distinct handling.
- Worker deadline, obsolete reply, worker error, last-consumer unmount and StrictMode replay scenarios assert visible plain-text fallback, correct latest highlighting and cleanup, not just mocked forwarding.
- Corrupt, wrong-shape and unavailable preference storage preserves valid safe defaults and session behavior; no Zod or global store is required for the current scalar settings.

Checklist

- [ ] Assign the Renderer agent src/main.tsx, src/app/Workspace.tsx, src/app/workbench/Workbench.tsx, src/features/diff/highlighting/useCodeHighlight.ts and proposed src/ui/RecoveryBoundary.tsx, tests/unit/RecoveryBoundary.test.tsx and tests/unit/useCodeHighlight.test.tsx after milestone 3. Include tests/unit/ReviewPreferences.test.tsx, tests/unit/TextDiff.test.tsx, tests/integration/Workspace.test.tsx and scripts/browser-smoke.mjs; resolve existing unowned edits before claiming files.
- [ ] Write regression scenarios in RecoveryBoundary.test.tsx for render/layout-effect errors and explicit retry. Extend tests/integration/Workspace.test.tsx to prove actual placement preserves repository navigation and session choices; raw exceptions, component stacks and private source must not enter persisted diagnostics.
- [ ] Place the scoped boundary around the Workbench/HistoryGraph subtree below Workspace navigation and IconThemeProvider, with retry and repository/selection-generation changes resetting only the failed subtree. Add the root fallback without remounting Workspace for normal scoped recovery; do not replace unavailable measurement in src/features/diff/text/useSourceLayout.ts with fabricated geometry.
- [ ] Write scheduler scenarios in the proposed useCodeHighlight.test.tsx using a controllable Worker and fake timers: coalesced newest work, obsolete/mismatched replies, the 15-second deadline, error/messageerror, cleanup and remount recovery. Preserve the existing real grammar tests in tests/unit/CodeHighlight.test.ts.
- [ ] Extend tests/unit/TextDiff.test.tsx to assert visible native text after worker failure and correct highlighted content after recovery. Keep scheduler assertions in the new hook test and real grammar assertions in CodeHighlight.test.ts.
- [ ] Extend tests/unit/ReviewPreferences.test.tsx with malformed JSON, unknown enum, wrong-shape and read-denied cases against src/features/appearance/reviewPreferences.ts. Record the provider-local icon preference lifetime difference without changing cross-window semantics in this milestone.
- [ ] Run npm exec -- vitest run tests/unit/RecoveryBoundary.test.tsx tests/unit/useCodeHighlight.test.tsx tests/unit/CodeHighlight.test.ts tests/unit/ReviewPreferences.test.tsx tests/unit/TextDiff.test.tsx tests/integration/Workspace.test.tsx after integration.
- [ ] Add the recovery scenario to the milestone 2 launcher and run npm run build followed by npm run smoke:browser -- --scenario recovery. Inject unavailable canvas measurement before mounting the review, observe usable repository controls, restore measurement and retry; trigger worker failure and observe readable native text. Keep injection in the test harness only and record browser-only evidence.
- [ ] Update DEVELOPMENT.md with recovery smoke expectations and close through milestone 1 review/full verification; keep react-error-boundary and broad thiserror migration deferred.

### Milestone 7: Restrict packaged frontend content loading

Toc: Content policy

Goal: Enable essential Tauri CSP protection without weakening existing command capabilities or breaking bundled assets; estimated effort small to medium.

Acceptance Criteria

- src-tauri/tauri.conf.json has an explicit production CSP admitting only required resources; Tauri main-window capability and require_main_window restrictions remain intact.
- The ordinary bundled app loads fonts, icons, theme/grammar chunks and the highlighting worker and accepts legitimate IPC. A separately identified native inspection artifact with identical production CSP rejects controlled prohibited loads; instrumented observations are not mislabeled ordinary-release evidence.
- Development-server exceptions are kept separate from production policy; missing native platform observations remain explicitly unverified.

Checklist

- [ ] Assign the CSP agent src-tauri/tauri.conf.json after milestone 5 configuration handoff. Reserve src-tauri/src/host/mod.rs setup with the integration owner; milestone 8 waits for its handoff. Read src-tauri/capabilities/main.json, src/features/diff/highlighting/highlighting.worker.ts, src/main.tsx and vite.config.ts without widening command permissions.
- [ ] Derive the policy from https://v2.tauri.app/security/csp/ and the emitted production worker/chunk/WASM/font URLs. Record the exact required directives before editing; do not start from wildcard hosts or unrestricted script execution.
- [ ] Configure production CSP and the narrowly necessary development policy; account explicitly for IPC schemes, worker loading and the chosen Shiki engine's requirements.
- [ ] Run npm run build and npm run tauri build in the owned native environment. Open a disposable repository in that artifact, select a highlighted file, switch themes and verify loaded fonts/icons plus successful IPC.
- [ ] Obtain approval for an inspection-only Cargo feature enabling native webview developer inspection, with no extra IPC capability. Build a separately named artifact with the same production CSP; install a securitypolicyviolation listener in its native webview, append a prohibited external script and issue a prohibited fetch to an owned test origin, then observe violation directives and absence of successful loads.
- [ ] Build the ordinary artifact without that feature and prove inspection bootstrap/features are excluded. Record both identities and the instrumentation boundary; a missing authorized native inspection mechanism blocks negative CSP evidence rather than allowing Chromium/Vite to substitute for it.
- [ ] Record platform, artifact hash, observed allowed/blocked loads and remaining platform gaps in Status and docs/VERIFICATION-EVIDENCE.md. Apply milestone 1 independent review before accepting the policy.

### Milestone 8: Generate renderer-safe contracts from Rust

Toc: Generated contracts

Goal: Replace duplicated wire DTO declarations with stable ts-rs generation while preserving existing commands, privacy and runtime semantics; useful high-priority work with medium effort.

Acceptance Criteria

- Generated DTOs match Serde field names, discriminants, nullability and current JSON-number representation; ordinary commands retain their behavior. Numeric-overflow rejection requires explicit approval as boundary strengthening, not a silent semantics-preserving migration.
- A deterministic check fails when Rust DTO changes are not reflected in generated TypeScript, without normal test execution rewriting tracked files.
- Production tracing, main-window authorization, normal domain outcomes, rejected-promise semantics and real-service journeys remain intact. After numeric-safety approval, oversized outgoing integers fail with a fixed transport error before serialization; revisions are never clamped, wrapped or reused to fit JavaScript.

Checklist

- [ ] Assign the Contract agent src/contracts/, src/platform/RepositoryClient.ts and DTO sections in src-tauri/src/workspace/mod.rs, workspace/persistence.rs, observation.rs, git/status/mod.rs, diff/mod.rs, history/mod.rs, inspection.rs, diff/committed.rs, browsing/mod.rs, diagnostics/types.rs and host/renderer_diagnostics.rs after CSP handoff. Include lib.rs export entry, host/commands.rs numeric boundaries, src-tauri/tests/integration/host.rs, tests/integration/NativeJourneys.test.tsx and tests/unit/RepositoryClient.test.ts; the owner controls shared manifest/runner edits.
- [ ] Recheck https://docs.rs/ts-rs/latest/ts_rs/ against the pinned toolchain and Serde attributes. Use ts-rs rather than the prerelease Tauri Specta line; keep generated bindings free of React, Tauri runtime imports and native filesystem identities.
- [ ] Define a contract-export Cargo feature and proposed src-tauri/examples/export_contracts.rs. Export through a feature-gated lib.rs entry and host-owned export helper so private RendererCommand/RendererPhase/RendererDiagnostic and transitive git/status enums are derived without making native internals public. Register the example's required feature so ordinary all-targets checks remain valid.
- [ ] Enumerate roots from actual Tauri arguments/results: WorkspaceSnapshot, OpenOutcome, SelectOutcome, MutationOutcome, ObservationSnapshot, ReviewCategory, ReviewResult, HistoryPageResult, ContextOptionsResult, CommitFilesResult, CommitReviewResult, RepositoryFilesRequest, RepositoryFilesResult, RepositoryFileResult, DiagnosticHealth and RendererDiagnostic. Reconcile the list against the agreed current host handoff; transitively include referenced wire types, not filesystem authority or SQLite-only records.
- [ ] Add proposed scripts/check-contracts.mjs and npm run contracts:check to compare temporary generation with proposed src/contracts/generated/ without modifying tracked output during checks. Document the explicit regeneration command; ordinary tests must not rewrite bindings.
- [ ] Inventory exposed 64-bit integers, including WorkspaceSnapshot.revision, all ObservationSnapshot.observation_revision variants, review_file's observation_revision input, DiagnosticHealth.accepted/written/dropped and RendererDiagnostic.duration_ms. Retain ts-rs number overrides and the stricter existing 86,400,000-ms duration limit; exclude SQLite-only counters/timestamps.
- [ ] Obtain approval for numeric safety before implementing it: outgoing DTOs containing values above 9,007,199,254,740,991 return fixed native transport error numeric_wire_overflow before serialization; unsafe incoming revision authority is rejected before dispatch. Keep native revisions monotonic without clamping, saturation or token reuse and retain existing diagnostic-metadata errors for invalid duration. A declined strengthening remains an explicit blocker/known precision limitation, not a falsely completed safety claim.
- [ ] Add max/max-plus-one cases in src-tauri/tests/integration/host.rs for each exposed numeric family, nested snapshots and input authority, plus tag renames, nullable options and every result family. Assert actual accepted values/rejections and no service dispatch for invalid authority; retain consumer assertions in the assigned frontend tests rather than generated-source snapshots.
- [ ] Derive/export the DTOs, migrate every frontend consumer and delete superseded handwritten wire declarations. Preserve semantic comments and the RepositoryClient interface; keep command argument wiring explicitly reviewed because ts-rs does not generate it.
- [ ] Run npm run contracts:check, npm run build, npm exec -- vitest run tests/unit/RepositoryClient.test.ts tests/integration/NativeJourneys.test.tsx and cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib after integration.
- [ ] Exercise a disposable real native open, selection, working-file review and history pagination through the actual adapter. Prove a controlled unexported DTO change fails contracts:check in a disposable fixture; remove that fixture. Document generation ownership in CODE-STYLE.md and DEVELOPMENT.md and close through milestone 1.

### Milestone 9: Decide targeted runtime validation without schema duplication

Toc: Validation decision

Goal: Produce an actionable decision on useful complex-boundary validation while retaining existing scalar preference checks; this milestone is decision-only with small assessment effort.

Acceptance Criteria

- Status records which runtime inputs are trusted co-shipped values, external/persisted data and opaque native authority; it distinguishes shape validation from authorization and compile-time generation.
- A Zod adoption decision names a concrete malformed-input behavior, one schema authority and measured representative-payload cost. No third independently handwritten Rust/TypeScript/Zod contract is approved implicitly.
- Keeping existing validation is an acceptable completed decision; package installation and production rollout remain separate explicitly authorized work.

Checklist

- [ ] Assign a read-only Validation agent src/platform/RepositoryClient.ts, src/contracts/, src/features/appearance/reviewPreferences.ts, appearancePreference.ts and src/ui/file-icons/IconThemeProvider.tsx after milestone 8. Deliver the boundary inventory and a single recommendation to the owner.
- [ ] Recheck https://zod.dev/basics and the current stable release. Keep benchmark packages/scripts in a disposable assessment directory, never application manifests. Compare parse/deep-clone and validation-only behavior on synthetic workspace/history/near-limit diff payloads; retain sanitized timings/shape categories and leave production rejection identity unchanged.
- [ ] Specify malformed-reply handling at the adapter as a sanitized transport/contract failure, not an empty successful repository result. Preserve original domain rejection semantics and best-effort diagnostics.
- [ ] Record a schema-source decision: retain generated DTOs with targeted external-data schemas; broad IPC validation remains deferred until a reviewed generation/compatibility mechanism eliminates independent schema drift.
- [ ] Keep current enum checks for scalar preferences. Record whether consistent icon-provider remount/storage-event behavior is desired; changing that behavior requires a named acceptance scenario rather than a global-store migration.
- [ ] Record adopt/defer and its rationale in Status. An adoption handoff must name files, schema ownership, invalid-payload cases, a proposed tests/unit/RepositoryClient.test.ts extension, exact command npm exec -- vitest run tests/unit/RepositoryClient.test.ts and a real adapter smoke before implementation begins.

### Milestone 10: Strengthen native parser invariants with proptest

Toc: Parser properties

Goal: Add useful shrinking property tests to existing parser coverage without exposing production internals or replacing real-Git tests; estimated effort small to medium.

Acceptance Criteria

- Bounded generators assert exact path/parent preservation, hunk-count consistency and whole-response rejection for malformed frames; tests do more than assert no panic.
- Failures are reproducible from fixed seeds and retained minimized regressions, with bounded case counts and no shared global environment mutations.
- Existing native unit module registration and real-Git integration coverage remain intact; Cargo automatic integration discovery stays disabled unless separately justified.

Checklist

- [ ] Assign the Parser agent src-tauri/tests/unit/status.rs, history.rs and diff.rs. Read their owning parsers in src-tauri/src/git/status/mod.rs, history/reader.rs and diff/parser.rs without changing production behavior; the owner adds proptest as a dev dependency.
- [ ] Recheck https://docs.rs/crate/proptest/latest, including its stated passive-maintenance status and toolchain requirements. Keep dedicated cargo-fuzz targets deferred until property coverage identifies a distinct need.
- [ ] Replace redundant bounded enumeration only where shrinking adds value; preserve existing regressions. Generate valid records and deliberate truncation, duplicate, overflow and encoding mutations with known semantic expectations.
- [ ] Add fixed-seed property cases for exact status path/category preservation, ordered raw parents and batch framing, and diff hunk endpoint counts. Persist only synthetic minimized cases under the existing test ownership, never captured user repository contents.
- [ ] Run cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib unit_tests:: followed by cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib integration_tests:: after integration.
- [ ] Exercise a disposable Git repository containing unusual paths, staged/unstaged changes and a merge through real observation/history/review; confirm correct presentation and unchanged repository bytes. Document generator seeds/bounds in DEVELOPMENT.md and close through milestone 1.

### Milestone 11: Automate real-browser geometry and interaction smoke

Toc: Browser smoke

Goal: Add useful Playwright coverage for layout and interactions that jsdom cannot prove while retaining all existing service/component tests; estimated effort medium.

Acceptance Criteria

- Real Chromium and WebKit browser scenarios observe wrapping, sticky rows, scrolling, focus, pointer capture and asset loading at wide and narrow viewports; browser WebKit is not labeled packaged WKWebView evidence.
- Tests own disposable repositories, ports, app state and subprocess teardown; the production adapter still reaches the real RepositoryService through a test-only transport boundary.
- The existing shared verifier records browser scenario failures and missing/skipped evidence as nonpasses, with only sanitized summaries uploaded.

Checklist

- [ ] Assign the Browser agent proposed playwright.config.ts and tests/integration/browser/workbench.spec.ts; extend milestone 2's tests/support/browserJourney.ts after handoff. Include src-tauri/tests/support/journey_fixture.rs, journey_bridge.rs and tests/support/nativeJourney.ts for bounded fixture extensions; the owner integrates package, CI, tsconfig and runner changes.
- [ ] Reuse milestone 2's @playwright/test installation and add fixed npm run test:browser. Place .spec.ts cases under tests/integration/browser so existing Vitest .test.ts matching excludes them; deliberately include the new configuration in TypeScript checking.
- [ ] Extend tests/infrastructure/verify.test.mjs and agent-report.test.mjs for stable IDs browser_chromium and browser_webkit using Playwright JSON, including zero tests, skips, malformed results and failures. Update both scripts/verify.mjs and .agents/scripts/verification-report.mjs; keep screenshots/traces local and synthetic instead of uploading raw reports.
- [ ] Extend journey_fixture.rs with fixed scenarios containing nested sibling folders with enough rows to scroll, a 10,000-line recognized .ts source with late-line markers and short syntax-distinct content. Expose only fixed scenario selection through journey_bridge.rs, not caller paths, arbitrary content writes or commands; expand fixture_verify's expected-byte checks to every new file.
- [ ] Reuse built-dist loopback serving and the pre-main.tsx page-scoped invoke binding from milestone 2. Each page owns its NativeJourney; require fixture_verify and child/server/browser teardown on success and failure. Install locked browser binaries in CI and never reuse another session's port.
- [ ] Assert narrow/wide wrapping, sticky sibling-folder replacement, divider/junction drag outside its target, keyboard focus, late-row anchors and inert hostile text. For offline highlighting, select the recognized .ts fixture and assert grammar-distinct token styling plus actual successful grammar/theme/engine asset loads; plain text alone does not prove enhancement loaded.
- [ ] Run npm exec -- playwright install chromium webkit and npm run test:browser after integration. Observe the actual surface at 1440x900 and 720x760, including the built frontend rather than only Vite development assets.
- [ ] Add the browser check to the shared integration/full verification flow and update DEVELOPMENT.md with its prerequisites and evidence limits. Apply milestone 1 review/full verification; do not remove manual packaged-native requirements.

### Milestone 12: Establish performance evidence before optimization

Toc: Performance evidence

Goal: Produce useful reproducible native/renderer baselines and explicit optimization decisions, not speculative dependency migrations; estimated measurement effort small to medium.

Acceptance Criteria

- Local evidence records fixture size, runtime/build mode, warm/cold repetitions, latency distribution, memory/resource observations and measurement limitations for each named workload.
- Decisions identify measured bottlenecks and preserve native authority, read-only behavior, source-retention bounds and keyboard/reading continuity.
- Unmeasured improvements are not claimed; optional virtualization, Criterion, notify and backend changes remain separately authorized scope.

Checklist

- [ ] Assign the Performance agent an owned disposable fixture and local .verification/stack-performance output; read src-tauri/src/observation.rs, git/status/snapshot.rs, history/reader.rs, src/ui/file-explorer/ChangeTree.tsx, src/features/history/graph/HistoryGraph.tsx and src/features/diff/text/useSourceLayout.ts. No production edits belong to this measurement slice.
- [ ] Before measurement, author an owned temporary .verification/stack-performance/protocol.json and run.mjs. Record the exact command node .verification/stack-performance/run.mjs --protocol .verification/stack-performance/protocol.json, selected service/browser entrypoints, fixture generator, per-platform collector commands, teardown and bounded result schema in Status. The harness is measurement-only and must not add production instrumentation.
- [ ] Set the protocol to synthetic indices of 1,000, 10,000 and 100,000 files, a 60-second idle observation, 500 annotated tags, 1,000 loaded history commits and diff sizes up to the existing 32,768-line cap. Record intentional resource-limit outcomes instead of increasing limits. Use three warmups and ten measured repetitions; distinguish fresh-process runs from genuinely cold filesystem caches and label the small sample's statistical limits.
- [ ] Name collectors before execution: existing operation durations/subprocess facts only where an authorized sink actually captures them; operating-system process/I/O collectors for native memory and temporary I/O; Playwright/Chromium performance traces plus DOM counts for renderer work. The journey bridge does not itself provide native diagnostic capture. Record unsupported metrics as unavailable and do not base an optimization claim on them.
- [ ] Measure native idle scans, a large index with few changes, annotated-tag-heavy history, and near-limit parsing; record subprocess count, elapsed time, temporary I/O and process memory using synthetic repositories and authorized diagnostics only.
- [ ] Measure renderer main-thread/layout time and mounted DOM for expanded file trees, accumulated history and near-limit diff rows during scroll, wrap/theme changes and panel resizing. Use the milestone 11 browser fixture and actual font loading.
- [ ] Inspect rapid-refresh listing work at the agreed baseline, including src/features/repositories/files/useRepositoryFiles.ts after the organization handoff. Measure its existing serial page pump and actual native overlap; do not add a second coalescing mechanism merely because the original review predates this hook.
- [ ] Measure the existing TanStack-based src/ui/file-explorer/ChangeTree.tsx and changeTreeRows.ts before proposing tree work. Record remaining history/tree bottlenecks; reuse the installed virtualizer only for uncovered needs and preserve lane continuity, commit expansion, sticky ancestry and keyboard focus.
- [ ] Evaluate https://docs.rs/crate/criterion/latest for pure parser baselines and https://docs.rs/notify/latest/notify/ for polling replacement. Watchers remain invalidation hints with overflow/deletion/network-filesystem recovery; they cannot replace identity checks or Git status authority.
- [ ] Record a keep/adopt decision with before/after measurement design for each candidate. Keep Git CLI over git2/gix until a measured bottleneck and a differential compatibility fixture justify a separately approved large migration; no savings are inferred from library choice alone.

### Milestone 13: Define native automation and Windows parity acceptance

Toc: Native platform gate

Goal: Produce a useful executable native-testing/parity decision without confusing builds, mocked IPC, instrumented apps and release artifacts; assessment effort medium, Windows implementation effort unresolved.

Acceptance Criteria

- docs/VERIFICATION-EVIDENCE.md contains a feature-by-platform parity matrix for macos-arm64, windows-x64 and ubuntu-24.04-x64, including directory listing, worktree-byte review and existing preview scenarios, with named evidence or explicit blockers.
- A native-automation decision identifies the real command path, test-only instrumentation, production exclusion proof and platform prerequisites. No automation plugin or capability is silently added to shipped builds.
- Windows parity implementation and broader release certification remain blocked until their scope, environment and acceptance fixtures are explicitly approved; decision completion is not feature completion.

Checklist

- [ ] Assign the NativePlatform agent src-tauri/src/diff/rooted_read.rs, browsing/mod.rs, browsing/walk.rs, src-tauri/tests/integration/browsing.rs, diff.rs, host.rs and scripts/preview-evidence.mjs for read-only assessment; do not assume Windows CI build success proves worktree file previews.
- [ ] Record the non-Unix UnsupportedReason::Other return in diff/rooted_read.rs and the unsupported directory-browsing branch in browsing/mod.rs. Produce the Windows parity matrix in docs/VERIFICATION-EVIDENCE.md covering listing and ordinary/Unicode files, symlinks/junctions/reparse points, identity replacement races, nested repositories and stale authority.
- [ ] Obtain maintainer selection between full Windows directory-browsing/worktree-review parity and explicitly retained limited support. Implementation requires a real Windows worker and a reviewed rooted/no-follow I/O design; no generic path-based fallback is permitted.
- [ ] Evaluate current https://v2.tauri.app/develop/tests/webdriver/ and https://webdriver.io/docs/desktop-testing/tauri/plugin-setup. Distinguish WebdriverIO embedded support on all three platforms from direct tauri-driver support on Windows/Linux; document the upstream latest-only maintenance policy.
- [ ] Produce an automation proposal using real native IPC for repository/read-only/secondary-window scenarios. Proposed test-only features, capability overrides and frontend bootstrap must be absent from ordinary release artifacts; raw log forwarding stays disabled unless its privacy contract is explicitly approved.
- [ ] Record a concrete authorized pilot contract before installing WDIO tooling: owned files, proposed fixed command npm run test:native-browser, artifact/build selection, teardown and production-exclusion assertions. Retain the native folder-picker and human observation requirements not exercised by the chosen driver.
- [ ] For an authorized native acceptance run, build with npm run tauri build and assess the actual artifact using npm run preview:evidence -- --artifact "$ARTIFACT" --platform "$PLATFORM" --report "$REPORT", where the local owner records the real artifact/report paths and one supported platform ID. Never synthesize human observed:true fields from browser results.
- [ ] Keep the parity matrix independent of preview-evidence.json: scripts/preview-evidence.mjs currently has no required directory-browsing or worktree-file-review scenario, so a validated preview report cannot prove those capabilities.
- [ ] Record per-platform passed, failed, blocked and not-run facts in Status and the maintainer-approved support documentation. Missing environments remain blockers; do not mark parity or release readiness complete merely because this assessment is complete.

### Milestone 14: Resolve remaining optional migrations and durability guarantees

Toc: Retain and qualify

Goal: Close every remaining review recommendation with a maintainable keep/defer/adopt decision and a concrete prerequisite for future changes; assessment effort small to medium.

Acceptance Criteria

- Status explicitly records retention of feature-owned state/loading, native typed errors, Git CLI and existing persistence libraries unless a new approved scope provides evidence for replacement.
- Workspace atomic replacement is distinguished from power-loss durability; the intended guarantee and its platform evidence are documented without claiming a simulated failure proves power-loss survival.
- No optional dependency is installed solely to complete the plan; approved follow-on implementation receives its own owned checklist and acceptance evidence before execution.

Checklist

- [ ] Assign a read-only Architecture agent src/features/repositories/useWorkspace.ts, src/features/changes/useObservation.ts, src/features/diff/useFileReview.ts, src/features/history/useHistory.ts, src-tauri/src/git/process.rs, workspace/persistence.rs and diagnostics/store.rs. Deliver code-backed decisions, not a redesign.
- [ ] Retain useState/feature hooks and small useSyncExternalStore preference stores rather than Zustand/Redux. Preserve native snapshot authority, revision filtering, serialized user intents and session-only preference behavior; a local reducer remains optional for demonstrated transition complexity.
- [ ] Defer TanStack Query until multiple independent consumers need the same native read. An adoption proposal must preserve client/context/generation/authority query keys, explicit history refresh, outdated-text labeling, bounded private-source caching and mutation order; use https://tanstack.com/query/latest/docs/framework/react/guides/important-defaults to account for retries and refetching.
- [ ] Retain sanitized typed domain outcomes and primary-versus-cleanup error precedence. Treat thiserror as optional localized boilerplate reduction; do not replace renderer contracts with anyhow strings or add a second tracing/logger pipeline.
- [ ] Retain rusqlite's dedicated bounded diagnostic writer, Serde workspace validation and tempfile replacement; do not introduce an ORM or Tauri store migration. Assess the existing missing parent-directory sync at the migrated src-tauri/src/workspace/persistence.rs owner.
- [ ] Obtain the maintainer's durability requirement. Preserve the current atomic-replacement guarantee unless power-loss durability is explicitly requested; a stronger guarantee requires platform-specific directory-entry durability analysis and a named fault/restart proof. Identify temporary.persist as the replacement commit point and define post-replacement directory-sync failure feedback, retry semantics and uncertainty before implementation.
- [ ] For an approved durability change, extend src-tauri/tests/integration/workspace_persistence.rs and application_persistence.rs before implementation. Assert old-byte preservation for pre-replacement failures separately from the approved post-replacement durability-uncertainty outcome; preserve application save serialization and error publication, without promising rollback after replacement.
- [ ] Run cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --test persistence plus cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target --lib integration_tests:: after the approved durability change. Exercise a real disposable save/restart; label power-loss behavior unverified until its separate proof exists.
- [ ] Record disposition and trigger for React Compiler, alternative worker abstractions, dedicated fuzzing and broader formatting tooling as deferred: no demonstrated gap currently warrants them. Do not replace existing Base UI, Shiki or specialized diff virtualization just for ecosystem popularity.
- [ ] Reconcile every accepted milestone and every decision-only outcome in Status; list blocked publication/native requirements separately from completed implementation. Archive the Markdown and regenerated HTML together under docs/plans only after the authorized execution scope has ended; never check blocked implementation tasks merely to reach 100 percent.
