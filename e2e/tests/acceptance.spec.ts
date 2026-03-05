import { expect, test } from "@playwright/test";
import { access, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  loadText,
  startSymphonyHarness,
  waitForStateCondition,
} from "./helpers/symphony-harness";

function buildIssue(args: {
  id: string;
  identifier: string;
  state: string;
  blockedBy?: Array<{ id: string | null; identifier: string | null; state: string | null }>;
  priority?: number | null;
  createdAt?: string;
  updatedAt?: string;
}): {
  id: string;
  identifier: string;
  title: string;
  description: string;
  priority: number | null;
  state: string;
  branchName: string;
  url: string;
  labels: string[];
  blockedBy: Array<{ id: string | null; identifier: string | null; state: string | null }>;
  createdAt: string;
  updatedAt: string;
} {
  const now = new Date().toISOString();
  const createdAt = args.createdAt ?? now;
  const updatedAt = args.updatedAt ?? now;
  return {
    id: args.id,
    identifier: args.identifier,
    title: `Issue ${args.identifier}`,
    description: "Acceptance scenario issue",
    priority: args.priority ?? 1,
    state: args.state,
    branchName: `feature/${args.identifier.toLowerCase()}`,
    url: `https://linear.example/${args.identifier}`,
    labels: ["acceptance"],
    blockedBy: args.blockedBy ?? [],
    createdAt,
    updatedAt,
  };
}

test.describe("Given an idle Symphony service", () => {
  let harness: Awaited<ReturnType<typeof startSymphonyHarness>> | null = null;

  test.beforeAll(async () => {
    harness = await startSymphonyHarness({
      issues: [],
    });
  });

  test.afterAll(async () => {
    if (!harness) {
      return;
    }
    await harness.stop();
    harness = null;
  });

  test("Given idle state When requesting dashboard Then SSR runtime heading is visible", async ({ page }) => {
    const service = harness!;

    await page.goto(`${service.appBaseUrl}/`);
    await expect(page.getByRole("heading", { name: "Symphony Runtime" })).toBeVisible();
    await expect(page.getByText("Running Sessions")).toBeVisible();
  });

  test("Given idle state When requesting /api/v1/state Then response follows baseline runtime schema", async ({ request }) => {
    const service = harness!;

    const response = await request.get(`${service.appBaseUrl}/api/v1/state`);
    expect(response.ok()).toBeTruthy();
    const payload = await response.json();

    expect(payload).toEqual(
      expect.objectContaining({
        generated_at: expect.any(String),
        counts: expect.objectContaining({
          running: expect.any(Number),
          retrying: expect.any(Number),
        }),
        running: expect.any(Array),
        retrying: expect.any(Array),
        codex_totals: expect.objectContaining({
          input_tokens: expect.any(Number),
          output_tokens: expect.any(Number),
          total_tokens: expect.any(Number),
          seconds_running: expect.any(Number),
        }),
      }),
    );
  });

  test("Given idle state When posting /api/v1/refresh Then request is accepted and poll/reconcile are queued", async ({ request }) => {
    const service = harness!;

    const response = await request.post(`${service.appBaseUrl}/api/v1/refresh`, {
      data: {},
    });
    expect(response.status()).toBe(202);

    const payload = await response.json();
    expect(payload).toEqual(
      expect.objectContaining({
        queued: true,
        coalesced: false,
        operations: expect.arrayContaining(["poll", "reconcile"]),
      }),
    );
  });

  test("Given unknown issue id When requesting issue details Then service returns issue_not_found envelope", async ({ request }) => {
    const service = harness!;

    const response = await request.get(`${service.appBaseUrl}/api/v1/UNKNOWN-9999`);
    expect(response.status()).toBe(404);
    expect(await response.json()).toEqual(
      expect.objectContaining({
        error: expect.objectContaining({
          code: "issue_not_found",
        }),
      }),
    );
  });

  test("Given defined API routes When using unsupported HTTP methods Then service returns method not allowed", async ({ request }) => {
    const service = harness!;

    const refreshGet = await request.get(`${service.appBaseUrl}/api/v1/refresh`);
    expect(refreshGet.status()).toBe(405);

    const statePost = await request.post(`${service.appBaseUrl}/api/v1/state`, { data: {} });
    expect(statePost.status()).toBe(405);

    const issuePost = await request.post(`${service.appBaseUrl}/api/v1/UNKNOWN-9999`, { data: {} });
    expect(issuePost.status()).toBe(405);
  });

  test("Given valid running config When WORKFLOW.md reload is invalid Then service keeps last known good config active", async ({ request }) => {
    const service = harness!;
    const originalWorkflow = await loadText(service.workflowPath);
    try {
      await writeFile(service.workflowPath, "---\ntracker:\n  kind: linear\n  active_states: [\n", "utf8");

      const recovered = await waitForStateCondition(
        service.appBaseUrl,
        (state) => typeof state.generated_at === "string",
        10_000,
        "service did not remain available after invalid workflow reload",
      );
      expect(recovered.counts).toEqual(
        expect.objectContaining({
          running: expect.any(Number),
          retrying: expect.any(Number),
        }),
      );

      const response = await request.get(`${service.appBaseUrl}/api/v1/state`);
      expect(response.ok()).toBeTruthy();
    } finally {
      await writeFile(service.workflowPath, originalWorkflow, "utf8");
    }
  });
});

