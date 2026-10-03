<!-- All PR text and attachments are public. Never attach diagnostic DBs/journals,
workspace files, raw logs/stack dumps, local paths, secrets, environment/config payloads,
private repository contents or private product/account identifiers. Follow SECURITY.md
for suspected vulnerabilities; an ordinary PR is not a private reporting route. -->

## Purpose and agreed scope

Link the public issue or describe the maintainer-agreed request and observable acceptance criteria. No Auto-K account or private planning access is required.

## Change

Summarize the affected behavior, boundaries, callers and documentation. Explain any intentional scope limits, without claiming unfinished work is complete.

## Verification evidence

- Exact commands run and sanitized outcomes:
- Actual application/CLI action exercised and observed result:
- OS/architecture and source revision relevant to that observation:
- Failed, blocked, skipped or unverified checks/scenarios and reasons:

Include `npm run verify` evidence or explain why it could not run. Tests alone are not smoke proof. Browser/mock results do not prove native-picker interaction, packaged installers, signing or release readiness. Review any screenshot for private names and personal data before attaching it.

## Licensing and bundled material

List added/updated dependencies, fonts, icons, copied code or other assets, their upstream source/version and license. Describe required notice/provenance changes and unresolved permissions; write “none” only when no such material changed. Include the outcome of `node scripts/check-licenses.mjs` when applicable.

## Checklist

- [ ] The change matches the public maintainer-approved scope (or is a focused bug/documentation fix).
- [ ] Affected callers, behavioral tests, contracts and documentation are updated or intentionally unchanged with a reason.
- [ ] I exercised the changed path and reported outcomes and limits honestly.
- [ ] Required shared verification passed, or every failed/blocked/skipped check is explicitly reported above.
- [ ] I have the right to submit my original contributions under GPL-3.0-only; no CLA or copyright transfer is required.
- [ ] Third-party copyright/license notices and asset provenance are preserved; THIRD_PARTY_NOTICES.md and licenses/inventory.json are updated where applicable.
- [ ] Dependencies, fonts, icons and bundled assets have been reviewed for redistribution/compatibility, or unresolved findings are explicitly disclosed rather than waived.
- [ ] Text, code, evidence and attachments contain no secrets, private data, diagnostic databases, raw log dumps or raw local paths.
- [ ] For agent/concurrent work, the mandatory AGENTS.md session gate and file-ownership/integration contract were followed.
