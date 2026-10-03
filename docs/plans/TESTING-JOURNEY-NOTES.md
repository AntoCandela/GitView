# GitView testing journeys

Proceed locally, step by step, using disjoint subagent work and one integration owner. The user explicitly deferred Auto-K and authorized this Markdown-led testing work. Preserve the current application's behavior and existing dirty changes; add missing consumer-visible coverage rather than duplicate tests. Repository/service, browser and packaged-native evidence remain separate. No claim of exhaustive input coverage or release certification is intended.

## Status

- 2026-10-02 — Discussion notes saved; Auto-K discovery failed without reading or mutating product items.
- 2026-10-02 — User deferred Auto-K. Local execution authorized; current baseline is ecf63412275509d02bc684b0f0f70b793c8e289c with substantial uncommitted application work. No managed workers were registered; unregistered sessions are not detectable from that registry.
- 2026-10-02 — Prior icon-tree checks and a 15/15 verifier pass are historical evidence only. A separate dark-syntax test failure remains unexplained. Previous restricted macOS launch did not prove native UI/picker/IPC; Windows/Linux native observations are unavailable.
- 2026-10-02 — Four implementation slices landed without production behavior changes: real-service transport/fixtures, seven real-Git UI journeys, five presentation cases, and seven bounded native invariant tests. Focused suites pass. Independent review found a secondary-worktree integrity blind spot; the fixture now checks its full tracked diff, and a negative mutation smoke detects changes in both secondary repositories.
- 2026-10-02 — First integrated manifest, .verification/testing-journeys/manifest.json: 14/15 checks passed; frontend build rejected three unsupported Testing Library query options. Corrected those test-only options. Final integrated manifest, .verification/testing-journeys-final/manifest.json: 15/15 required checks passed.
- 2026-10-02 — Chromium exercised all six arrangements at 1440×900 and 720×760, real Git content, inert hostile markup, external edits, persisted Full file/Wrap/explicit light syntax, session-layout reset, keyboard focus and pointer/keyboard resizing. A 10,000-line fixture retained line 9000 for 100 layout/reading-mode cycles in 26.1 seconds, with at most 48 mounted rows per pane. This is bounded repetition, not long-running native soak certification.
- 2026-10-02 — Accessibility scan: axe-core 4.13.0 reported one moderate best-practice finding, page-has-heading-one, in the selected-repository surface. Keyboard layout focus restoration and visible outline were observed; no native screen-reader result is claimed. License inventory has no drift after the test-only Cargo manifest fingerprint update; three existing publication-license blockers remain.
- 2026-10-02 — Local execution complete; archived Markdown/HTML under docs/plans. Built production assets rendered with all non-loopback network access denied: both Geist fonts, four file icons and the bundled highlighting worker loaded, with zero page errors. Both browser fixtures shut down cleanly. Native certification, long-running external fuzz/soak work and the recorded accessibility/publication findings remain separate unmet requirements.

## Milestones

### Milestone 1: Coverage contracts and isolated journey transport

Toc: Journey contracts

Goal: Exercise the production frontend invocation adapter against the real RepositoryService and Git, without native-picker or packaged-WebView claims.

Acceptance Criteria

- The seven journey topics below have named tests or explicit evidence gaps; repository browsing is included because the current checkout exposes that user surface.
- The test-only subprocess transport accepts fixed commands, owns disposable app/repository data, shuts down children and never accepts a user's repository path.
- New tests remain in the existing Vitest/Cargo infrastructure; no production API or extra runtime dependency is introduced for testing.

Checklist

- [x] Map existing frontend and native tests to repository management, working-file review, external changes, committed review, branches/worktrees, presentation and recovery.
- [x] Add the test-only Rust JSON-lines bridge at src-tauri/tests/support/journey_bridge.rs and register it as a Cargo example for integration fixtures.
- [x] Add subprocess support at tests/support/nativeJourney.ts, reusing the production RepositoryClient command names and native serialization.
- [x] Independently review the bridge isolation and lifecycle contract before accepting its evidence.

### Milestone 2: Repeatable user journeys

Toc: User journeys

