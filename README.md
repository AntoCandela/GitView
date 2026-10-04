# GitView

GitView is a local desktop workspace for inspecting Git repositories: live changed-file trees, side-by-side file comparisons and a paged commit-ancestry graph. It uses your installed Git and keeps workspace choices on your machine.

**Developer preview.** Build from source to try it. Source availability is not a tested installer release, a cross-platform support guarantee or signing/notarization approval. Earlier passing checks do not certify the current uncommitted application, UI and native changes; see [verification evidence and limits](docs/VERIFICATION-EVIDENCE.md#scope-and-freshness).

## What it does

- Opens existing working trees, linked worktrees and bare repositories; saves repository order, app-only display names and selection locally.
- Monitors working-tree changes and rereads the selected file approximately one second after each completed scan/read. Staged compares HEAD → index; unstaged compares index → working file; untracked has an absent old side. Switching categories never stages a file.
- Browses locally known commit ancestry in 100-commit pages, with branch/tag labels, merge lanes and explicit shallow-history boundaries. **Refresh history** captures newer refs; browsing does not fetch.
- Expands a commit's changed-file tree. Select a committed file for a parent → commit comparison; merges offer a parent selector, and verified root commits compare with the empty tree. Selecting a working-file row returns to live comparison.
- Offers six workbench arrangements, pointer/keyboard resizing and **Classic**, **Material** and **Catppuccin Latte** file-icon themes.
- Reads **Changes** or **Full file** with aligned side-by-side source, **Scroll / Wrap** long-line controls, inline change emphasis and locally bundled Shiki syntax styles.
- Offers Brazilian Portuguese, European Portuguese, Italian, Spanish, US English and British English for app-owned controls and explanations, with a persistent **Language** selector and native **System** preference matching.
- Offers an opt-in macOS menu-bar companion for the same admitted repositories and live comparisons, with acknowledged **Open in GitView** handoff.

Git inspection does not checkout, stage, commit, fetch, push, delete repository files or change Git configuration. GitView **does mutate its own app state**: opening/selecting worktrees, renaming labels and removing sidebar entries update private workspace storage; diagnostics and presentation preferences also write local data. It does not install or bundle Git, and it preserves Git's safe-directory checks.

## Run from source

Install:

- [Node.js](https://nodejs.org/) **24.21+ on Node 24 LTS**, and npm. The exact shared CI baseline is in `.nvmrc`; with nvm, run `nvm install && nvm use` before installing dependencies.
- [Rust](https://www.rust-lang.org/tools/install), Cargo and an [existing Git installation](https://git-scm.com/downloads) available on `PATH`.
- [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/): Xcode Command Line Tools on macOS; C++ build tools and WebView2 on Windows; WebKitGTK 4.1 and the listed distribution-specific development libraries on Linux.

From the repository root:

```sh
npm ci
npm run tauri dev
```

`npm run dev` serves web assets only: it does **not** supply native Git operations, the folder picker, diagnostics or the authorized surface bootstrap needed to mount the application. Use the native app, or the explicitly isolated browser-journey transport documented in DEVELOPMENT.md. `npm ci` installs the versioned Git hooks outside CI, while preserving existing custom-hook ownership; `npm run hooks:install` retries installation explicitly.

```sh
npm run build          # TypeScript check and web assets, not a desktop installer
npm run tauri build    # Native build/package attempt; not release certification
npm run test:unit      # Shared frontend, Rust and infrastructure unit groups
npm run test:integration
npm run verify         # Both builds, all shared checks, executable agent docs and policy
```

`npm test` runs frontend tests only. For Rust directly, use `cargo test --manifest-path src-tauri/Cargo.toml --locked --target-dir .verification/native-target`. The jsdom development dependency can emit a `whatwg-encoding` deprecation notice during installation; use `npm audit` to inspect advisories, not `npm audit fix --force` to suppress a notice.

Read [DEVELOPMENT.md](DEVELOPMENT.md) for diagnostics, failure evidence and hooks; [CODE-STYLE.md](CODE-STYLE.md) and [module ownership](docs/VERIFICATION-EVIDENCE.md#module-ownership) describe the existing implementation boundaries.

## Using the workbench

1. Open the compact repository selector, choose **Open repository**, and choose a folder using the native picker. A worktree subdirectory is valid. Opening an alias reuses its entry; linked worktrees remain separate contexts.
2. **Select the new row yourself**—opening does not automatically activate it. Picker cancellation or an invalid repository leaves existing entries intact. Unavailable locations stay listed and are never called clean.
3. Select a changed file and, for partially staged files, choose the comparison category in its toolbar. A disappeared file/category shows **No remaining changes**, rather than silently selecting something else.
4. Use the graph's branch picker to view another local branch **without checkout** or changing the actual-worktree sidebar. Selecting an existing worktree instead activates that context and saves the choice. Click a commit to expand/collapse its files; select a file to review the historical endpoints.

A row's kebab menu acts without selecting the row. **Rename** edits the app label inline: Enter saves a trimmed nonempty name; Escape before submission discards the draft. It never renames the folder, and a submitted native operation may finish after the editor closes. **Remove from sidebar** removes only the entry, never files or linked-worktree siblings. Removing the active row selects the first other available entry in stored order, or clears selection; reopen a folder to add it again.

The repository selector stays centered in the app header; the branch/worktree picker belongs to the Git graph header. The leftmost header button expands or collapses a **Files-only sidebar**, without closing the selected preview. It lists the current working repository's unchanged, changed, untracked and ignored files, overlays available Git-status markers, and shares the changed-file explorer's Tree/List switch, folder controls and icon theme. Click a file to read its current working contents in the shared viewer; **Refresh files** also discovers changes to ignored files. Git metadata is excluded; nested repositories and symlinks are opaque and cannot be read through this browser.

Repository files load progressively in bounded directory pages, including ignored files. Folders start collapsed, including folders discovered later; **Expand all** also expands newly discovered descendants. **Files loaded** is a partial count, not a repository total. Expanded folders continue discovery; collapsing a folder pauses further pages beneath it, while List view discovers all folders. Progress stays in the count rather than an explanatory footer; actual loading errors remain visible. Refresh starts a new listing without closing an already selected preview. File trees, flat lists and history rows—including graph dots, connections and expanded committed files—render only a viewport-sized window plus interaction rows; scrolling preserves selection, keyboard navigation and expanded-commit state.

The default Cream/navy/teal workbench has zero workspace padding/margins, 28px file rows and paths in accessible tooltips. By default the old/new comparison is above the file explorer and graph. **Workbench layout** offers six keyboard-accessible radio previews, preserving mounted panels and the reading choice. Drag a divider, or focus it and use arrow keys, Shift for larger steps, Home or End. Layout and divider sizes are in-session choices, not saved workspace data.

**Appearance** in the top toolbar offers six coordinated presets (Cream, Paper, Mist, Stone, Graphite and Midnight), plus independent interface, syntax and file-icon choices behind chevron disclosures. Changing only the interface keeps explicitly selected syntax and icons; **Match interface** follows its palette when you change it. A live sample shows a tree icon and both old/new source lines. Classic uses Lucide; Material uses upstream SVG overrides; Catppuccin uses Latte artwork. Plain, GitHub Light/Dark, Catppuccin Latte and Solarized Light syntax themes use locally bundled Shiki assets; GitHub Dark's license is already covered by the canonical bundled theme notice. Sources and original licenses remain available in the popover. Icon SVGs stay in `src/assets/icon-themes`, while canonical notices stay in `licenses/texts/assets` rather than being copied into `src/assets`.

The interface palette, icon pack and review choices persist independently in webview storage (`gitview.app-theme`, `gitview.icon-theme` and `gitview.code-review`); a failed save is reported as session-only. The review header has compact **Changes / Full file** and **Scroll / Wrap** switchers. Complete verified old/new snapshots—not reconstructed hunk snippets—supply syntax context and full-file reading. Wrapping aligns unequal-height source rows without rendering every file line; switching back restores independent horizontal offsets. Source text never goes to a remote highlighting service. [Comparison contracts and limits](docs/VERIFICATION-EVIDENCE.md#live-file-comparisons) · [icon lookup and provenance](docs/VERIFICATION-EVIDENCE.md#icons).

**Language** is available in both empty and active workspaces. System uses the ordered native UI-language preferences; exact `pt-PT` and `en-GB` stay distinct, generic Portuguese uses `pt-BR`, and unsupported languages fall back to `en-US`. Manual choices apply immediately without resetting repository/file selection and persist under `gitview.locale`. While System is resolving, the previous language remains active; choosing a manual language cancels that older intent. Failed saves leave the choice active only for the session and display a warning. Repository data, source, commit messages, branch names and diagnostic codes are never translated. The app-owned picker title follows the selected language; OS-owned dialog buttons do not.

Selecting the already-checked language retries a session-only save. Selecting the already-checked System option resolves native preferences again. Branch/worktree choices, grouped history references and Appearance disclosure tooltips use the same live locale while preserving original repository names.

Catalog completeness is enforced during `npm run build` and shared verification. This is not linguistic or packaged-platform certification: each regional catalog still requires human review, and each native target requires six-locale observations. Localization of raster/SVG and rendered-document previews awaits those missing predecessor surfaces; see [issue #15](https://github.com/AntoCandela/GitView/issues/15).

### macOS menu-bar companion

The companion is off by default. Enable it from **Menu-bar companion** in the main toolbar; it does not register login startup. Click its menu-bar icon to toggle the compact review panel. The native context menu offers **Open GitView** and **Quit**.

The panel selects only already-admitted repositories/worktrees and reuses the changed-file tree and live comparison viewer. Repository admission, removal, worktree management, history, file browsing and preference controls remain in the main window. Language, appearance, icons and reading choices follow the main window, including session-only choices after a failed save.

Each opening waits for a newly started native scan. Hidden surfaces stop periodic review work; both hidden surfaces suspend shared scanning and recovery. **Open in GitView** transfers the exact native-validated reading choice, including a truthful **No remaining changes** outcome. Merely focusing the main window is not successful delivery: the panel closes only after main applies and acknowledges that request.

Closing main hides it only while the enabled companion remains available. Disabling first reveals main, then removes the companion. Explicit **Quit** closes the application and drains owned native work. Activation/save failures remain visible in the main settings; unavailable native access never justifies hiding the only usable main window.

The opt-in lives in native `companion.json` beside workspace data. Unreadable or unsupported preference documents are preserved; failed saves leave an explicit session-only choice. Windows and Linux do not expose this macOS companion. Native focus, Spaces/fullscreen, display configurations and resource-budget acceptance require separate observations; implementation and automated tests are not release certification.

## Platforms and limitations

| Boundary | Actual evidence / limitation |
| --- | --- |
| macOS arm64 | Earlier native executables launched and WKWebView IPC/diagnostic correlation was observed. An unsigned DMG was built; installation, native-picker interaction, current visual flow and live secondary-window denial remain unverified. |
| Windows / Linux | The shared CI configuration targets Windows and Ubuntu as well as macOS; that is not observed native-platform support. Windows 11 x64 and Ubuntu 24.04 x64 packaged previews remain unverified. |
| Working-file reads | Secure working-file reads are currently implemented for Unix; repository-tree enumeration is implemented for macOS/Linux. Other platforms return explicit unsupported/unavailable outcomes. Linux implementation does not establish observed Linux preview readiness; Windows is not equivalent to macOS/Linux. |
| Public installers | Signing, notarization and public-release readiness are unassessed. Repository tests, Chromium fixtures and Tauri MockRuntime do not certify installers or picker behavior. |

Only a successful empty status snapshot is **Clean**. Configured clean/process filters, configuration races, unsupported extensions, sparse/split indexes and assume-unchanged/skip-worktree flags fail closed as **Changes unavailable**, not clean. Conflict, rename/copy, type-change and submodule inspection can be listed but return explicit unsupported review outcomes. Recursive submodule status is disabled, including for clean gitlinks. Binary, oversized, unsupported and unavailable comparisons are distinct from empty text; bounded reads, deadlines and unavailable shallow parents can limit inspection. This is not a sandbox for a malicious Git executable or same-user interference.

## Local data and privacy

The native host stores `workspace.json` and `diagnostics/diagnostics.sqlite` in Tauri's per-user application-data directory, outside the source and inspected repositories. Workspace data contains real repository locations, optional app labels and selection—not credentials, contents or cached Git facts. Do not commit or attach that private file. Missing locations survive restart; Git facts are rechecked. An unreadable/unsupported workspace file is preserved and replacement disabled for that session: repair it deliberately and restart. Save failures leave navigation usable and show a separate warning; a later successful choice retries saving.

Diagnostics are local, not uploaded. Their closed vocabulary records operation IDs, timings, outcomes and counts—not repository names/paths, file or branch contents, command arguments/output, credentials, environment values or exception text. Capture is bounded and can drop records; active capture is not lossless. Authorize the specific local store before using the [read-only diagnostic CLI](DEVELOPMENT.md#locate-and-query-the-store), and share only sanitized evidence. Unix workspace/diagnostic files use owner-only permissions. See [persistence details](docs/VERIFICATION-EVIDENCE.md#private-workspace-persistence) and [diagnostic limits](docs/VERIFICATION-EVIDENCE.md#local-diagnostics).

## Native-preview evidence

[The artifact-bound report contract](docs/VERIFICATION-EVIDENCE.md#native-preview-evidence) records real human observations for the exact platform and artifact, including install/launch, picker, IPC, repository behavior and diagnostics. Missing, blocked or failed observations leave readiness false. A validated report is attributed evidence, not independent certification. [Historical verification evidence](docs/VERIFICATION-EVIDENCE.md#historical-verification-account) preserves prior failures, smoke boundaries and unresolved gaps; it does not certify newer code.

## Contributing, security and license

- [CONTRIBUTING.md](CONTRIBUTING.md): setup, scoped changes and the shared verification workflow. Public contributions must not require access to a private product graph.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): participation expectations.
- Questions or a conversation: [message Tobias Candela privately on LinkedIn](https://www.linkedin.com/in/tobiascandela/). LinkedIn sign-in or messaging restrictions may apply.
- [SECURITY.md](SECURITY.md): the approved private vulnerability-reporting route and its limitations. Never put secrets or private repository/workspace data in a public issue.
- GitView source is licensed **GPL-3.0-only**, without warranty; see [LICENSE](LICENSE). Third-party code, fonts and artwork retain their own licenses: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), [dependency inventory](licenses/inventory.json) and [asset provenance](licenses/asset-provenance.json). Review the strict license gate and artifact-specific obligations before distributing source or binaries.
- [Publication audit](docs/PUBLICATION-AUDIT.md): source-selection decisions, remediation evidence and verification limits. Preparation is local-only; no hosted repository or installer readiness is claimed.
