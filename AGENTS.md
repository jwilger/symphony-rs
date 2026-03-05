# AGENTS.md

## Required Skill

- Always use the `memory-protocol` skill for tasks in this project.

## Architectural Rules

1. Functional core / imperative shell.
- Keep orchestration policy and state transitions in pure functions.
- Keep IO in effectful adapters.

2. Parse-don't-validate.
- Parse boundary input into semantic types at the boundary.

3. Semantic domain types over primitives.
- Do not use primitive/structural types internally unless unavoidable at IO boundaries.

4. Private-by-default visibility.
- Do not make items `pub`/`pub(crate)` unless required by compiler/API boundaries.

## Stack Direction

- Build runtime services with `Tokio` + `Axum`.
- Build dashboard surfaces with `Leptos` SSR + hydration.
- Keep `cargo-leptos` workflow metadata and build paths aligned with the crate.

## Dependency Management

- Manage dependencies via the appropriate CLI tooling only (for Rust crates, use `cargo add` /
  `cargo rm` / equivalent CLI workflows).
- Manage JavaScript dependencies via `bun` CLI workflows.
- Avoid hand-editing dependency manifests for add/remove/version operations.

## Code Quality Policy

1. Treat all compiler/linter/clippy warnings as errors.
2. Keep crate-root lint groups at the strictest practical level (`forbid` where compatible;
   `deny` where third-party proc macros inject incompatible `allow(...)` attributes).
3. Keep `clippy::multiple_crate_versions` enforced as `deny`; manage unavoidable ecosystem
   duplicates through explicit `clippy.toml` `allowed-duplicate-crates` entries rather than
   in-code `allow(...)`.

## Testing Policy

1. All user-facing features must be covered by Playwright acceptance tests.
2. Mutation testing (`cargo-mutants`) is required with target full kill-rate for repository gates.
3. Property tests are required for all custom validation/parsing logic on domain types.

## Documentation Sync Policy

- Keep `SPEC.md` up to date with upstream plus repository addendum.
- Update in-repo documentation when behavior/architecture/quality rules change.
- Documentation synchronization is enforced by pre-commit checks.
