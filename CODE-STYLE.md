# GitView code conventions

Use these conventions for Rust, TypeScript/TSX, SCSS and their tests. Prefer code that states its intent over commentary that narrates its syntax. Existing UI design rules remain in [DESIGN-RULES.md](DESIGN-RULES.md).

## Comment structure

### 1. File or module header: purpose and boundary

Give each handwritten source module a short header explaining its responsibility. Add a boundary or invariant only when it prevents a likely mistake. Tiny primitives and entry points normally need one sentence; coordination and infrastructure modules may need a short paragraph. Do not copy the filename, author, date, change history or a dependency inventory into the header. Generated files are excluded.

Rust uses module documentation before imports:

```rust
//! Owns atomic workspace transitions and immutable renderer snapshots.
//!
//! Git probing happens in the application layer, outside the state lock.
//! Request generations reject superseded completions; snapshot revisions
//! order externally visible state changes.
```

TypeScript/TSX uses a leading documentation block:

```ts
/**
 * Coordinates workspace requests for the mounted screen.
 * Rust owns repository state; this hook applies ordered snapshots and
 * prevents superseded selection results from reaching the current view.
 */
```

SCSS uses a short `//` ownership comment. Keep global tokens, reusable primitives and feature styles with their respective owners.

### 2. Type and API documentation: caller-visible contracts

Use Rust `///` and TypeScript JSDoc immediately above the declaration. Explain semantics that types and names cannot express: identity, units, allowed states, side effects, ordering, cancellation, error meaning or ownership. Document exported domain/application contracts and tricky internal boundaries. Do not add boilerplate to obvious accessors, props or one-line wrappers.

Rust example:

```rust
/// Activates an admitted entry and invalidates its older availability requests.
///
/// Returns `NotFound` without changing state for an unknown ID.
/// This transition performs no Git or filesystem operations.
```

Use rustdoc `# Errors`, `# Panics` and `# Safety` sections only where applicable; `# Safety` is required for an unsafe API. TypeScript `@param`/`@returns` tags are optional and should explain a constraint, not repeat the signature. Keep declaration documentation before Rust attributes or derives.

### 3. Inline comments: rationale and invariants

Use `//` immediately before the relevant statement or small block. Explain **why**, not what. Important subjects here include stale-result guards, canonical native identity, shared deadlines, child-process reaping, virtualized focus and accessible descriptions.

```ts
// Keep backend selections in click order even when earlier IPC calls are slow.
```

```rust
// Reopening must invalidate an older refresh without replacing the entry ID.
```

A deliberate empty error handler must explain what state remains valid and why suppression is safe. Do not disguise a failure as success. Remove or update a comment in the same change that alters its invariant.

### 4. Sections: only when they improve navigation

Prefer named functions, types and modules over banner comments. If a long cohesive file still needs landmarks, use brief domain-oriented labels such as `// Native path decoding` or `// Keyboard focus`. Avoid numbered steps, ASCII borders and repeated `Imports`, `State`, `Functions` or `Render` headings. Rust `mod tests` and named tests already identify the test section.

### 5. Tests: explain the scenario, not every assertion

Name tests after observable behavior. Comment only surprising fixture setup, synchronization gates or the regression's causal sequence. Do not require `Arrange / Act / Assert` banners. Verify behavior, boundaries and failures—not comment text, source structure, forwarding or incidental wording. Use isolated fixtures; never mutate global environment variables in concurrently running tests.

Keep frontend test bodies in `tests/unit` or `tests/integration`, infrastructure tests in `tests/infrastructure`, and reusable support in `tests/support`. Keep Rust test bodies in `src-tauri/tests/unit` or `src-tauri/tests/integration` and shared fixtures in `src-tauri/tests/support`. Production Rust modules may retain test-only `#[path]` declarations for private child access; do not expose production internals to relocate tests. Tests using real Git, filesystem, SQLite or native boundaries belong to integration even when their private module is compiled by `cargo test --lib`.

Reuse isolated temporary fixtures where a real boundary matters. Each scenario owns its data and cleanup; no user's repositories, workspace file, diagnostic store or global environment. Keep platform-dependent scenarios explicitly gated without labeling their omission native-platform proof.

## Naming and function style

