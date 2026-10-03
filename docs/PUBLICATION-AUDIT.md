# Source publication audit

## Scope and owner decisions — 2026-10-03

This report records **preparation of the selected source for publication on `main`**, not an installer release. The preparation evidence below predates the separately authorized upload; hosted follow-up is recorded at the end.

The owner selected a **clean, parentless initial `main` commit**, using their approved GitHub noreply identity. The cutover preserved prior history and four hooked remediation commits on the private backup branch `local/pre-publication-history`. The resulting `main` has one real commit and zero parents; its author and committer use the approved noreply identity. Publish only the reviewed `main` ref to public destinations: never the backup, unrelated branches, automation refs, reflogs, dangling objects or `.git`. The old history retains personal metadata, private-service bindings and earlier artwork whose rights were not established; replacing current files does not clear those historical objects for public disclosure.

The current artwork is the user-selected original View V design and generated native exports described in [asset provenance](../licenses/asset-provenance.json). Selecting a new root commit excludes superseded artwork from outgoing history; this is not trademark clearance.

The owner replaced the proposed email aliases with [private LinkedIn messages to Tobias Candela](https://www.linkedin.com/in/tobiascandela/). [SECURITY.md](../SECURITY.md), [CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md), the README and issue-template links use that canonical profile. Browser inspection reached LinkedIn's sign-up/authentication wall; no message was sent and delivery or unrestricted access is not claimed. Policies disclose account/connection/platform restrictions and prohibit sensitive details in public posts, comments, issues or PRs.

## Findings resolved in the selected source

| Finding | Remedy and evidence |
| --- | --- |
| Inconsistent frontend integration verification | A normal full Vitest run reproduced two layout/appearance cases exceeding the unchanged 5-second deadline. Repeated document-wide accessibility queries traversed the full workspace and open menus. Scope queries to their toolbar, file list, history or dialog and await menu transitions; retain all original actions/assertions and concurrency. The corrected full Vitest run passed 299 tests in 31 files. Shared frontend integration subsequently passed all 113 tests. |
| Missing LZMA and stackback license evidence | Select Rollup 4.62.2 without LZMA, and dependency-free why-is-node-running 3.2.2 with its original MIT grant. Preserve the diagnostic package's original ESM/CLI and add a mechanically verified CommonJS entry for Vitest. Neither removed package receives an invented grant. |
| Unclear embedded GLSL rights | Replace the Shiki runtime GLSL module with Evan Wallace's independently licensed GLSLX grammar at `0517b580f47749ae9913c8db3d724b92b9be79c8`. Retain original XML and MIT license; allow only three identity changes in derived JSON. App, worker, development and test resolution use the same replacement. Do not redistribute raw Shiki archives as cleared app assets. |
| Missing bundled notices | Collect `ThirdPartyNotices.txt` and `*.js.LICENSE` / `*.js.LICENSE.txt` alongside existing notice types. Preserve Playwright's third-party inventories and sidecars byte-for-byte. Remove notice files belonging only to superseded dependencies. |
| GLib iterator unsoundness | Backport the exact upstream mutable-output-pointer fix to authenticated glib 0.18.5 source. An optimized original consumer terminated with a segmentation fault; the patched consumer passed forward/backward traversal, skipping, exhaustion, overflow, Unicode and empty-array checks. |
| Unmaintained proc-macro-error | Migrate glib-macros and gtk3-macros to their existing syn 2 error APIs. Neither proc-macro-error nor its also-advised proc-macro-error2 fork remains selected. Positive Enum/Variant/closure consumers pass; seven rejected consumers retain intended messages/spans. A combined-error expression bug found by smoke was corrected with a block expression; both duplicate-watch locations are now reported without parser noise. |
| Cargo path dependencies omitted by advisory scanners | Both license refresh and strict check independently authenticate original archives and verify complete local source trees, exact iterator edits, narrow manifest changes and reviewed macro-source digests. Self-consistent replacement hashes alone do not establish approval. The full shared verifier now requires this provenance/license check. |
| Historical privacy/artwork selection | Owner-approved parentless source selection, with original history retained locally rather than erased or pushed. |
| Publication branch and reporting configuration | Workflow push trigger is `main`; pull-request checks remain enabled. Both issue-template contact links resolve to the approved canonical LinkedIn URL. Hosted defaults/protections remain separate owner setup steps. |

See [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) for exact original archive checksums, selected grants, source derivations, native backport scope and redistribution limits.

## Executed verification

| Check | Observed result |
| --- | --- |
| Locked npm installation | `npm ci` passed. |
| Frontend test suite and production build | 299/299 Vitest tests passed; TypeScript/Vite build passed. |
| License/source verifier regression checks | 20/20 targeted infrastructure checks passed. |
| Final strict license/source gate | Passed after refresh: 262 npm packages, 442 Cargo packages, 1,080 retained notice references, no drift and no blockers. |
| Shared required checks before final provenance refresh | 15/16 passed, including both builds, all frontend/native/infrastructure tests, executable documentation and policy. The license check correctly rejected inventory drift after the final macro correction/documentation update; a stale inventory is not clearance. |
| Final full shared gate after refresh | `git hook run pre-push` passed all 16 required checks, including strict publication licensing and source provenance. |
| Browser baseline and organization smoke | Both passed, including exact shader source, visible syntax/comment contrast, renderer-error checks, fixture safety and cleanup. Screenshots were inspected. Boundary: built Chromium renderer, production adapter and real disposable native service, not native Tauri IPC or a packaged WebView. |
| Emitted grammar inspection | C++ and Ruby chunk dependency closures contain the exact approved grammar JSON. All 39 emitted JavaScript files were inspected for serialized original GLSL bodies; none was present. |
| Optimized native dependency consumer | Final patched iterator, Enum, Variant and closure scenarios passed; seven negative macro consumers emitted intended diagnostics without macro panics. |
| npm audit | No advisories reported. |
| cargo-audit 0.22.2 | Exit 0, zero vulnerability entries and no warnings. Advisory database: 1,288 entries at `f8dee89e1b2f2f1eaf548312df7655fe5202a302`. Path-package source verification remains independently required. |
| Independent review | Read-only runtime and legal/provenance reviews reported no remaining findings; the combined-diagnostic correction received a separate bounded review. |

GLib development libraries were available on the macOS ARM64 host, enabling actual optimized dependency consumers. GTK3/WebKit development libraries and Linux/Windows native hosts were not available here. Compilation of the macro crates and metadata inspection do not establish a full Linux application build. Existing upstream compiler warnings were not suppressed or reformatted away.

The shared runner has 16 required checks. Local ignored `.verification/pre-push/manifest.json` records the exact revision, dirty state and outcomes of its most recent run. Hooks inspect the worktree, not each historical partial commit. The final full run passed after inventory refresh; earlier failures describe superseded states and remain documented above. Re-run the gate on the final committed `main` before any separately authorized push.

The initial orphan-commit attempt correctly failed the workflow-evaluation tests' committed-`HEAD` prerequisite. The cutover instead used a normal hooked amend with a temporary parentless graft, then removed the replacement ref. All hooks passed on that committed baseline; no test, hook or revision-binding requirement was disabled. Real parent count was checked with replacement objects disabled.

A subsequent required run exposed two more integration failures in repeated layout transitions and icon preference/remount coverage. An isolated integration pass did not clear that failure; a normal full-suite diagnostic also reproduced an appearance-test timeout. Remaining document-wide queries in these three scenarios were scoped to their actual dialogs, trees and review controls, repeated layout actions reuse their mounted trigger, and icon-menu outside-click dismissal is explicitly awaited. All original actions, assertions, repetition counts, deadlines and concurrency remain. The corrected normal full suite passed 299/299 tests, and organization browser smoke passed all phases with fixture safety and cleanup. The archived 49-commit history was not changed by this later test-only stabilization.

## Secret scanning and source boundary

Use Gitleaks 8.30.1 from its official release with verified archive SHA-256. Scan the exact exported source tree and the selected outgoing `main` history independently, with built-in rules, full redaction, five decoding levels, no archive traversal, no size cutoff, no baseline/suppression file and no inline allow comments. Do not upload raw reports. Asset-hash findings must be independently compared with the corresponding same-revision bytes rather than suppressed.

The selected source-tree export and the outgoing parentless `main` history were scanned independently after remediation. Each reported seven matches, all SHA-256 asset checksums in `licenses/inventory.json`; every value was independently verified against its corresponding same-revision asset bytes. No confirmed credential was found in these scopes. Both scans intentionally exited 1 for those findings; no suppression was added. The preserved backup and unrelated refs are outside this publication selection, not cleared for disclosure.

The outgoing source excludes ignored connector/account configuration, environment credentials, application databases, diagnostics, screenshots, scanner material, caches, node_modules and build outputs. Do not broaden the publication selection to a checkout archive or all Git refs. Secret-pattern scanning is not exhaustive semantic privacy review, legal clearance or evidence that no external copy exists. Preserved upstream authorship/contact/license information is intentional third-party attribution, not GitView's reporting route.

## Remaining hosted and release boundaries

The separately authorized source upload published the parentless baseline to [AntoCandela/GitView](https://github.com/AntoCandela/GitView). `main` is the hosted default branch and is protected by pull requests and all three required platform checks, without a bypass actor. [GitHub setup](GITHUB-SETUP.md) remains the setup reference; protection configuration is not evidence that those checks pass.

GitHub private vulnerability reporting remains optional and unconfigured here; the approved LinkedIn route is not evidence that GitHub reporting is enabled. Installer signing/notarization, native picker/window authorization, packaged resource inspection, target-specific runtime licenses and binary corresponding-source obligations remain outside this source-publication evidence. Do not describe this preparation as a certified public binary release.

## Hosted verification follow-up

The [first hosted baseline run](https://github.com/AntoCandela/GitView/actions/runs/37120918867) failed on all three platforms. The pre-publication local pass did not establish fresh-checkout or cross-platform readiness. Comparing the six initial Dependabot pull requests against that baseline separates inherited failures from additional TypeScript, Vite and Vitest failures; none of those dependency updates is cleared by this report.

Local reproductions established three environment/source gaps: two ignored Lucide icon implementation files had been incorrectly inventoried as legal texts; standalone documentation drivers lost the workspace's Cargo path patches; and native fixtures inherited unsupported global Git filters. A CRLF tar-listing probe also reproduced dropped notice entries. Corrections preserve actual license sidecars, reapply the audited patches and owning SQLite requirement, bind workflow evidence to vendored sources, isolate only test-child Git configuration and accept both line endings. The corrected documentation CLI passed with a fresh Cargo home, and the shared integration CLI passed under a synthetic hostile global filter.

Retained legal-text hashes are ordered by normalized relative keys, not platform-native absolute paths. Otherwise Windows places `gtk3-macros` before `gtk`, reversing four serialized entries despite identical bytes. The strict byte/hash comparison remains unchanged; the corrected inventory check passes with 1,078 notice references and no drift or blockers.

Windows also exposed canonical-path diagnostics failures and a library-test loader exit of `0xC0000139`. The repair skips filesystem queries on syntactic drive prefixes and supplies the Common Controls v6 activation dependency to the library test executable, following [Tauri's documented workaround](https://github.com/orgs/tauri-apps/discussions/11179). The [first repair matrix](https://github.com/AntoCandela/GitView/actions/runs/37124988476) confirmed Windows native build/library startup and diagnostics now pass, with 15/16 required checks passing on macOS and Linux. It was not a green matrix: remaining fixture, documentation, frontend and Windows evidence failures still required repair.

Follow-up fixture corrections wait for cleanup-record admission after actual child reaping, drain batch input before testing malformed output, require complete bounded diagnostic queries, and assert the declared non-Unix working-file limitation while adding staged-file success coverage on every platform. The remount/preferences journey scopes accessibility queries to their owning surfaces. The standalone documentation executable now owns its Windows activation dependency too. Required checks, production deadlines, test concurrency and production filter rejection remain intact.

Failure evidence retains only catalogued source locations, fixed classifications, bounded timing and closed documentation stage/process facts. A real parameterized Vitest failure exercised this path: the report identified its assertion line and omitted private parameter/assertion content. Regression checks reject contradictory zero-exit documentation success and preserve valid all-suite reporting without inventing documentation test counts.

A later pre-push run correctly blocked publication on an absolute live-read call-count assertion. Live previews explicitly poll once per second before historical activation, so the total depends on journey duration rather than routing correctness. The incidental count and unused spy were removed; pinned/live content, selection state and the separate no-fallback scenarios remain covered. The normal 113-case frontend integration suite passed afterward.

The [second repair matrix](https://github.com/AntoCandela/GitView/actions/runs/37128924786) again passed 15/16 checks on macOS and Linux, with only cancellation-evidence capture failing. Windows license collection and observation now passed; remaining evidence identified a documentation-driver build exit, three frontend deadline failures, five diagnostic flush failures and an opaque persistence fixture command. The Vite/Vitest dependency checks lacked usable framework reports on every platform: macOS/Linux returned zero, while Windows returned one. That identifies process/evidence failures, not which test cases failed.

The cancellation fixture now bounds its own traced readiness traffic and keeps the current-thread executor runnable while waiting for durable cleanup evidence. Related fixtures avoid blocking observation tasks on flush and identify fixed flush failures separately; Git fixture panics retain their source caller. These changes preserve deadlines, child-reaping, causality and privacy assertions. They do not establish the causes of every Windows flush or persistence failure.

The three frontend deadline cases retain their complete keyboard/remount journeys but omit user-event's synthetic zero-delay timers, unnecessary accessibility scans and one redundant dialog reopening. All 14 cases in the two affected files passed locally with the original five-second deadlines and normal concurrency. A real Rust compilation failure retained only `E0425`, omitted its private source marker/path, and was followed by a successful real documentation-driver run; all 32 targeted privacy/report regressions passed. Hosted Windows still must resolve the build failure using bounded compiler identifiers rather than published raw output.

The corrected shared integration CLI also passed locally with a complete Git 2.55 installation and the hostile global-filter fixture. Independent static reviews found no introduced scheduling/privacy defects; neither that local run nor those reviews certify the still-pending hosted Windows corrections.
