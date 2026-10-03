# Contributing to GitView

GitView is a **source developer preview**. Contributions are welcome through this repository's GitHub issues and pull requests. Building from source, passing CI or exercising a browser fixture does not certify native installers, signing, notarization or packaged-platform behavior.

You do not need an Auto-K account, a private product graph, an MCP connector or a globally installed agent toolkit. Maintainers retain approval of product scope and merge decisions. Authorized maintainer-private planning is separate from the public contribution workflow.

## Before starting

- Read [README.md](README.md) for prerequisites, current capabilities and preview limits.
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md). For suspected vulnerabilities, read [SECURITY.md](SECURITY.md) instead of opening a public issue with details.
- Search existing issues and PRs. For a substantial feature, behavior change, new dependency or architecture change, open an issue describing the problem, affected users, proposed scope, alternatives and observable success criteria. Get maintainer agreement before investing in implementation. Small bug and documentation fixes can go directly to a focused PR.
- Keep the agreed scope and acceptance criteria visible in the issue/PR. Public contributors are not required to retrieve private planning artifacts or make private product-graph decisions.
- Coding agents must read [AGENTS.md](AGENTS.md) and complete its mandatory session-start gate before editing or launching a worker. Concurrent work requires an explicit ownership contract and one integration owner; do not copy, stash or commit someone else's unfinished changes.

## Develop a focused change

1. Fork the repository on GitHub, clone your fork and create a branch for the agreed change. Keep unrelated edits out of the PR.
2. Install the prerequisites listed in [README.md](README.md), then run `npm ci` from the repository root. This uses the lockfile and installs supported local hooks while preserving existing hook ownership.
3. Use `npm run dev` for browser-only frontend work or `npm run tauri dev` for native Git, folder-picker and SQLite boundaries. Use disposable repositories and temporary fixtures, not valuable user workspaces.
4. Follow [DEVELOPMENT.md](DEVELOPMENT.md) for commands and diagnostics, [CODE-STYLE.md](CODE-STYLE.md) for code/test conventions, and [DESIGN-RULES.md](DESIGN-RULES.md) for frontend work. Reuse the existing platform boundaries and shared verifier rather than introducing a parallel runner or logger.
5. For a bug, capture a minimal synthetic reproduction and prove the changed path works. Add or update behavioral regression coverage where appropriate. For UI changes, exercise the actual surface and provide sanitized visual evidence. Never claim native behavior from a browser/mock run.

## Verify and open a PR

Use the checks appropriate to the changed boundary while working:

```sh
npm run test:unit
npm run test:integration
npm run policy
```

Before handing off the change, run the existing full shared verifier:

```sh
npm run verify
```

The full verifier includes frontend/native builds, both test categories, agent documentation examples and repository policy. It requires the native prerequisites and cached/downloadable dependencies described in the development guide. When agents collaborate, the integration owner runs consolidated verification after the slices land; do not launch competing project-wide checks against half-finished edits.

A passing suite is not smoke evidence: launch the relevant application/CLI, perform the changed action and report the observed result. Include exact commands, outcomes, affected platform, and anything failed, blocked, skipped or unverified in the PR template. Missing native prerequisites are a limitation, not a pass. Update affected contracts, callers and documentation in the same change. Maintainers may request changes or decline a proposal even when checks pass; do not merge or change release status on their behalf.

## Keep public reports safe

Issues, PRs, screenshots and attachments are public. Use synthetic repository/file names and relative fixture paths. Give OS/architecture and tool versions, concise reproduction steps, expected/actual behavior, fixed error codes and sanitized aggregate outcomes.

Do **not** upload diagnostic databases or journals, workspace files, raw logs or stack dumps, verification directories, credentials, environment/config payloads, personal contact details, private repository contents, raw local paths or private product/account identifiers. Reading a local diagnostic store requires its owner's explicit authorization; repository access alone is not consent. Summarize only bounded sanitized evidence using the existing diagnostic workflow. If sensitive material was accidentally published, follow [SECURITY.md](SECURITY.md); deleting a comment does not revoke a leaked credential.

## Contribution licensing and third-party material

Original contributions to GitView are submitted under **GPL-3.0-only**, the GNU General Public License version 3 only; see [LICENSE](LICENSE). By submitting a contribution, you represent that you have the right to provide it under these terms. You retain your copyright. This project does not require a separate contributor license agreement or copyright transfer.

Third-party code and assets retain their upstream licenses and notices. GPL licensing of GitView does not relicense dependencies, fonts, icons, logos or bundled artwork. Before adding or updating any of them:

- Identify the upstream source, exact version/revision, license text and provenance; for vendored assets include the source artifact and integrity evidence.
- Check compatibility with GPL-3.0-only source and the intended distribution, including attribution, redistribution, font restrictions and trademark limitations. Do not assume a package's SPDX field covers every bundled asset or logo.
- Preserve required copyright/license texts and update [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), the [license inventory](licenses/inventory.json), and [asset provenance](licenses/asset-provenance.json) as applicable. Explain modifications and any unresolved permission question in the PR.
- Run the repository license audit with `node scripts/check-licenses.mjs`. A failed or unresolved license finding blocks publication of the affected material; do not silently waive it or claim distribution clearance from a source build.

Do not contribute proprietary material, personal data, secrets or artwork whose redistribution rights you cannot establish. Maintainer review of a PR is not independent legal certification.
