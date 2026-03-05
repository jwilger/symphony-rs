import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";

type MaybeString = string | null;

type MockIssue = {
  id: string;
  identifier: string;
  title: string;
  description: MaybeString;
  priority: number | null;
  state: string;
  branchName: MaybeString;
  url: MaybeString;
  labels: string[];
  blockedBy: Array<{ id: MaybeString; identifier: MaybeString; state: MaybeString }>;
  createdAt: string;
  updatedAt: string;
};

type HarnessOptions = {
  issues: MockIssue[];
  stateRefreshSequenceByIssueId?: Record<string, string[]>;
  codexTurnDelayMs?: number;
  codexMode?: "success" | "turn-failed" | "turn-cancelled" | "input-required" | "stall" | "unsupported-tool";
  codexCommandOverride?: string;
  pollIntervalMs?: number;
  appPort?: number;
  maxConcurrentAgents?: number;
  maxTurns?: number;
  maxRetryBackoffMs?: number;
  turnTimeoutMs?: number;
  readTimeoutMs?: number;
  stallTimeoutMs?: number;
  maxConcurrentAgentsByState?: Record<string, number>;
  activeStates?: string[];
  terminalStates?: string[];
  hooks?: {
    afterCreate?: string;
    beforeRun?: string;
    afterRun?: string;
    beforeRemove?: string;
    timeoutMs?: number;
  };
  precreateWorkspaceIssueIdentifiers?: string[];
  trackerFailurePlan?: {
    candidateFailuresBeforeSuccess?: number;
    issueStateFailuresBeforeSuccess?: number;
  };
  useDefaultWorkflowPath?: boolean;
};

type HarnessState = {
  issueStatesById: Map<string, MockIssue>;
  stateRefreshSequenceByIssueId: Map<string, string[]>;
  candidateFailuresRemaining: number;
  issueStateFailuresRemaining: number;
};

type SymphonyHarness = {
  appBaseUrl: string;
  workflowPath: string;
  workspaceRoot: string;
  logs: string[];
  triggerRefresh: () => Promise<void>;
  getState: () => Promise<any>;
  stop: () => Promise<void>;
};

const E2E_ROOT = resolve(fileURLToPath(new URL("../../", import.meta.url)));
const REPO_ROOT = resolve(E2E_ROOT, "..");
const CARGO_MANIFEST_PATH = join(REPO_ROOT, "Cargo.toml");
const FAKE_CODEX_PATH = join(E2E_ROOT, "fixtures", "fake-codex-app-server.mjs");

async function readJsonBody(request: IncomingMessage): Promise<any> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
  }
  const raw = Buffer.concat(chunks).toString("utf8");
  if (raw.trim().length === 0) {
    return {};
  }
  return JSON.parse(raw);
}

function toIssueNode(issue: MockIssue): any {
  return {
    id: issue.id,
    identifier: issue.identifier,
    title: issue.title,
    description: issue.description,
    priority: issue.priority,
    branchName: issue.branchName,
    url: issue.url,
    createdAt: issue.createdAt,
    updatedAt: issue.updatedAt,
    state: { name: issue.state },
    labels: {
      nodes: issue.labels.map((name) => ({ name })),
    },
    inverseRelations: {
      nodes: issue.blockedBy.map((blocker) => ({
        type: "blocks",
        relatedIssue: {
          id: blocker.id,
          identifier: blocker.identifier,
          state: blocker.state ? { name: blocker.state } : null,
        },
      })),
    },
  };
}

