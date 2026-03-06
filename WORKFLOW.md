---
tracker:
  kind: linear
  api_key: development-placeholder-api-key
  endpoint: http://127.0.0.1:9/graphql
  project_slug: DEMO
polling:
  interval_ms: 300000
agent:
  max_concurrent_agents: 1
server:
  port: 3000
---
You are working on {{ issue.identifier }}: {{ issue.title }}.

Review the issue description, inspect the current implementation, and make the next concrete progress step.
