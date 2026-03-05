# Symphony SPEC Acceptance Matrix (Playwright GWT)

Status: Pass 3 (Validated)
Date: 2026-03-05

Purpose:
- Map `SPEC.md` requirements to black-box acceptance scenarios.
- Keep a repeatable "am I done yet?" gate for runtime/user-facing behavior.
- Require review/fix/review/fix passes before final validation.

Legend:
- `VALIDATED`: Scenario passed in a full-suite run.

## Orchestrator and Runtime Control

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| ORCH-001 | 8.1 | Service performs reconcile then dispatch on tick | Given refresh trigger, when `POST /api/v1/refresh`, then state reflects poll+reconcile processing | VALIDATED |
| ORCH-002 | 8.2 | Dispatch eligibility requires active non-terminal state | Given non-active issue, when refresh, then no running/retrying row appears | VALIDATED |
| ORCH-003 | 8.2 | `Todo` with non-terminal blockers is not dispatchable | Given Todo issue blocked by active blocker, when refresh, then issue is not dispatched | VALIDATED |
| ORCH-004 | 8.2 | `Todo` with terminal blockers is dispatchable | Given Todo issue blocked by Done blocker, when refresh, then issue dispatches | VALIDATED |
| ORCH-005 | 8.3 | Global concurrency limit enforced | Given 2 active issues and global cap=1, when dispatch runs, then running count is 1 | VALIDATED |
| ORCH-006 | 8.3 | Per-state concurrency limit enforced | Given per-state cap for `in progress`, when multiple same-state issues exist, then state cap is enforced | VALIDATED |
| ORCH-007 | 8.2 | Dispatch sorting honors priority | Given two issues with different priority, when one slot is available, then lower numeric priority dispatches first | VALIDATED |
| ORCH-008 | 8.2 | Dispatch sorting honors `created_at` tie-break | Given equal priority and one slot, oldest issue dispatches first | VALIDATED |
| ORCH-009 | 8.4 | Continuation retry uses short delay | Given normal completion with active issue, continuation retry row appears with short due time | VALIDATED |
| ORCH-010 | 8.4 | Failure retries use exponential backoff | Given failing turn, retry due delay reflects first failure backoff window | VALIDATED |
| ORCH-011 | 8.4 | Slot exhaustion requeues retries with explicit error | Given retry due while slot is occupied, issue is requeued with `no available orchestrator slots` | VALIDATED |
| ORCH-012 | 8.4 | Retry release when issue no longer eligible | Given continuation completes and issue becomes non-active, retry drains and issue endpoint returns 404 | VALIDATED |
| ORCH-013 | 8.5 | Stall timeout stops stalled run and retries | Given stalled codex mode and short stall timeout, run is stopped and retry row includes stalled error | VALIDATED |
| ORCH-014 | 8.5 | Stall detection can be disabled | Given `stall_timeout_ms <= 0`, stalled worker remains running and is not retried by stall logic | VALIDATED |
| ORCH-015 | 8.5 | Terminal reconciliation stops run and cleans workspace | Given running issue transitions to terminal, worker stops and workspace is removed | VALIDATED |
| ORCH-016 | 8.5 | Non-active non-terminal reconciliation stops without cleanup | Given running issue transitions to non-active/non-terminal, worker stops and workspace remains | VALIDATED |
| ORCH-017 | 8.5 | State refresh errors keep workers running | Given issue-state refresh transport failures, reconciliation keeps active workers running | VALIDATED |
| ORCH-018 | 8.6 | Startup terminal cleanup removes stale workspaces | Given pre-existing workspace for terminal issue at startup, workspace is removed | VALIDATED |
| ORCH-019 | 8.6 | Startup cleanup failure is non-fatal | Given startup terminal-fetch failure, service still starts and state API stays available | VALIDATED |

## Workflow and Config

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| CFG-001 | 5.1, 17.7 | Explicit workflow path works | Given explicit workflow path, service starts and state API responds | VALIDATED |
| CFG-002 | 5.1, 17.7 | Default `./WORKFLOW.md` path works | Given cwd contains `WORKFLOW.md`, startup without explicit path succeeds | VALIDATED |
| CFG-003 | 5.1, 17.7 | Missing default workflow fails startup | Given missing `./WORKFLOW.md`, startup exits nonzero | VALIDATED |
| CFG-004 | 6.2 | Invalid reload keeps last-known-good config | Given invalid workflow edit, service remains healthy and state API remains available | VALIDATED |
| CFG-005 | 6.2 | Reload can update active-state behavior | Given active-state list expanded on reload, previously ineligible issue dispatches | VALIDATED |
| CFG-006 | 6.2 | Reload can update concurrency behavior | Given max concurrency increased on reload, running capacity increases | VALIDATED |
| CFG-007 | 6.3 | Startup dispatch preflight validation enforced | Given invalid dispatch config (missing project slug), explicit-path startup exits nonzero | VALIDATED |