async function startMockLinearServer(options: HarnessOptions): Promise<{
  endpoint: string;
  stop: () => Promise<void>;
}> {
  const state: HarnessState = {
    issueStatesById: new Map(options.issues.map((issue) => [issue.id, { ...issue }])),
    stateRefreshSequenceByIssueId: new Map(
      Object.entries(options.stateRefreshSequenceByIssueId ?? {}).map(([issueId, values]) => [
        issueId,
        [...values],
      ]),
    ),
    candidateFailuresRemaining: Math.max(0, options.trackerFailurePlan?.candidateFailuresBeforeSuccess ?? 0),
    issueStateFailuresRemaining: Math.max(0, options.trackerFailurePlan?.issueStateFailuresBeforeSuccess ?? 0),
  };

  const server = createServer(async (request: IncomingMessage, response: ServerResponse) => {
    if (request.method !== "POST") {
      response.writeHead(405, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: "method_not_allowed" }));
      return;
    }

    try {
      const body = await readJsonBody(request);
      const query = String(body.query ?? "");
      const variables = (body.variables ?? {}) as Record<string, any>;

      if (query.includes("IssueStatesByIds")) {
        if (state.issueStateFailuresRemaining > 0) {
          state.issueStateFailuresRemaining -= 1;
          response.writeHead(500, { "content-type": "application/json" });
          response.end(JSON.stringify({ errors: [{ message: "mock issue-state refresh failure" }] }));
          return;
        }

        const ids = Array.isArray(variables.ids) ? variables.ids.map(String) : [];
        for (const id of ids) {
          const issue = state.issueStatesById.get(id);
          if (!issue) {
            continue;
          }
          const sequence = state.stateRefreshSequenceByIssueId.get(id);
          if (sequence && sequence.length > 0) {
            issue.state = sequence.shift()!;
            issue.updatedAt = new Date().toISOString();
          }
        }

        const nodes = ids
          .map((id) => state.issueStatesById.get(id))
          .filter((issue): issue is MockIssue => issue !== undefined)
          .map((issue) => ({
            id: issue.id,
            identifier: issue.identifier,
            title: issue.title,
            state: { name: issue.state },
            priority: issue.priority,
            createdAt: issue.createdAt,
            updatedAt: issue.updatedAt,
          }));

        response.writeHead(200, { "content-type": "application/json" });
        response.end(JSON.stringify({ data: { issues: { nodes } } }));
        return;
      }

      if (query.includes("CandidateIssues")) {
        if (state.candidateFailuresRemaining > 0) {
          state.candidateFailuresRemaining -= 1;
          response.writeHead(500, { "content-type": "application/json" });
          response.end(JSON.stringify({ errors: [{ message: "mock candidate fetch failure" }] }));
          return;
        }

        const stateNames = Array.isArray(variables.stateNames)
          ? variables.stateNames.map((value: unknown) => String(value))
          : [];
        const normalized = new Set(stateNames.map((value) => value.trim().toLowerCase()));
        const nodes = Array.from(state.issueStatesById.values())
          .filter((issue) => normalized.has(issue.state.trim().toLowerCase()))
          .map(toIssueNode);

        response.writeHead(200, { "content-type": "application/json" });
        response.end(
          JSON.stringify({
            data: {
              issues: {
                pageInfo: {
                  hasNextPage: false,
                  endCursor: null,
                },
                nodes,
              },
            },
          }),
        );
        return;
      }

      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ data: {} }));
    } catch (error) {
      response.writeHead(500, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          errors: [{ message: String(error) }],
        }),
      );
    }
  });

  const listenPort = await new Promise<number>((resolvePort, rejectPort) => {
    server.once("error", rejectPort);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        rejectPort(new Error("failed to bind linear mock server"));
        return;
      }
      resolvePort(address.port);
    });
  });

  return {
    endpoint: `http://127.0.0.1:${listenPort}/graphql`,
    stop: async () => {
      await new Promise<void>((resolveStop, rejectStop) => {
        server.close((error) => {
          if (error) {
            rejectStop(error);
            return;
          }
          resolveStop();
        });
      });
    },
  };
}

