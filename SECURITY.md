# Security policy

## Scope and support

GitView is a source developer preview. Security reports concerning the current source and its dependencies are relevant, particularly native Git/subprocess execution, filesystem and symlink boundaries, Tauri permissions/IPC, diagnostic privacy, and dependency or bundled-asset supply chains. There is no supported stable-release series or guaranteed security response time declared here. A source build or passing verification does not certify packaged native installers or public-release readiness.

## Report a vulnerability privately

Send a **private LinkedIn message** to [Tobias Candela](https://www.linkedin.com/in/tobiascandela/), the repository owner's approved contact. Mention “GitView security report” and start with a non-sensitive summary so a suitable private evidence exchange can be agreed.

Do not post vulnerability details, exploits, credentials, personal data or private repository contents in public issues, discussions, PRs, LinkedIn posts or profile comments. Ordinary issue templates are not a security-reporting channel. Use the linked profile rather than deriving a contact from Git author identities or private account configuration.

LinkedIn may require an account, a connection or another messaging option offered by the platform. Anonymous or unrestricted messaging access is not promised. If private messaging is unavailable, do not substitute a public disclosure. No target GitHub repository or GitHub private vulnerability-reporting setting has been verified; this policy does not imply either has been configured.

## What a private report should include

Send only the information necessary to investigate:

- Affected source revision or dependency version, OS/architecture and relevant tool versions.
- The affected boundary, expected behavior, observed impact and prerequisites for exploitation.
- Minimal reproducible steps using an owned disposable repository or synthetic fixture.
- A proposed mitigation, if known, and whether sensitive material has already been exposed.

Do not attach a user's diagnostic database/journal, workspace state, raw logs, local paths, credentials, environment/config payloads or private repository contents. If additional evidence is essential, agree on a bounded private transfer with the responsible maintainer first. Do not probe other users' systems or repositories without authorization.

## Coordinated handling

Maintainers should acknowledge and triage reports through the approved private route, coordinate investigation and disclosure with the reporter, and publish affected-version and remediation guidance when appropriate. No response deadline or bounty is promised. Do not publicly disclose a working exploit while private investigation/disclosure arrangements are unresolved.

If a credential is exposed, its owner must revoke or rotate it with the issuing service. Removing the public text or rewriting history does not make it safe. Do not paste the credential into another issue or report; describe the type and exposure boundary without reproducing it. Agents must not rotate credentials, rewrite history, grant access or disclose private contact information without explicit authorization.