Goal: Protect meaningful cross-layer workflows with real Git data and explicit failure/recovery assertions.

Acceptance Criteria

- UI interactions through the production adapter display actual native Git results, not canned repository-client responses.
- Restart restores intended app choices, and read-only journeys preserve controlled index, HEAD, refs and working-file bytes except explicit fixture mutations.
- npm exec -- vitest run tests/integration/NativeJourneys.test.tsx passes after bridge compilation; the shared npm run verify executes this coverage too.

Checklist

- [x] Add repository open/switch/sidebar-label rename/removal/restart assertions in tests/integration/NativeJourneys.test.tsx.
- [x] Assert staged, unstaged and untracked review endpoints and content.
- [x] Assert same-status external edits refresh the selected comparison without changing the selected file.
- [x] Assert commit expansion and parent-specific review, history pagination and return to working changes.
- [x] Assert view-only branch browsing and linked-worktree selection without checkout.
- [x] Assert the current repository-file browsing surface reads unchanged files through issued native authority.
- [x] Assert unavailable repository recovery preserves user data and does not become an empty-success state.

### Milestone 3: Presentation, accessibility and bounded resources

Toc: Presentation

Goal: Protect preference transitions and keyboard continuity, then observe actual rendered behavior and resource bounds.

Acceptance Criteria

- The planned tests/integration/PresentationJourneys.test.tsx asserts selection/content continuity across layout and appearance changes, remount persistence and keyboard focus behavior.
- Real Chromium observations cover wide/narrow layouts, loaded assets, scrolling/virtualization and keyboard navigation; DOM-only results are not labeled visual proof.
- Resource observations name fixture size, repetition count and bounds; no arbitrary timing threshold or native soak certification is inferred.

Checklist

- [x] Add missing presentation and keyboard journey tests after inspecting existing AppearanceReview, FileTreeIcons and Workbench tests.
- [x] Run npm exec -- vitest run tests/integration/PresentationJourneys.test.tsx after all slices land.
- [x] Exercise the actual Workspace in Chromium against the real-service bridge at wide and narrow viewports.
- [x] Observe large-file rendering and repeated review/context transitions for bounded DOM and subprocess lifecycle.
- [x] Record unsupported native assistive-technology observations explicitly rather than deriving them from Chromium.

### Milestone 4: Invariants, races and privacy

Toc: Failure boundaries

Goal: Add deterministic invariant coverage where example-based tests leave meaningful input or transition gaps.

Acceptance Criteria

- Parser/property cases use reproducible bounded inputs and assert semantic outcomes, not only no-panic behavior.
- Existing race, process-cleanup and storage-failure scenarios are mapped to exact test owners; new cases target uncovered boundaries without sleeps or global environment mutation.
- Hostile content remains inert, opaque authority stays context-bound, and packaged assets resolve offline in the exercised browser build.

Checklist

- [x] Extend bounded parser and identity invariants in src-tauri/tests/unit/diff.rs, status.rs and history.rs where gaps remain.
- [x] Map native concurrency, storage recovery and privacy tests in src-tauri/tests/integration to their protected invariants and add only missing deterministic cases within assigned ownership.
- [x] Exercise hostile content and invalid native identifiers through the journey boundary.
- [x] Run native library unit_tests:: and integration_tests:: through npm run verify after integration.
- [x] Exercise locally bundled production assets and run node scripts/check-licenses.mjs --inventory-only; distinguish inventory consistency from publication clearance.

### Milestone 5: Platform evidence and integrated handoff

Toc: Evidence

Goal: Record reproducible repository/browser evidence without converting unavailable native observations into passes.

Acceptance Criteria

- npm run verify -- --output .verification/testing-journeys records all required checks, with failed/skipped/not-run results preserved.
- An independent review is reconciled and all introduced blocking findings are resolved before completion is claimed.
- macOS arm64, Windows x64 and Ubuntu 24.04 x64 have explicit native-evidence status; missing human/platform prerequisites remain blocked.

Checklist