- Rust: `snake_case` functions/fields/modules, `PascalCase` types, `SCREAMING_SNAKE_CASE` constants. TypeScript: `camelCase` values/functions, `PascalCase` components/types, `use*` hooks.
- Name values by responsibility: distinguish a workspace revision, selection generation and entry request generation. Use generic names such as `result`, `next` or `index` only when their scope makes the meaning immediate.
- Use verbs for actions and nouns for facts. A type called a probe result should not appear to be the process executor. Prefer explicit domain words over `Manager`, `Helper`, `Data` and `Utils` buckets.
- Rename only when it resolves real ambiguity; migrate every caller and test in the same change. Do not keep deprecated aliases or forwarding shims.
- Keep one level of abstraction per function. Extract a named helper when it isolates a real decision, duplicated behavior or boundary—not simply to hit a line-count target.
- Prefer guard clauses and typed outcomes over nested branches or magic sentinels. Preserve intentional `null`/`Option` domain states such as no active selection.
- Keep related declarations together; entry points and public contracts should make the module's purpose discoverable before low-level helpers.
- Avoid unnecessary allocations, clones, conversions and repeated work in Rust. Do not introduce abstractions without a concrete consumer or boundary.
- Follow existing formatting and import conventions. Use language tooling for symbol-aware changes when available; never mix unrelated reformatting with a focused refactor.

## Architecture boundaries

| Owner | Owns | Must not own |
| --- | --- | --- |
| React bootstrap/screen | Mounting, composition, search and presentation | Native paths, Git execution or authoritative repository state |
| Workspace hook | Request ordering, current-view state and recoverable transport feedback | Repository eligibility or a second authoritative store |
| Frontend contracts | DTOs and the client interface | React or Tauri implementation dependencies |
| Platform adapter | Typed Tauri invocation | Rendering or Git interpretation |
| Shared UI | Generic rendering and local interaction | Repository-specific data fetching or policy |
| Tauri host | Picker, permissions, decoding and composition | Workspace policy or Git parsing |
| Application service | Probe orchestration and admission/refresh workflow | Holding state locks during native I/O |
| Workspace store | Atomic transitions, opaque IDs, generations and revisions | Filesystem access, processes or Tauri types |
| Git probe | Read-only repository facts and native identity | Workspace admission/selection policy |
| Process adapter | Fixed Git operations, output limits, deadlines and child lifecycle | Renderer-supplied commands or repository presentation |

Keep native paths authoritative only in Rust. Display labels are not filesystem identities. Availability is not a clean-worktree result. An older completion must not replace newer state or appear under another selected context. Preserve safe-directory protection, sanitized errors and the main-window command boundary.

`src/app` owns screen and workbench composition. Features expose curated `index.ts` APIs to app and cross-feature consumers; their implementation modules import siblings directly. Features never import app, and shared UI never imports app/features/contracts/platform. Renderer-only committed-selection and callback types belong to `features/diff/selection.ts`, not native wire DTOs: history consumes that public type-only API, diff may consume the changes observation API, and changes does not depend on diff.

Native `lib.rs` declares subsystem modules and exposes `run`; `host/` alone owns Tauri setup, commands and renderer diagnostics. `workspace/mod.rs` owns pure state transitions while `workspace/persistence.rs` owns disk persistence. Git execution/status, diff readers/parsers/committed review, history readers and browsing authority live under their respective subsystem directories. Keep existing cohesive service and diagnostic modules intact rather than creating generic layers or one-file folders.

## Repository verification and evidence

`npm run test:unit` and `npm run test:integration` run independently; `npm run verify` combines required checks through the same runner used by CI. Use direct Vitest/Cargo commands for focused local debugging. Verification artifacts belong in ignored `.verification/`; do not commit databases, raw test output, secrets or private repository payloads.

Native verification uses `.verification/native-target` independently of Tauri development and packaging targets. Keep that target in direct rerun commands and upload only sanitized manifests, never the build cache. Do not replace normal parallel checks with serial runs or stack-size overrides to hide failures.

Versioned `.githooks/pre-commit` runs policy plus unit verification; `.githooks/pre-push` runs the full shared verifier. `npm ci` installs them through `prepare` outside CI; `npm run hooks:install` installs explicitly. Never replace another `core.hooksPath` or active default hook without its owner's decision. Hooks do not stage, stash, auto-fix or commit source. They check the current worktree, not an isolated index or every pushed ref; CI still verifies the exact pushed revision, and partial staging requires review.

`npm run policy` enforces dedicated test placement, architecture imports and tracked-artifact exclusions. Correctness, fixture isolation, privacy allowlists, trace causality and actual native observations remain human-review obligations, not a machine policy pass.

The verification runner records explicit passed/failed/not-run states, sanitized behavior identifiers and revision/runtime context. A missing check, skipped required scenario or failed check cannot certify readiness. Browser and mocked IPC checks do not prove native picker, packaged artifacts or another OS. Native assessment validates an artifact-bound observation report; it does not independently certify observations, signing or release readiness.

## Change checklist

1. Read the owning module and its callers; preserve the supported contract.
2. Decide whether a clearer name or smaller function removes the need for a comment.
3. Document the remaining purpose, caller contract and non-obvious invariants.
4. Separate readability refactoring from behavioral fixes. Reproduce a bug before fixing it, and retain a focused behavior regression when practical.
5. Run affected checks and exercise the actual changed path. Do not add tests for comment presence or file layout.
6. Update ownership documentation when boundaries or public names change.
