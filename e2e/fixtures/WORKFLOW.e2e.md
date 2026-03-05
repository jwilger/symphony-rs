---
tracker:
  kind: linear
  api_key: $LINEAR_API_KEY
  endpoint: http://127.0.0.1:9/graphql
  project_slug: DEMO
  active_states: Todo, In Progress
  terminal_states: Closed, Cancelled, Canceled, Duplicate, Done
polling:
  interval_ms: 120000
workspace:
  root: ./target/e2e-workspaces
hooks:
  timeout_ms: 5000
agent:
  max_concurrent_agents: 2
  max_turns: 2
  max_retry_backoff_ms: 120000
codex:
  command: codex app-server
  approval_policy: never
  thread_sandbox: danger-full-access
  turn_sandbox_policy:
    type: dangerFullAccess
  turn_timeout_ms: 60000
  read_timeout_ms: 5000
  stall_timeout_ms: 30000
server:
  port: 4173
---
You are working on {{ issue.identifier }}: {{ issue.title }}.