- [x] Run the consolidated shared verifier after all edits land and diagnose failures at their owning boundary.
- [x] Review the implemented tests, transport isolation, cleanup and evidence independently.
- [x] Record actual browser journey observations and remove owned throwaway smoke fixtures/services.
- [x] Assess available native artifact observations using the existing preview:evidence contract; never fabricate a human report.
- [x] Record missing platform installation/picker/IPC/secondary-window/assistive-technology observations and exact prerequisites.
- [x] Update this tracker with test names, commands, outcomes and unresolved limitations.

## Journey review order

Repository management → working-file review → external changes → committed review/history → branches/worktrees → presentation → recovery. The current repository browsing surface is an additional cross-layer path, not a reason to silently change product behavior.

## Evidence boundaries and deferred breadth

Use feature → success → boundaries → failure/recovery → concurrency → platform → dated evidence as the coverage matrix. Branch coverage and mutation testing are diagnostic techniques, not completeness percentages. Fuzzing here starts with bounded deterministic generated cases; a long-running external fuzz campaign, multi-platform soak baselines and native screen-reader certification need their own environments and measured evidence. Nothing in this tracker claims those observations have happened.

## Inspected coverage map

This inventory names inspected tests; it is not a fresh passing result.

| Boundary | Existing evidence owner | Added coverage target |
| --- | --- | --- |
| Workspace state and saved choices | tests/integration/Workspace.test.tsx; src-tauri/tests/integration/application_persistence.rs and persistence.rs | NativeJourneys couples visible choices to real native persistence. |
| Working and committed review | tests/integration/FileReview.test.tsx and CommitFileReview.test.tsx; src-tauri/tests/integration/diff.rs and committed_review.rs | NativeJourneys couples actual UI selection/endpoints to real Git content. |
| History and contexts | tests/integration/HistoryGraph.test.tsx and Workbench.test.tsx; src-tauri/tests/integration/history.rs and inspection.rs | NativeJourneys exercises pagination, merge parents, view-only branches and worktree selection. |
| Repository browsing | tests/integration/WorkspaceSidebar.test.tsx and RepositoryFileReview.test.tsx; src-tauri/tests/integration/browsing.rs | NativeJourneys reads unchanged content through issued listing/file IDs. |
| Presentation | AppearanceReview, FileTreeIcons, TextDiff, ResizableWorkbench and AppearanceMenu tests | PresentationJourneys adds composed selection/remount/focus/resource transitions. |
| Observation races | observation_internal.rs: switching_cancels_child_and_late_old_results_never_publish; same_id_reselection_resets_revision_and_drop_reaps_the_new_scan; slow_scans_coalesce_and_cached_reads_do_not_queue_work | Existing deterministic gates retained; no redundant observation test added. |
| Saved-choice races | application_persistence.rs: queued_rename_save_cannot_overwrite_a_newer_removal; cancelling_a_saver_keeps_its_write_serialized_with_newer_choices | Existing deterministic coverage retained. |
| Process cleanup | application_persistence.rs: cancelled_pending_open_reaps_its_child_and_preserves_saved_bytes; diff.rs: cancelled_review_reaps_git_and_retains_safe_causal_failure_evidence | Journey driver additionally owns bridge lifetime and private fixture cleanup. |
| Storage recovery | persistence.rs: corrupt_and_unsupported_files_remain_unchanged_after_navigation; failed_save_retries_on_next_successful_choice_and_clears_warning; diagnostics.rs hot-journal recovery/corruption/concurrent-writer cases | Existing byte-preservation scenarios retained. |
| Privacy and read authority | host.rs renderer metadata rejection; browsing.rs symlink-swap rejection and diagnostic privacy; diagnostics.rs private/symlink storage | Hostile-text journey and fixture Git/index/ref preservation supplement native boundaries. |
| Parser/authority invariants | src-tauri/tests/unit/diff.rs, status.rs, history.rs | Generated endpoint/path/UTF-8 framing cases and duplicate-cursor completion authority. |

## Native prerequisites not supplied by automated journeys

The bridge replaces the native picker with fixed fixture choices and bypasses actual Tauri window authorization and IPC instrumentation. Its diagnostic commands reject explicitly; no healthy capture is fabricated. Browser/source tests cannot satisfy the native observation report.

