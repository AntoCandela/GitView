# GitHub repository setup

This preparation is **local-files-only**. The settings below are a future owner-operated checklist, not permission for an assistant to change GitHub settings. This guide prepares **source publication**, not a release, installer certification, signing or notarization. It does not authorize creating a repository, changing visibility, pushing refs, publishing releases, rewriting history or widening access. The owner must approve the target repository and publication/history choice separately. Never push all branches, mirror refs or include local automation refs merely to publish the source.

## Observed setup boundary

During the 2026-10-02 preparation, this checkout had **no Git remotes**. GitHub CLI was available and authentication succeeded, but `gh repo view` could not resolve a repository. Authentication alone does not identify the intended repository or prove administrative permission.

Consequently, no hosted workflow runs, effective branch protections, repository security settings or GitHub private vulnerability reporting route could be inspected, and no external settings were changed. The target repository may or may not already exist; this checkout cannot establish that. None of the settings below are claimed to be enabled. Target/access confirmation, effective protections and hosted evidence remain owner setup steps, not CI successes or blockers to preparing local source. GitHub private reporting is an optional additional route alongside the owner-approved LinkedIn contact.

The owner selected [private LinkedIn messages to Tobias Candela](https://www.linkedin.com/in/tobiascandela/) for conversations, vulnerability reports and conduct reports, replacing the previously proposed email aliases. Ask reporters to identify “GitView security report” or “GitView conduct report” and start with a non-sensitive summary. LinkedIn may require sign-in, a connection or another messaging option; delivery and universal access are not certified. Never direct sensitive details to public profile comments, posts, issues or PRs.

## Repository-side configuration

- `.github/workflows/verification.yml` preserves the shared verifier on macOS ARM64, Windows x64 and Ubuntu 24.04 x64. It uses `pull_request`, not `pull_request_target`, and a read-only `contents` permission. It does not need repository secrets for fork pull requests; checkout credentials are not persisted for dependency scripts.
- Each action is pinned to an upstream commit. The checkout, Node setup and artifact upload pins were resolved from the upstream release tags `v7.0.1`, `v7.0.0` and `v7.0.1`, respectively; the Rust action pin was resolved from its `stable` branch. Its `toolchain: stable` input is explicit, so SHA pinning does not depend on the branch name to select Rust. Upstream refs and the `action.yml` at each commit were inspected on 2026-10-02. Node remains version 22 and Rust remains rolling stable: action pins do not freeze toolchains or runner images.
- `.github/dependabot.yml` requests weekly npm updates at the root, Cargo updates under `src-tauri`, and GitHub Actions updates. It does not grant auto-merge or bypass protection. Review dependency/asset license implications and lockfile changes as well as security fixes; retain full action SHA pins with accurate version comments.
- The workflow uploads only each platform's sanitized verification `manifest.json`, not logs, native diagnostics, screenshots, application data or build outputs. Review public console output too; an artifact allowlist does not sanitize every command's stdout.

## 1. Confirm the actual target and access

The owner must first identify the intended repository and connect this checkout to its reviewed remote through their normal authorized workflow. Do not guess an account/repository from a local directory name, create a new repository automatically or publish a private remote URL in an issue.

Once that connection is established, these **read-only** commands resolve the actual repository instead of embedding placeholder identities:

```sh
# Capture the identity locally; do not copy it into a public readiness report.
REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner) || exit 1
# Confirm this is the owner's intended target before continuing.
gh repo view --json viewerPermission --jq .viewerPermission
BRANCH=$(gh repo view --json defaultBranchRef --jq '.defaultBranchRef.name // empty') || exit 1
[ -n "$BRANCH" ] || exit 1
BRANCH_ENCODED=$(gh repo view --json defaultBranchRef --jq '.defaultBranchRef.name | @uri') || exit 1
```

A repository administrator (or a specifically delegated role) must review and apply the settings. Stop if the target is wrong or authorization is insufficient; do not grant additional account/token permissions to work around that. A missing default branch means there is no branch to protect yet. The publication branch is `main`, and the workflow's push trigger targets only `main`; pull request CI remains unfiltered. Set the hosted repository's default branch and protection rules to `main` after the separately authorized initial upload.

Inspect Settings → Rules → Rulesets and the branch protection settings, including inherited organization rules. These optional read-only queries give bounded summaries; inspect full rule details privately in the UI before changing anything:

```sh
gh api "repos/$REPO/rulesets?includes_parents=true" \
  --jq 'map({target, enforcement})'
gh api "repos/$REPO/branches/$BRANCH_ENCODED/protection" \
  --jq '{required_status_checks: .required_status_checks, enforce_admins: .enforce_admins.enabled, allow_force_pushes: .allow_force_pushes.enabled, allow_deletions: .allow_deletions.enabled}'
```

A `403`, `404`, empty result or `null` value is not proof that a feature is enabled or absent: authorization, repository state, plan and API availability can differ. Branch protection queries do not replace reviewing rulesets. Keep raw configuration, account data and private repository identifiers out of public reports.

## 2. Inspect hosted evidence, then require the stable CI contexts

Inspect actual runs without starting, rerunning or dispatching one:

```sh
gh run list --repo "$REPO" --workflow verification.yml --limit 10 \
  --json databaseId,headSha,status,conclusion,event
```

Choose the run corresponding to the reviewed revision and inspect its jobs with `gh run view --repo "$REPO" --json jobs` using that run's real ID (or the CLI's interactive run selection). Do not copy raw logs to a public report. No run, a run from another revision, a cancelled/skipped job, or local verification alone does not prove all three hosted jobs passed. Check the manifest revision and verification results as well as the job conclusions.

