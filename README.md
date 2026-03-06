# symphony-rs

Rust implementation of the Symphony service orchestration specification.

## What It Provides

- Tracker polling and issue dispatch orchestration
- Per-issue workspace lifecycle and hook execution
- Codex app-server session runner integration
- Axum HTTP observability API
- Leptos SSR dashboard with live browser refresh controls

## Key Repository Contracts

- `WORKFLOW.md` defines runtime policy and prompt contract.
- The repository ships a development-safe root `WORKFLOW.md` so `cargo leptos watch` boots the dashboard from the project root without extra setup; replace the tracker settings there when pointing the app at a real Linear workspace.
- `SPEC.md` mirrors upstream Symphony spec plus repository addendum.
- `AGENTS.md` defines contributor and coding-agent operating rules.
- Rust test suites run with `cargo nextest run --workspace`.
- `e2e/` acceptance tests are executed with Bun + Playwright.

## Development Workflow

- Use `nix develop` for the project shell; it provisions the `wasm32-unknown-unknown` target and ensures the exact `wasm-bindgen-cli` version pinned in `flake.nix` is installed under `./.cargo-tools/` before Leptos builds run.
- `cargo leptos watch` builds `crates/symphony-app` as an SSR binary plus a hydrated browser bundle, matching the repository's Leptos SSR + hydration contract.
- The shipped dashboard serves `/pkg/*` assets from the app server and exposes a hydrated `Refresh dashboard` control that re-renders visible runtime state without a full page reload.