test.describe("Given a dispatchable active issue", () => {
  let harness: Awaited<ReturnType<typeof startSymphonyHarness>> | null = null;
  const issueIdentifier = "MT-649";

  test.beforeAll(async () => {
    const now = new Date().toISOString();
    harness = await startSymphonyHarness({
      issues: [
        {
          id: "issue-1",
          identifier: issueIdentifier,
          title: "Implement acceptance coverage",
          description: "Work item for orchestration tests",
          priority: 1,
          state: "In Progress",
          branchName: "feature/mt-649",
          url: "https://linear.example/MT-649",
          labels: ["backend", "automation"],
          blockedBy: [],
          createdAt: now,
          updatedAt: now,
        },
      ],
      stateRefreshSequenceByIssueId: {
        "issue-1": ["Done"],
      },
      codexTurnDelayMs: 5000,
      pollIntervalMs: 120_000,
    });
  });

  test.afterAll(async () => {
    if (!harness) {
      return;
    }
    await harness.stop();
    harness = null;
  });

  test("Given queued active work When initial startup tick runs Then issue is dispatched and visible in running and issue-debug APIs", async ({ request }) => {
    const service = harness!;
    const trackedState = await waitForStateCondition(
      service.appBaseUrl,
      (state) => state.running.some((item: any) => item.issue_identifier === issueIdentifier),
      30_000,
      "issue was not tracked after startup dispatch",
    );

    const runningRow = trackedState.running.find(
      (item: any) => item.issue_identifier === issueIdentifier,
    );
    expect(runningRow).toEqual(
      expect.objectContaining({
        issue_identifier: issueIdentifier,
        turn_count: expect.any(Number),
      }),
    );
    if (runningRow?.session_id !== null && runningRow?.session_id !== undefined) {
      expect(typeof runningRow.session_id).toBe("string");
    }

    const issueResponse = await request.get(`${service.appBaseUrl}/api/v1/${issueIdentifier}`);
    expect(issueResponse.ok()).toBeTruthy();
    const issuePayload = await issueResponse.json();

    expect(issuePayload).toEqual(
      expect.objectContaining({
        issue_identifier: issueIdentifier,
        status: "running",
        issue_id: "issue-1",
        workspace: expect.objectContaining({
          path: expect.stringContaining(`${service.workspaceRoot}/${issueIdentifier}`),
        }),
        attempts: expect.objectContaining({
          restart_count: expect.any(Number),
          current_retry_attempt: expect.any(Number),
        }),
        running: expect.objectContaining({
          state: "In Progress",
          turn_count: expect.any(Number),
          tokens: expect.objectContaining({
            input_tokens: expect.any(Number),
            output_tokens: expect.any(Number),
            total_tokens: expect.any(Number),
          }),
        }),
      }),
    );
  });

  test("Given an active codex turn When token and rate-limit events are emitted Then aggregated telemetry is exposed via state API", async () => {
    const service = harness!;
    const telemetryState = await waitForStateCondition(
      service.appBaseUrl,
      (state) =>
        state.codex_totals.total_tokens > 0 &&
        state.codex_totals.input_tokens > 0 &&
        state.codex_totals.output_tokens > 0 &&
        state.rate_limits !== null,
      30_000,
      "codex telemetry was not surfaced in runtime snapshot",
    );

    expect(telemetryState.codex_totals.total_tokens).toBeGreaterThan(0);
    expect(telemetryState.rate_limits).toEqual(
      expect.objectContaining({
        requests_remaining: expect.any(Number),
      }),
    );
  });

  test("Given worker completion and continuation retry When issue becomes non-active Then retry queue drains and claim is released", async ({ request }) => {
    const service = harness!;

    await waitForStateCondition(
      service.appBaseUrl,
      (state) => state.counts.running === 0 && state.counts.retrying === 0,
      40_000,
      "retry queue did not drain after continuation cycle",
    );

    const issueResponse = await request.get(`${service.appBaseUrl}/api/v1/${issueIdentifier}`);
    expect(issueResponse.status()).toBe(404);
  });

  test("Given successful run completion When checking workspace filesystem Then per-issue workspace persists", async () => {
    const service = harness!;
    const workspacePath = join(service.workspaceRoot, issueIdentifier);
    await access(workspacePath);
  });
});