Configure an active ruleset or extend existing branch protection for the confirmed default branch, without replacing or weakening existing/inherited rules:

- Require a pull request and conversation resolution before merging.
- Require these exact job check names, with **GitHub Actions** selected as their expected source where supported:
  - `Verify macos-arm64`
  - `Verify windows-x64`
  - `Verify ubuntu-24.04-x64`
- Require the branch to be up to date before merging. Select the contexts after they have actually appeared on the repository; verify their names/source from hosted jobs rather than selecting a similarly named check.
- Block force pushes and branch deletion. Apply protections to administrators; do not add bypass actors or broaden existing permissions.
- Preserve existing review requirements. If there is an independent maintainer who can review, require at least one approval and dismiss stale approvals. Do not add a self-review requirement that a sole maintainer cannot satisfy or weaken an existing approval policy to make merging easier.

Do not enable a merge queue as part of this setup: this workflow has no `merge_group` trigger. If the repository already requires a queue, the owner must reconcile that existing requirement and CI trigger before merging; do not disable the protection to bypass it. Do not rename these jobs without updating the required contexts. Ensure no other workflow publishes the same job names.

## 3. Keep Actions safe for outside contributions

In Settings → Actions → General, retain the existing organization action policy and allow only the reviewed actions needed by this workflow, if a tighter compatible allowlist is available. Keep the default workflow token read-only and leave permission to create/approve pull requests disabled. Require approval for outside collaborators' fork workflows where supported. Use GitHub-hosted runners for untrusted pull requests, not a privileged self-hosted machine. Do not enable sending secrets or write tokens to fork workflows, change to `pull_request_target`, add personal tokens to CI, or weaken an inherited policy to make a run pass.

Action major-version upgrades now use the upstream Node 24 action runtime, independently of the project's Node 22. Hosted execution on each declared runner remains unverified until actual runs are inspected.

## 4. Enable available security controls and a monitored reporting route

In Settings → Advanced Security (the label may differ by GitHub version), inspect the repository's available controls and enable, where supported:

- Dependency graph, Dependabot alerts and Dependabot security updates. The committed version-update file alone does not establish that alerts/security updates are enabled. Confirm npm and Cargo manifests/lockfiles are recognized, and that Actions update pull requests retain SHA pins.
- Secret scanning and repository push protection. GitHub documents automatic free scanning for public repositories, but that is an availability statement, not evidence about this unresolved repository. Private/internal access depends on repository ownership and plan. Review the actual state and alerts; do not purchase a plan, change visibility, expose findings, bypass detections or enable provider-contacting validity checks without separate authorization.
- Private vulnerability reporting, when supported. GitHub documents this feature for public repositories. An administrator must enable it and configure security-alert subscriptions/notifications for a maintainer who will monitor reports.

After target/access confirmation, these read-only queries inspect bounded state:

```sh
gh api "repos/$REPO" \
  --jq '{secret_scanning: .security_and_analysis.secret_scanning.status, push_protection: .security_and_analysis.secret_scanning_push_protection.status}'
gh api "repos/$REPO/private-vulnerability-reporting" --jq .enabled
```

Treat missing fields/API failures as unverified, not enabled. Keep the approved LinkedIn private-message route and its access limitations in the policies independently of GitHub setup. Do not change visibility to unlock reporting. Confirm the **Report a vulnerability** button on the actual repository's Security → Advisories page when applicable, without filing a fake report. Add a GitHub reporting route to `SECURITY.md` only after confirming availability and monitoring. Conduct reports remain private messages to the approved contact, not security advisories. Do not invent another contact, infer consent from Git history, or direct sensitive details into public issues.

## Publication handoff

Keep the target/authorization confirmation, effective protections, current three-platform run conclusions, enabled/unsupported controls and monitored reporting-route confirmation in the owner's private operational record. Public readiness notes should contain only sanitized outcomes and unresolved blockers. Resolve the source/history privacy review and dependency/font/icon/bundled-asset license review before an owner-approved publication. A clean scanner report and green CI are not guarantees that history contains no personal information, or that installers work.

The preparation did not perform any of these external actions, publish refs or certify a platform. After the owner separately approves publication, publish only the explicitly reviewed source branch/refs through their normal process, never a broad `--all` or mirror push.

## References

- [GitHub: protected branches and required check sources](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)
- [GitHub: secret scanning availability](https://docs.github.com/en/code-security/concepts/secret-security/secret-scanning)
- [GitHub: configure private vulnerability reporting and notifications](https://docs.github.com/en/code-security/how-tos/report-and-fix-vulnerabilities/configure-vulnerability-reporting/configure-for-a-repository)
- Upstream action contracts: [checkout](https://github.com/actions/checkout), [setup-node](https://github.com/actions/setup-node), [upload-artifact](https://github.com/actions/upload-artifact), [Rust toolchain](https://github.com/dtolnay/rust-toolchain).