async function writeWorkflowFile(args: {
  path: string;
  workspaceRoot: string;
  linearEndpoint: string;
  appPort: number;
  codexTurnDelayMs: number;
  codexMode: HarnessOptions["codexMode"];
  codexCommandOverride?: string;
  pollIntervalMs: number;
  maxConcurrentAgents: number;
  maxTurns: number;
  maxRetryBackoffMs: number;
  turnTimeoutMs: number;
  readTimeoutMs: number;
  stallTimeoutMs: number;
  maxConcurrentAgentsByState: Record<string, number>;
  activeStates: string[];
  terminalStates: string[];
  hooks: Required<NonNullable<HarnessOptions["hooks"]>>;
}): Promise<void> {
  const codexMode = args.codexMode ?? "success";
  const codexCommand = args.codexCommandOverride
    ? args.codexCommandOverride
    : [
        "bun",
        FAKE_CODEX_PATH,
        "--delay-ms",
        String(args.codexTurnDelayMs),
        "--mode",
        codexMode,
      ].join(" ");

  const activeStates = args.activeStates.join(", ");
  const terminalStates = args.terminalStates.join(", ");
  const maxConcurrentByState =
    Object.keys(args.maxConcurrentAgentsByState).length === 0
      ? "{}"
      : JSON.stringify(args.maxConcurrentAgentsByState);

  const template = `---
tracker:
  kind: linear
  api_key: $LINEAR_API_KEY
  endpoint: ${args.linearEndpoint}
  project_slug: DEMO
  active_states: ${activeStates}
  terminal_states: ${terminalStates}
polling:
  interval_ms: ${args.pollIntervalMs}
workspace:
  root: ${args.workspaceRoot}
hooks:
  after_create: |
${indentMultiline(args.hooks.afterCreate)}
  before_run: |
${indentMultiline(args.hooks.beforeRun)}
  after_run: |
${indentMultiline(args.hooks.afterRun)}
  before_remove: |
${indentMultiline(args.hooks.beforeRemove)}
  timeout_ms: ${args.hooks.timeoutMs}
agent:
  max_concurrent_agents: ${args.maxConcurrentAgents}
  max_turns: ${args.maxTurns}
  max_retry_backoff_ms: ${args.maxRetryBackoffMs}
  max_concurrent_agents_by_state: ${maxConcurrentByState}
codex:
  command: ${codexCommand}
  approval_policy: never
  thread_sandbox: danger-full-access
  turn_sandbox_policy:
    type: dangerFullAccess
  turn_timeout_ms: ${args.turnTimeoutMs}
  read_timeout_ms: ${args.readTimeoutMs}
  stall_timeout_ms: ${args.stallTimeoutMs}
server:
  port: ${args.appPort}
---
You are working on {{ issue.identifier }}: {{ issue.title }}.
`;

  await writeFile(args.path, template, "utf8");
}

async function waitFor(
  predicate: () => Promise<boolean>,
  timeoutMs: number,
  errorMessage: string,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) {
      return;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(errorMessage);
}