test.describe("Given dispatch eligibility and concurrency constraints", () => {
  test("Given non-active issue state When refresh is triggered Then issue is not dispatched", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-na", identifier: "NA-1", state: "Backlog" })],
    });
    try {
      await harness.triggerRefresh();
      const state = await harness.getState();
      expect(state.counts.running).toBe(0);
      expect(state.counts.retrying).toBe(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given Todo issue blocked by non-terminal blocker When refresh is triggered Then issue is not dispatched", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({
          id: "issue-blocked-active",
          identifier: "BLK-1",
          state: "Todo",
          blockedBy: [{ id: "dep-1", identifier: "DEP-1", state: "In Progress" }],
        }),
      ],
    });
    try {
      await harness.triggerRefresh();
      const state = await harness.getState();
      expect(state.counts.running).toBe(0);
      expect(state.counts.retrying).toBe(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given Todo issue blocked by terminal blocker When refresh is triggered Then issue becomes dispatchable", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({
          id: "issue-blocked-terminal",
          identifier: "BLK-2",
          state: "Todo",
          blockedBy: [{ id: "dep-2", identifier: "DEP-2", state: "Done" }],
        }),
      ],
      codexTurnDelayMs: 2000,
      stateRefreshSequenceByIssueId: {
        "issue-blocked-terminal": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "BLK-2"),
        30_000,
        "terminally-unblocked todo issue did not dispatch",
      );
      expect(state.counts.running).toBeGreaterThanOrEqual(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given two active issues and max concurrency one When refresh is triggered Then only one issue runs at a time", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({ id: "issue-c1", identifier: "CON-1", state: "In Progress", priority: 1 }),
        buildIssue({ id: "issue-c2", identifier: "CON-2", state: "In Progress", priority: 2 }),
      ],
      maxConcurrentAgents: 1,
      codexTurnDelayMs: 3000,
      stateRefreshSequenceByIssueId: {
        "issue-c1": ["Done"],
        "issue-c2": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      const running = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 1,
        30_000,
        "global concurrency limit was not enforced",
      );
      expect(running.counts.running).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given per-state concurrency limit When multiple same-state issues are available Then state cap is enforced", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({ id: "issue-s1", identifier: "ST-1", state: "In Progress", priority: 1 }),
        buildIssue({ id: "issue-s2", identifier: "ST-2", state: "In Progress", priority: 2 }),
      ],
      maxConcurrentAgents: 2,
      maxConcurrentAgentsByState: {
        "in progress": 1,
      },
      codexTurnDelayMs: 3000,
      stateRefreshSequenceByIssueId: {
        "issue-s1": ["Done"],
        "issue-s2": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      const running = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 1,
        30_000,
        "per-state concurrency limit was not enforced",
      );
      expect(running.counts.running).toBe(1);
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given dispatch ordering rules", () => {
  test("Given different priorities When only one slot is available Then lowest numeric priority dispatches first", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({ id: "issue-order-1", identifier: "ORD-1", state: "In Progress", priority: 2 }),
        buildIssue({ id: "issue-order-2", identifier: "ORD-2", state: "In Progress", priority: 1 }),
      ],
      maxConcurrentAgents: 1,
      codexTurnDelayMs: 6000,
    });
    try {
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 1,
        20_000,
        "priority ordering scenario did not dispatch a running issue",
      );
      expect(state.running[0].issue_identifier).toBe("ORD-2");
    } finally {
      await harness.stop();
    }
  });

  test("Given equal priorities with different creation times When only one slot is available Then oldest issue dispatches first", async () => {
    const newer = new Date("2026-03-05T18:00:10.000Z").toISOString();
    const older = new Date("2026-03-05T18:00:00.000Z").toISOString();
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({
          id: "issue-created-new",
          identifier: "CRT-2",
          state: "In Progress",
          priority: 1,
          createdAt: newer,
          updatedAt: newer,
        }),
        buildIssue({
          id: "issue-created-old",
          identifier: "CRT-1",
          state: "In Progress",
          priority: 1,
          createdAt: older,
          updatedAt: older,
        }),
      ],
      maxConcurrentAgents: 1,
      codexTurnDelayMs: 6000,
    });
    try {
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 1,
        20_000,
        "created_at ordering scenario did not dispatch a running issue",
      );
      expect(state.running[0].issue_identifier).toBe("CRT-1");
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given codex failure and timeout modes", () => {
  test("Given turn failure mode When refresh dispatches run Then retry queue records turn_failed error", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-fail", identifier: "FAIL-1", state: "In Progress" })],
      codexMode: "turn-failed",
      codexTurnDelayMs: 200,
    });
    try {
      await harness.triggerRefresh();
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "FAIL-1"),
        20_000,
        "failed turn did not schedule retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "FAIL-1");
      expect(row.error).toContain("turn_failed");
    } finally {
      await harness.stop();
    }
  });

  test("Given turn cancelled mode When refresh dispatches run Then retry queue records turn_cancelled error", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-cancel", identifier: "CNL-1", state: "In Progress" })],
      codexMode: "turn-cancelled",
      codexTurnDelayMs: 200,
    });
    try {
      await harness.triggerRefresh();
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "CNL-1"),
        20_000,
        "cancelled turn did not schedule retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "CNL-1");
      expect(row.error).toContain("turn_cancelled");
    } finally {
      await harness.stop();
    }
  });

  test("Given input-required mode When refresh dispatches run Then attempt fails with turn_input_required", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-input", identifier: "INP-1", state: "In Progress" })],
      codexMode: "input-required",
      codexTurnDelayMs: 100,
    });
    try {
      await harness.triggerRefresh();
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "INP-1"),
        20_000,
        "input-required did not fail attempt",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "INP-1");
      expect(String(row.error)).toBe("turn_input_required");
    } finally {
      await harness.stop();
    }
  });

  test("Given stalled codex mode and short turn timeout When refresh dispatches run Then timeout retry is produced", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-timeout", identifier: "TMO-1", state: "In Progress" })],
      codexMode: "stall",
      turnTimeoutMs: 1200,
      readTimeoutMs: 500,
      codexTurnDelayMs: 100,
    });
    try {
      await harness.triggerRefresh();
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "TMO-1"),
        20_000,
        "turn timeout did not schedule retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "TMO-1");
      expect(String(row.error)).toBe("turn_timeout");
    } finally {
      await harness.stop();
    }
  });

  test("Given an invalid codex command When issue dispatches Then attempt fails with codex_not_found", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-codex-missing", identifier: "CDEX-1", state: "In Progress" })],
      codexCommandOverride: "this-command-does-not-exist-xyz",
    });
    try {
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "CDEX-1"),
        20_000,
        "invalid codex command did not produce retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "CDEX-1");
      expect(
        String(row.error).includes("codex_not_found") || String(row.error).includes("port_exit"),
      ).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });

  test("Given unsupported tool call mode When startup dispatches run Then session continues and emits telemetry", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-tool", identifier: "TOOL-1", state: "In Progress" })],
      codexMode: "unsupported-tool",
      codexTurnDelayMs: 300,
      stateRefreshSequenceByIssueId: {
        "issue-tool": ["Done"],
      },
    });
    try {
      const telemetry = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.codex_totals.total_tokens > 0,
        20_000,
        "unsupported tool flow did not continue turn processing",
      );
      expect(telemetry.codex_totals.total_tokens).toBeGreaterThan(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given approval-required mode When startup dispatches run Then the session auto-approves and continues", async ({ request }) => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-approval", identifier: "APR-1", state: "In Progress" })],
      codexMode: "approval-required",
      codexTurnDelayMs: 250,
      maxTurns: 2,
      stateRefreshSequenceByIssueId: {
        "issue-approval": ["In Progress", "Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.codex_totals.total_tokens > 0 &&
          payload.running.some((row: any) => row.issue_identifier === "APR-1"),
        20_000,
        "approval-required flow did not continue",
      );

      const issueResponse = await request.get(`${harness.appBaseUrl}/api/v1/APR-1`);
      expect(issueResponse.ok()).toBeTruthy();
      const issuePayload = await issueResponse.json();
      expect(
        issuePayload.recent_events.some((event: any) => event.event === "approval_auto_approved"),
      ).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given multi-turn continuation semantics", () => {
  test("Given active issue remains active across refreshes When max_turns allows continuation Then running turn_count increments within a single worker run", async () => {
    const issueIdentifier = "TURN-1";
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-turns", identifier: issueIdentifier, state: "In Progress" })],
      maxTurns: 3,
      codexTurnDelayMs: 500,
      stateRefreshSequenceByIssueId: {
        "issue-turns": ["In Progress", "In Progress", "Done"],
      },
    });
    try {
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.running.some(
            (row: any) => row.issue_identifier === issueIdentifier && Number(row.turn_count) >= 2,
          ),
        20_000,
        "turn_count did not increment for continuation turns",
      );
      const row = state.running.find((item: any) => item.issue_identifier === issueIdentifier);
      expect(row.turn_count).toBeGreaterThanOrEqual(2);
      expect(typeof row.session_id === "string" || row.session_id === null).toBeTruthy();

      const transcript = JSON.parse(await loadText(harness.codexTranscriptPath));
      expect(Array.isArray(transcript.turnInputs)).toBeTruthy();
      expect(transcript.turnInputs.length).toBeGreaterThanOrEqual(2);
      expect(transcript.turnInputs[0].prompt).toBe(
        `You are working on ${issueIdentifier}: Issue ${issueIdentifier}.`,
      );
      expect(transcript.turnInputs[0].title).toBe(`${issueIdentifier}: Issue ${issueIdentifier}`);
      for (const turnInput of transcript.turnInputs.slice(1)) {
        expect(turnInput.prompt).toBe(
          "Continue from prior thread context and make the next concrete progress step on this issue.",
        );
      }
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given workspace and hook policies", () => {
  test("Given two worker attempts for the same issue When after_create hook writes a marker Then marker is written once", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-ac", identifier: "HOOK-1", state: "In Progress" })],
      maxTurns: 1,
      codexTurnDelayMs: 200,
      stateRefreshSequenceByIssueId: {
        "issue-ac": ["In Progress", "Paused"],
      },
      hooks: {
        afterCreate: "echo created >> .after_create_count",
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.running.some((row: any) => row.issue_identifier === "HOOK-1") ||
          payload.retrying.some((row: any) => row.issue_identifier === "HOOK-1"),
        20_000,
        "hook scenario did not start",
      );
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0 && payload.counts.retrying === 0,
        40_000,
        "hook scenario did not settle",
      );

      const markerPath = join(harness.workspaceRoot, "HOOK-1", ".after_create_count");
      const markerContents = await readFile(markerPath, "utf8");
      const count = markerContents
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line === "created").length;
      expect(count).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given failing before_run hook When issue dispatches Then attempt fails and retry is scheduled", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-br", identifier: "HOOK-2", state: "In Progress" })],
      hooks: {
        beforeRun: "exit 12",
      },
    });
    try {
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "HOOK-2"),
        20_000,
        "before_run failure did not schedule retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "HOOK-2");
      expect(String(row.error)).toContain("before_run");
    } finally {
      await harness.stop();
    }
  });

  test("Given timed-out before_run hook When issue dispatches Then attempt fails with hook-timeout error", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-br-timeout", identifier: "HOOK-2B", state: "In Progress" })],
      hooks: {
        beforeRun: "sleep 2",
        timeoutMs: 200,
      },
    });
    try {
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "HOOK-2B"),
        20_000,
        "timed-out before_run did not schedule retry",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "HOOK-2B");
      expect(String(row.error)).toContain("before_run hook timeout");
    } finally {
      await harness.stop();
    }
  });

  test("Given failing after_run hook When run completes Then failure is logged and service remains healthy", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-ar", identifier: "HOOK-3", state: "In Progress" })],
      codexTurnDelayMs: 200,
      stateRefreshSequenceByIssueId: {
        "issue-ar": ["Done"],
      },
      hooks: {
        afterRun: "exit 13",
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0,
        30_000,
        "after_run scenario did not complete",
      );
      const state = await harness.getState();
      expect(state).toHaveProperty("generated_at");
    } finally {
      await harness.stop();
    }
  });

  test("Given identifier with path traversal characters When workspace is prepared Then workspace path stays contained under root", async () => {
    const identifier = "../evil/MT-777";
    const sanitized = identifier
      .trim()
      .split("")
      .map((character) => (/^[A-Za-z0-9._-]$/.test(character) ? character : "_"))
      .join("");

    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-safe", identifier, state: "In Progress" })],
      codexTurnDelayMs: 1200,
      stateRefreshSequenceByIssueId: {
        "issue-safe": ["Done"],
      },
    });

    try {
      await harness.triggerRefresh();
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running >= 1,
        20_000,
        "containment scenario did not start running",
      );

      const expectedWorkspacePath = join(harness.workspaceRoot, sanitized || "issue");
      await access(expectedWorkspacePath);
      expect(expectedWorkspacePath.startsWith(harness.workspaceRoot)).toBeTruthy();
      expect(expectedWorkspacePath.endsWith(sanitized || "issue")).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given retry timing and reconciliation transitions", () => {
  test("Given normal completion When continuation retry is queued Then retry due time is short (~1s)", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-cont", identifier: "RET-1", state: "In Progress" })],
      maxTurns: 1,
      codexTurnDelayMs: 200,
      stateRefreshSequenceByIssueId: {
        "issue-cont": ["In Progress", "Paused"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "RET-1"),
        20_000,
        "continuation scenario did not start running",
      );
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "RET-1"),
        20_000,
        "continuation retry row did not appear",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "RET-1");
      const dueAtMs = Date.parse(row.due_at);
      const deltaMs = dueAtMs - Date.now();
      expect(deltaMs).toBeLessThanOrEqual(2_500);
      expect(deltaMs).toBeGreaterThanOrEqual(-500);
    } finally {
      await harness.stop();
    }
  });

  test("Given failure completion When retry is queued Then retry due time reflects exponential failure backoff", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-backoff", identifier: "RET-2", state: "In Progress" })],
      codexMode: "turn-failed",
      codexTurnDelayMs: 100,
      maxRetryBackoffMs: 60_000,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "RET-2"),
        20_000,
        "failure retry row did not appear",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "RET-2");
      const dueAtMs = Date.parse(row.due_at);
      const deltaMs = dueAtMs - Date.now();
      expect(deltaMs).toBeGreaterThan(7_000);
      expect(deltaMs).toBeLessThanOrEqual(12_000);
    } finally {
      await harness.stop();
    }
  });

  test("Given retry due with no available orchestrator slots When retry handler runs Then issue is requeued with slot-exhaustion error", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({ id: "issue-slot-a", identifier: "RET-SLOT-A", state: "In Progress", priority: 1 }),
        buildIssue({ id: "issue-slot-b", identifier: "RET-SLOT-B", state: "In Progress", priority: 2 }),
      ],
      maxConcurrentAgents: 1,
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      hooks: {
        beforeRun: 'if [ "$(basename "$PWD")" = "RET-SLOT-A" ]; then exit 15; fi',
      },
    });
    try {
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.retrying.some(
            (row: any) =>
              row.issue_identifier === "RET-SLOT-A" &&
              Number(row.attempt) >= 2 &&
              String(row.error).includes("no available orchestrator slots"),
          ),
        35_000,
        "slot-exhaustion retry requeue did not occur",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "RET-SLOT-A");
      expect(Number(row.attempt)).toBeGreaterThanOrEqual(2);
      expect(String(row.error)).toContain("no available orchestrator slots");
    } finally {
      await harness.stop();
    }
  });

  test("Given stall timeout disabled When codex emits no activity Then reconciliation does not kill the running worker", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-stall-disabled", identifier: "STALL-0", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 0,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "STALL-0"),
        20_000,
        "stall-disabled scenario did not start running",
      );

      await new Promise((resolveWait) => setTimeout(resolveWait, 2200));
      const state = await harness.getState();
      expect(state.running.some((row: any) => row.issue_identifier === "STALL-0")).toBeTruthy();
      expect(state.retrying.some((row: any) => row.issue_identifier === "STALL-0")).toBeFalsy();
    } finally {
      await harness.stop();
    }
  });

  test("Given stalled worker and short stall timeout When reconcile ticks Then run is stopped and stalled retry is queued", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-stall", identifier: "STALL-1", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 300,
      stallTimeoutMs: 700,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "STALL-1"),
        20_000,
        "stalled run did not produce retry row",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "STALL-1");
      expect(String(row.error)).toContain("stalled");
    } finally {
      await harness.stop();
    }
  });

  test("Given terminal transition observed by reconciliation When running issue is stopped Then workspace is cleaned even if before_remove fails", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-term", identifier: "REC-1", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
      hooks: {
        beforeRemove: "exit 21",
      },
      stateRefreshSequenceByIssueId: {
        "issue-term": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.counts.running === 0 &&
          !payload.retrying.some((row: any) => row.issue_identifier === "REC-1"),
        20_000,
        "terminal reconciliation did not stop run",
      );
      const workspacePath = join(harness.workspaceRoot, "REC-1");
      let removed = false;
      try {
        await access(workspacePath);
      } catch {
        removed = true;
      }
      expect(removed).toBeTruthy();
      expect(harness.logs.join("")).toContain("before_remove hook failed");
    } finally {
      await harness.stop();
    }
  });

  test("Given terminal transition and timed-out before_remove hook When reconciliation cleans workspace Then cleanup still proceeds", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-term-timeout", identifier: "REC-1B", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
      hooks: {
        beforeRemove: "sleep 2",
        timeoutMs: 200,
      },
      stateRefreshSequenceByIssueId: {
        "issue-term-timeout": ["Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "REC-1B"),
        20_000,
        "terminal transition timeout scenario did not start running",
      );
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0,
        20_000,
        "terminal transition with before_remove timeout did not complete",
      );
      const workspacePath = join(harness.workspaceRoot, "REC-1B");
      const removed = await (async () => {
        const deadline = Date.now() + 5000;
        while (Date.now() < deadline) {
          try {
            await access(workspacePath);
          } catch {
            return true;
          }
          await new Promise((resolveWait) => setTimeout(resolveWait, 100));
        }
        return false;
      })();
      expect(removed).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });

  test("Given non-terminal non-active transition When reconciliation stops run Then workspace is preserved", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-nonactive", identifier: "REC-2", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
      stateRefreshSequenceByIssueId: {
        "issue-nonactive": ["Paused"],
      },
    });
    try {
      await harness.triggerRefresh();
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0,
        20_000,
        "non-active reconciliation did not stop running issue",
      );
      const workspacePath = join(harness.workspaceRoot, "REC-2");
      await access(workspacePath);
    } finally {
      await harness.stop();
    }
  });

  test("Given issue-state refresh errors When reconciliation runs Then service keeps workers running and retries later", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-refresh-fail", identifier: "REC-3", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
      trackerFailurePlan: {
        issueStateFailuresBeforeSuccess: 2,
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "REC-3"),
        20_000,
        "state-refresh error scenario did not start running",
      );
      await new Promise((resolveWait) => setTimeout(resolveWait, 1200));
      const state = await harness.getState();
      expect(state.running.some((row: any) => row.issue_identifier === "REC-3")).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given workflow reload changes", () => {
  test("Given active state filter excludes issue initially When workflow reload expands active states Then issue dispatches after refresh", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-reload", identifier: "CFG-1", state: "In Progress" })],
      activeStates: ["Todo"],
      codexTurnDelayMs: 2000,
      stateRefreshSequenceByIssueId: {
        "issue-reload": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      const before = await harness.getState();
      expect(before.counts.running).toBe(0);

      const original = await loadText(harness.workflowPath);
      const updated = original.replace("active_states: Todo", "active_states: Todo, In Progress");
      await writeFile(harness.workflowPath, updated, "utf8");

      await waitForStateCondition(
        harness.appBaseUrl,
        () => harness.logs.join("").includes("workflow reloaded"),
        10_000,
        "workflow reload was not observed",
      );
      await harness.triggerRefresh();

      const after = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "CFG-1"),
        20_000,
        "reloaded active state config did not dispatch issue",
      );
      expect(after.counts.running).toBeGreaterThanOrEqual(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given max_concurrent_agents is increased by workflow reload When refresh is triggered Then additional issue dispatch capacity is applied", async () => {
    const now = new Date().toISOString();
    const harness = await startSymphonyHarness({
      issues: [
        {
          id: "issue-cap-1",
          identifier: "CFG-2A",
          title: "Capacity one",
          description: "Reload capacity scenario",
          priority: 1,
          state: "In Progress",
          branchName: "feature/cfg-2a",
          url: "https://linear.example/CFG-2A",
          labels: ["acceptance"],
          blockedBy: [],
          createdAt: now,
          updatedAt: now,
        },
        {
          id: "issue-cap-2",
          identifier: "CFG-2B",
          title: "Capacity two",
          description: "Reload capacity scenario",
          priority: 2,
          state: "In Progress",
          branchName: "feature/cfg-2b",
          url: "https://linear.example/CFG-2B",
          labels: ["acceptance"],
          blockedBy: [],
          createdAt: now,
          updatedAt: now,
        },
      ],
      maxConcurrentAgents: 1,
      codexTurnDelayMs: 6000,
      stateRefreshSequenceByIssueId: {
        "issue-cap-1": ["In Progress", "Done"],
        "issue-cap-2": ["In Progress", "Done"],
      },
    });
    try {
      const baseline = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 1,
        20_000,
        "baseline concurrency scenario did not reach one running issue",
      );
      expect(baseline.counts.running).toBe(1);

      const original = await loadText(harness.workflowPath);
      const updated = original.replace("max_concurrent_agents: 1", "max_concurrent_agents: 2");
      await writeFile(harness.workflowPath, updated, "utf8");

      await waitForStateCondition(
        harness.appBaseUrl,
        () => harness.logs.join("").includes("workflow reloaded"),
        10_000,
        "workflow reload for capacity update was not observed",
      );
      await harness.triggerRefresh();

      const expanded = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running >= 2,
        20_000,
        "updated max_concurrent_agents did not increase running capacity",
      );
      expect(expanded.counts.running).toBeGreaterThanOrEqual(2);
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given runtime observability semantics", () => {
  test("Given a long-running turn When querying state over time Then aggregate runtime seconds increase", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-runtime", identifier: "OBS-1", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
    });
    try {
      await harness.triggerRefresh();
      const first = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running >= 1,
        20_000,
        "runtime scenario did not reach running state",
      );
      const firstSeconds = first.codex_totals.seconds_running;

      await new Promise((resolveWait) => setTimeout(resolveWait, 2200));
      const second = await harness.getState();
      expect(second.codex_totals.seconds_running).toBeGreaterThan(firstSeconds);
    } finally {
      await harness.stop();
    }
  });
});

