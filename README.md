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
- `e2e/` acceptance tests are executed with Bun + Playwright.