export async function startSymphonyHarness(options: HarnessOptions): Promise<SymphonyHarness> {
  const tempRoot = await mkdtemp(join(tmpdir(), "symphony-rs-e2e-"));
  const workspaceRoot = join(tempRoot, "workspaces");
  const workflowPath = join(tempRoot, "WORKFLOW.md");

  const linearServer = await startMockLinearServer(options);
  const appPort = options.appPort ?? 4173;
  const hookDefaults = {
    afterCreate: "true",
    beforeRun: "true",
    afterRun: "true",
    beforeRemove: "true",
    timeoutMs: 5000,
  };

  await writeWorkflowFile({
    path: workflowPath,
    workspaceRoot,
    linearEndpoint: linearServer.endpoint,
    appPort,
    codexTurnDelayMs: options.codexTurnDelayMs ?? 100,
    codexMode: options.codexMode ?? "success",
    codexCommandOverride: options.codexCommandOverride,
    pollIntervalMs: options.pollIntervalMs ?? 120_000,
    maxConcurrentAgents: options.maxConcurrentAgents ?? 2,
    maxTurns: options.maxTurns ?? 3,
    maxRetryBackoffMs: options.maxRetryBackoffMs ?? 120_000,
    turnTimeoutMs: options.turnTimeoutMs ?? 60_000,
    readTimeoutMs: options.readTimeoutMs ?? 5_000,
    stallTimeoutMs: options.stallTimeoutMs ?? 30_000,
    maxConcurrentAgentsByState: options.maxConcurrentAgentsByState ?? {},
    activeStates: options.activeStates ?? ["Todo", "In Progress"],
    terminalStates: options.terminalStates ?? ["Closed", "Cancelled", "Canceled", "Duplicate", "Done"],
    hooks: {
      ...hookDefaults,
      ...(options.hooks ?? {}),
    },
  });

  for (const identifier of options.precreateWorkspaceIssueIdentifiers ?? []) {
    const workspacePath = join(workspaceRoot, sanitizeWorkspaceKey(identifier));
    await mkdir(workspacePath, { recursive: true });
    await writeFile(join(workspacePath, ".precreated"), "1\n", "utf8");
  }

  const baseArgs = ["run", "--manifest-path", CARGO_MANIFEST_PATH, "-p", "symphony-app", "--"];
  const args = options.useDefaultWorkflowPath
    ? [...baseArgs, "--port", String(appPort)]
    : [...baseArgs, workflowPath, "--port", String(appPort)];

  const logs: string[] = [];
  const appCwd = options.useDefaultWorkflowPath ? tempRoot : REPO_ROOT;
  const appProcess = spawn("cargo", args, {
    cwd: appCwd,
    env: {
      ...process.env,
      LINEAR_API_KEY: process.env.LINEAR_API_KEY ?? "e2e-token",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  appProcess.stdout?.on("data", (chunk: Buffer) => {
    const rendered = chunk.toString("utf8");
    logs.push(rendered);
    process.stdout.write(rendered);
  });
  appProcess.stderr?.on("data", (chunk: Buffer) => {
    const rendered = chunk.toString("utf8");
    logs.push(rendered);
    process.stderr.write(rendered);
  });

  const appBaseUrl = `http://127.0.0.1:${appPort}`;

  await waitFor(
    async () => {
      try {
        const response = await fetch(`${appBaseUrl}/api/v1/state`);
        return response.ok;
      } catch {
        return false;
      }
    },
    120_000,
    "timed out waiting for symphony app to start",
  );

  return {
    appBaseUrl,
    workflowPath,
    workspaceRoot,
    logs,
    triggerRefresh: async () => {
      const response = await fetch(`${appBaseUrl}/api/v1/refresh`, { method: "POST" });
      if (!response.ok) {
        throw new Error(`refresh failed status=${response.status}`);
      }
    },
    getState: async () => {
      const response = await fetch(`${appBaseUrl}/api/v1/state`);
      if (!response.ok) {
        throw new Error(`state request failed status=${response.status}`);
      }
      return response.json();
    },
    stop: async () => {
      await stopChildProcess(appProcess);
      await linearServer.stop();
      await rm(tempRoot, { recursive: true, force: true });
    },
  };
}

function sanitizeWorkspaceKey(identifier: string): string {
  const sanitized = identifier
    .split("")
    .map((character) => (/^[A-Za-z0-9._-]$/.test(character) ? character : "_"))
    .join("");
  return sanitized.length > 0 ? sanitized : "issue";
}

function indentMultiline(script: string): string {
  return script
    .split("\n")
    .map((line) => `    ${line}`)
    .join("\n");
}

async function stopChildProcess(child: ChildProcess): Promise<void> {
  if (child.killed) {
    return;
  }

  child.kill("SIGTERM");
  await waitForProcessExit(child, 5_000);
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }

  child.kill("SIGKILL");
  await waitForProcessExit(child, 5_000);
}

async function waitForProcessExit(child: ChildProcess, timeoutMs: number): Promise<void> {
  await Promise.race([
    new Promise<void>((resolveExit) => {
      child.once("exit", () => resolveExit());
    }),
    new Promise<void>((resolveTimeout) => {
      setTimeout(resolveTimeout, timeoutMs);
    }),
  ]);
}

export async function waitForStateCondition(
  appBaseUrl: string,
  condition: (state: any) => boolean,
  timeoutMs: number,
  message: string,
): Promise<any> {
  let latest: any = null;
  await waitFor(
    async () => {
      const response = await fetch(`${appBaseUrl}/api/v1/state`);
      if (!response.ok) {
        return false;
      }
      latest = await response.json();
      return condition(latest);
    },
    timeoutMs,
    message,
  );
  return latest;
}

export async function loadText(path: string): Promise<string> {
  return readFile(path, "utf8");
}