test.describe("Given workflow path selection and startup behavior", () => {
  test("Given cwd contains WORKFLOW.md When app starts without explicit path Then startup succeeds", async ({ request }) => {
    const harness = await startSymphonyHarness({
      issues: [],
      useDefaultWorkflowPath: true,
    });
    try {
      const response = await request.get(`${harness.appBaseUrl}/api/v1/state`);
      expect(response.ok()).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });

  test("Given missing default WORKFLOW.md When app starts without explicit path Then startup exits nonzero", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "symphony-rs-missing-workflow-"));
    try {
      const manifestPath = join(process.cwd(), "..", "Cargo.toml");
      const child = spawn("cargo", ["run", "--manifest-path", manifestPath, "-p", "symphony-app", "--"], {
        cwd: tempDir,
        env: {
          ...process.env,
          LINEAR_API_KEY: process.env.LINEAR_API_KEY ?? "e2e-token",
        },
        stdio: ["ignore", "pipe", "pipe"],
      });

      const exitCode = await new Promise<number | null>((resolveExit) => {
        child.once("exit", (code) => resolveExit(code));
      });

      expect(exitCode).not.toBe(0);
    } finally {
      await rm(tempDir, { recursive: true, force: true });
    }
  });

  test("Given invalid dispatch config at startup When app starts with explicit workflow path Then startup exits nonzero", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "symphony-rs-invalid-workflow-"));
    const workflowPath = join(tempDir, "WORKFLOW.md");
    const workflow = `---
tracker:
  kind: linear
  api_key: $LINEAR_API_KEY
polling:
  interval_ms: 30000
codex:
  command: codex app-server
---
You are working on {{ issue.identifier }}.
`;
    await writeFile(workflowPath, workflow, "utf8");

    try {
      const manifestPath = join(process.cwd(), "..", "Cargo.toml");
      const child = spawn(
        "cargo",
        ["run", "--manifest-path", manifestPath, "-p", "symphony-app", "--", workflowPath],
        {
          cwd: tempDir,
          env: {
            ...process.env,
            LINEAR_API_KEY: process.env.LINEAR_API_KEY ?? "e2e-token",
          },
          stdio: ["ignore", "pipe", "pipe"],
        },
      );

      const exitCode = await new Promise<number | null>((resolveExit) => {
        child.once("exit", (code) => resolveExit(code));
      });

      expect(exitCode).not.toBe(0);
    } finally {
      await rm(tempDir, { recursive: true, force: true });
    }
  });
});

test.describe("Given startup cleanup behavior", () => {
  test("Given terminal issue workspace exists at startup When terminal cleanup runs Then workspace is removed before steady state", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-terminal", identifier: "TERM-1", state: "Done" })],
      precreateWorkspaceIssueIdentifiers: ["TERM-1"],
    });
    try {
      const workspacePath = join(harness.workspaceRoot, "TERM-1");
      let removed = false;
      try {
        await access(workspacePath);
      } catch {
        removed = true;
      }
      expect(removed).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });

  test("Given startup terminal cleanup tracker failure When service starts Then startup continues with warning", async ({ request }) => {
    const harness = await startSymphonyHarness({
      issues: [],
      trackerFailurePlan: {
        candidateFailuresBeforeSuccess: 1,
      },
    });
    try {
      const response = await request.get(`${harness.appBaseUrl}/api/v1/state`);
      expect(response.ok()).toBeTruthy();
      expect(harness.logs.join("")).toContain("startup_terminal_workspace_cleanup failed; continuing startup");
    } finally {
      await harness.stop();
    }
  });
});