- macOS arm64: current install/launch, native-picker interaction, actual WebView journey, live secondary-window denial and assistive-technology observations require the real package and native/human access. The earlier restricted launch is not that evidence.
- Windows x64: no Windows runtime is available here. Working-file comparison currently returns explicit unsupported behavior outside Unix; tests must preserve that distinction instead of promising Unix parity.
- Ubuntu 24.04 x64: no matching Linux runtime or native WebKitGTK session is available here.
- Each platform requires its own artifact hash, observer/time, OS/WebView/Git metadata and all eleven scenarios in scripts/preview-evidence.mjs. Signing and notarization remain separate.

## Local evidence, 2026-10-02

| Check | Observed outcome |
| --- | --- |
| npm exec -- vitest run tests/integration/NativeJourneys.test.tsx | Seven passing real-Git journeys. Every journey checks HEAD, refs, index and expected working bytes before fixture shutdown. |
| npm exec -- vitest run tests/integration/PresentationJourneys.test.tsx | Five passing presentation cases, also passing in the full integration run. Fixtures model renderer dimensions; browser observations supply visual evidence separately. |
| npm run verify -- --output .verification/testing-journeys-final | 15/15 required checks passed: frontend 152 unit + 94 integration tests; native 30 unit + 147 internal integration + 52 external integration tests; 62 infrastructure tests; both builds, documentation and policy. No ignored/skipped tests reported. |
| node scripts/check-licenses.mjs --inventory-only | Passed, zero drift; 262 npm packages, 445 Cargo packages, 1,072 notice references. Only the manifest fingerprint changed: removing the new example declaration reproduces the prior recorded hash exactly. No dependency/provenance grants changed. |
| Independent transport review | Fixed the only finding: exclude the explicitly editable working path only in main, not secondary/linked repositories. Narrow re-review accepted the fix. Negative smoke modified each secondary file, observed gitStateUnchanged=false, restored bytes and observed all safety flags true; fixture directory absent after close. |
| Independent assertion review | No actionable findings in the seven real-service journeys, five presentation cases and seven generated native invariant tests. This was read-only review, not runtime proof. |
| .verification/journey-browser-evidence.json | Observed layouts, preferences, pointer/keyboard resizing, hostile source, bounded rendering and built-asset loading. Screenshots: .verification/journey-wide.webp, journey-narrow.webp, journey-bundled.webp. These local artifacts are not native-preview reports. |

Generated native cases are named generated_hunks_preserve_both_endpoint_counts_and_literal_source_text; overlapping_later_hunks_reject_the_whole_patch_on_either_endpoint; generated_literal_paths_keep_exact_identity_across_record_kinds; generated_invalid_path_segments_and_cross_category_duplicates_discard_valid_prefixes; generated_octopus_commits_preserve_raw_parent_order_for_both_object_formats; generated_batch_frames_use_byte_lengths_and_reject_every_incomplete_suffix; concurrent_cursor_completions_consume_authority_once_without_revoking_the_winner.

The 10,000-line Chromium measurement covered 100 layout changes and 200 full-file/hunk transitions. Line 9000 remained reachable, one comparison remained mounted, and each pane stayed below its viewport-derived bound, with a maximum of 48 rows. Eleven post-GC samples ranged from 33,965,244 to 37,676,164 JS heap bytes, with 5,110–5,236 DOM nodes and a stable document count. These are development-browser observations over 26.1 seconds, not a heap-leak proof, latency budget or multi-hour/platform soak.

Browser authority probes returned stale_observation for a forged path ID and not_found for a forged entry ID without changing the selected entry. Hostile img markup remained literal source with no injected image or executed sentinel. The bridge rejects host diagnostics explicitly; these probes do not establish Tauri window authorization or SQL privacy capture.

Unresolved publication blockers from the inventory remain npm:@napi-rs/lzma-linux-x64-gnu@1.5.1, npm:stackback@0.0.2 and assets:shiki-embedded-glsl. The selected-repository surface's missing level-one heading remains a moderate axe best-practice finding, not an accessibility pass. Native assistive-technology, packaged installation/picker/IPC/secondary-window observations, long-running fuzz campaigns and cross-platform soak baselines still require the environments described above.