## Workspace and Hooks

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| WS-001 | 9.1, 9.2 | Deterministic per-issue workspace path under root | Given dispatched issue, workspace path exists at `<root>/<sanitized_identifier>` | VALIDATED |
| WS-002 | 9.2 | Existing workspace is reused across attempts | Given continuation attempts for same issue, same workspace directory is reused | VALIDATED |
| WS-003 | 9.4 | `after_create` runs only on first creation | Given marker-writing `after_create` and multi-attempt issue lifecycle, marker is written once | VALIDATED |
| WS-004 | 9.4 | `before_run` failure aborts attempt | Given failing `before_run`, attempt fails and retry is scheduled | VALIDATED |
| WS-005 | 9.4 | `before_run` timeout aborts attempt | Given timed-out `before_run`, attempt fails with hook-timeout error | VALIDATED |
| WS-006 | 9.4 | `after_run` failure is best-effort | Given failing `after_run`, run path continues and service remains healthy | VALIDATED |
| WS-007 | 9.4 | `before_remove` failure is best-effort | Given failing `before_remove` on terminal cleanup, workspace is still removed | VALIDATED |
| WS-008 | 9.4 | `before_remove` timeout is best-effort | Given timed-out `before_remove`, cleanup still removes workspace | VALIDATED |
| WS-009 | 9.5 | Workspace containment and sanitization invariant enforced | Given identifier with traversal characters, realized workspace path stays under root and uses sanitized key | VALIDATED |

## Codex App-Server Integration

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| CX-001 | 10.2 | Startup handshake succeeds and session metadata appears | Given successful fake app-server mode, running row exposes session metadata | VALIDATED |
| CX-002 | 10.3 | `turn/completed` maps to normal path | Given successful mode, run follows normal completion path | VALIDATED |
| CX-003 | 10.3 | `turn/failed` maps to failure retry | Given turn-failed mode, retry row records failure | VALIDATED |
| CX-004 | 10.3 | `turn/cancelled` maps to failure retry | Given turn-cancelled mode, retry row records cancelled failure | VALIDATED |
| CX-005 | 10.5 | User-input-required maps to hard failure | Given input-required mode, attempt fails with `turn_input_required` retry error | VALIDATED |
| CX-006 | 10.5 | Unsupported tool call does not stall session | Given unsupported tool-call mode, run still completes and telemetry is emitted | VALIDATED |
| CX-007 | 10.6 | Turn timeout is enforced | Given stalled mode and short turn timeout, retry row reports timeout | VALIDATED |
| CX-008 | 10.6 | Startup/command failure is surfaced as codex failure class | Given invalid codex command, retry row error surfaces startup/transport failure category | VALIDATED |
| CX-009 | 7.1, 10.3 | Multi-turn continuation increments turn count | Given active issue across turn refreshes and `max_turns > 1`, running row `turn_count` increments | VALIDATED |

## HTTP Extension and API Contract

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| API-001 | 13.7.1 | Dashboard at `/` renders runtime view | Given running service, dashboard SSR heading and sections render | VALIDATED |
| API-002 | 13.7.2 | `GET /api/v1/state` returns baseline runtime schema | Given running service, state payload includes required summary fields and types | VALIDATED |
| API-003 | 13.7.2 | `GET /api/v1/{issue_identifier}` returns issue debug view | Given tracked issue, issue endpoint returns running/retry debug payload | VALIDATED |
| API-004 | 13.7.2 | Unknown issue returns `404` envelope | Given unknown identifier, endpoint returns `issue_not_found` error envelope | VALIDATED |
| API-005 | 13.7.2 | `POST /api/v1/refresh` returns accepted payload | Given refresh request, endpoint returns `202` with operations list | VALIDATED |
| API-006 | 13.7.2 | Unsupported methods return `405` | Given unsupported HTTP methods on API routes, responses are `405` | VALIDATED |

## Observability and Runtime Semantics

| ID | SPEC Ref | Requirement | Scenario | Status |
|---|---|---|---|---|
| OBS-001 | 13.5 | Token and rate-limit totals are aggregated | Given token/rate-limit events, state snapshot exposes non-zero token totals and rate-limit payload | VALIDATED |
| OBS-002 | 13.5 | Runtime seconds include live running sessions | Given long-running active worker, `seconds_running` increases over time | VALIDATED |
| OBS-003 | 13.5 | Running rows include `turn_count` | Given multi-turn continuation, running row exposes incrementing `turn_count` | VALIDATED |

## Pass 3 Summary

- Validated scenarios: 46
- Pending scenarios in this matrix: 0
- Validation evidence:
  - `cd e2e && bun run test:e2e tests/acceptance.spec.ts --reporter=line --workers=1`
  - Result: `46 passed`
