# symphony-rs

Rust implementation of the Symphony service orchestration specification.

## What It Provides

- Tracker polling and issue dispatch orchestration
- Per-issue workspace lifecycle and hook execution
- Codex app-server session runner integration
- Axum HTTP observability API
- Leptos SSR dashboard with hydration wiring

## Key Repository Contracts

- `WORKFLOW.md` defines runtime policy and prompt contract.
- `SPEC.md` mirrors upstream Symphony spec plus repository addendum.
- `AGENTS.md` defines contributor and coding-agent operating rules.
- Rust test suites run with `cargo nextest run --workspace`.
- `e2e/` acceptance tests are executed with Bun + Playwright.

## Development Workflow

- Use `nix develop` for the project shell; it provisions the `wasm32-unknown-unknown` target and ensures the exact `wasm-bindgen-cli` version pinned in `flake.nix` is installed under `./.cargo-tools/` before Leptos builds run.
- `cargo leptos watch` builds `crates/symphony-app` as an SSR binary plus a hydrated browser bundle, matching the repository's Leptos SSR + hydration contract.
