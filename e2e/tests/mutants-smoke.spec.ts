import { expect, test } from "@playwright/test";
import { access, writeFile } from "node:fs/promises";
import { join } from "node:path";

import {
  loadText,
  startSymphonyHarness,
  waitForStateCondition,
} from "./helpers/symphony-harness";

const CONTINUATION_PROMPT =
  "Continue from prior thread context and make the next concrete progress step on this issue.";

function buildIssue(args: {
  id: string;
  identifier: string;
  state: string;
  priority?: number | null;
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
  return {
    id: args.id,
    identifier: args.identifier,
    title: `Issue ${args.identifier}`,
    description: "Mutation acceptance issue",
    priority: args.priority ?? 1,
    state: args.state,
    branchName: `feature/${args.identifier.toLowerCase()}`,
    url: `https://linear.example/${args.identifier}`,
    labels: ["acceptance"],
    blockedBy: [],
    createdAt: now,
    updatedAt: now,
  };
}

test.describe("Mutation smoke acceptance", () => {
  test("Given hydrated dashboard When runtime state changes Then refresh updates the browser without reload", async ({ page, request }) => {
    const harness = await startSymphonyHarness({
      requireHydratedDashboard: true,
      issues: [buildIssue({ id: "issue-mut-hydrate", identifier: "MUT-HYD-1", state: "In Progress" })],
      codexTurnDelayMs: 5_000,
      stateRefreshSequenceByIssueId: {
        "issue-mut-hydrate": ["Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "MUT-HYD-1"),
        20_000,
        "hydrated mutation scenario never reached running state",
      );

      const assetResponse = await request.get(`${harness.appBaseUrl}/pkg/symphony-app.js`);
      expect(assetResponse.ok()).toBeTruthy();

      await page.goto(`${harness.appBaseUrl}/`);
      await expect(page.getByText("MUT-HYD-1")).toBeVisible();
      await expect(page.getByTestId("dashboard-live-status")).toHaveText("Live dashboard ready");

      await harness.triggerRefresh();
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0,
        20_000,
        "server state did not drain in hydrated mutation scenario",
      );

      await expect(page.getByText("MUT-HYD-1")).toBeVisible();
      await page.getByRole("button", { name: "Refresh dashboard" }).click();
      await expect(page.getByRole("status")).toHaveText("Dashboard updated from live state");
      await expect(page.getByTestId("running-count")).toHaveText("0");
      await expect(page.getByText("MUT-HYD-1")).not.toBeVisible();
    } finally {
      await harness.stop();
    }
  });

  test("Given dispatchable issue When startup tick runs Then run lifecycle, issue API, and workspace persistence behave correctly", async ({
    request,
  }) => {
    const issueIdentifier = "MUT-1";
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-1", identifier: issueIdentifier, state: "In Progress" })],
      codexTurnDelayMs: 300,
      stateRefreshSequenceByIssueId: {
        "issue-mut-1": ["Done"],
      },
      pollIntervalMs: 120_000,
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === issueIdentifier),
        30_000,
        "running row not observed for dispatchable issue",
      );

      const issueResponse = await request.get(`${harness.appBaseUrl}/api/v1/${issueIdentifier}`);
      expect(issueResponse.ok()).toBeTruthy();
      const issuePayload = await issueResponse.json();
      expect(issuePayload.status).toBe("running");

      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0 && payload.counts.retrying === 0,
        30_000,
        "run lifecycle did not settle",
      );

      const settledIssueResponse = await request.get(`${harness.appBaseUrl}/api/v1/${issueIdentifier}`);
      expect(settledIssueResponse.status()).toBe(404);

      await access(join(harness.workspaceRoot, issueIdentifier));
    } finally {
      await harness.stop();
    }
  });

  test("Given input-required codex event When turn streams Then retry records the exact turn_input_required code", async ({ request }) => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-input", identifier: "MUT-2", state: "In Progress" })],
      codexMode: "input-required",
      codexTurnDelayMs: 100,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "MUT-2"),
        20_000,
        "input-required path did not create retry entry",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "MUT-2");
      expect(String(row.error)).toBe("turn_input_required");
      expect(Number(row.attempt)).toBe(1);

      const issueResponse = await request.get(`${harness.appBaseUrl}/api/v1/MUT-2`);
      expect(issueResponse.ok()).toBeTruthy();
      const issuePayload = await issueResponse.json();
      expect(issuePayload).toEqual(
        expect.objectContaining({
          issue_identifier: "MUT-2",
          status: "retrying",
          attempts: expect.objectContaining({
            current_retry_attempt: 1,
          }),
          retry: expect.objectContaining({
            error: "turn_input_required",
          }),
        }),
      );
    } finally {
      await harness.stop();
    }
  });

  test("Given turn timeout When codex emits no completion Then retry records the exact turn_timeout code", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-timeout", identifier: "MUT-3", state: "In Progress" })],
      codexMode: "stall",
      turnTimeoutMs: 1200,
      readTimeoutMs: 500,
      codexTurnDelayMs: 100,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "MUT-3"),
        20_000,
        "turn timeout did not create retry entry",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "MUT-3");
      expect(String(row.error)).toBe("turn_timeout");
      expect(Number(row.attempt)).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given turn cancellation When codex reports turn/cancelled Then retry records the exact turn_cancelled code", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-cancelled", identifier: "MUT-3B", state: "In Progress" })],
      codexMode: "turn-cancelled",
      codexTurnDelayMs: 150,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "MUT-3B"),
        20_000,
        "turn cancellation did not create retry entry",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "MUT-3B");
      expect(String(row.error)).toBe("turn_cancelled");
      expect(Number(row.attempt)).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given approval-required codex event When turn streams Then the session auto-approves and continues", async ({ request }) => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-approval", identifier: "MUT-4", state: "In Progress" })],
      codexMode: "approval-required",
      codexTurnDelayMs: 250,
      maxTurns: 2,
      stateRefreshSequenceByIssueId: {
        "issue-mut-approval": ["In Progress", "Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.codex_totals.total_tokens > 0 &&
          payload.running.some((row: any) => row.issue_identifier === "MUT-4"),
        20_000,
        "approval flow did not continue after auto-approval",
      );

      const issueResponse = await request.get(`${harness.appBaseUrl}/api/v1/MUT-4`);
      expect(issueResponse.ok()).toBeTruthy();
      const issuePayload = await issueResponse.json();
      expect(
        issuePayload.recent_events.some((event: any) => event.event === "approval_auto_approved"),
      ).toBeTruthy();
    } finally {
      await harness.stop();
    }
  });

  test("Given an id-bearing notification When turn streams Then it is ignored and the turn still completes", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-note", identifier: "MUT-4B", state: "In Progress" })],
      codexMode: "id-notification",
      codexTurnDelayMs: 250,
      stateRefreshSequenceByIssueId: {
        "issue-mut-note": ["Done"],
      },
    });
    try {
      const telemetry = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.codex_totals.total_tokens > 0,
        20_000,
        "id-bearing notification path did not continue successfully",
      );
      expect(telemetry.codex_totals.total_tokens).toBeGreaterThan(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given unsupported tool call When turn streams Then the protocol response keeps the session moving", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-tool", identifier: "MUT-5", state: "In Progress" })],
      codexMode: "unsupported-tool",
      codexTurnDelayMs: 300,
      stateRefreshSequenceByIssueId: {
        "issue-mut-tool": ["Done"],
      },
    });
    try {
      const telemetry = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.codex_totals.total_tokens > 0,
        20_000,
        "unsupported-tool path did not emit token telemetry",
      );
      expect(telemetry.codex_totals.total_tokens).toBeGreaterThan(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given a multi-operation linear tool call When turn streams Then the invalid request is rejected and the turn continues", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-graphql", identifier: "MUT-5B", state: "In Progress" })],
      codexMode: "linear-multi-operation",
      codexTurnDelayMs: 300,
      stateRefreshSequenceByIssueId: {
        "issue-mut-graphql": ["Done"],
      },
    });
    try {
      const telemetry = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.codex_totals.total_tokens > 0,
        20_000,
        "multi-operation linear tool path did not continue successfully",
      );
      expect(telemetry.codex_totals.total_tokens).toBeGreaterThan(0);
    } finally {
      await harness.stop();
    }
  });

  test("Given an issue leaves active states after one continuation When turns stream Then exactly one continuation prompt is sent", async () => {
    const issueIdentifier = "MUT-6";
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-turns", identifier: issueIdentifier, state: "In Progress" })],
      maxTurns: 3,
      codexTurnDelayMs: 250,
      stateRefreshSequenceByIssueId: {
        "issue-mut-turns": ["In Progress", "Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0 && payload.counts.retrying === 0,
        25_000,
        "continuation scenario did not settle",
      );

      const transcript = JSON.parse(await loadText(harness.codexTranscriptPath));
      expect(Array.isArray(transcript.turnInputs)).toBeTruthy();
      expect(transcript.turnInputs).toHaveLength(2);
      expect(transcript.turnInputs[0].prompt).toBe(
        `You are working on ${issueIdentifier}: Issue ${issueIdentifier}.`,
      );
      expect(transcript.turnInputs[0].title).toBe(`${issueIdentifier}: Issue ${issueIdentifier}`);
      expect(transcript.turnInputs[1].prompt).toBe(CONTINUATION_PROMPT);
      expect(transcript.turnInputs[1].title).toBe(`${issueIdentifier}: Issue ${issueIdentifier}`);
    } finally {
      await harness.stop();
    }
  });

  test("Given stalled codex mode with short stall timeout When reconcile ticks Then stalled retry is queued with attempt one", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-stall", identifier: "MUT-7", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 700,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
    });
    try {
      await harness.triggerRefresh();
      const state = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.retrying.some((row: any) => row.issue_identifier === "MUT-7"),
        20_000,
        "stalled run did not produce retry",
      );
      const row = state.retrying.find((entry: any) => entry.issue_identifier === "MUT-7");
      expect(String(row.error)).toBe("stalled");
      expect(Number(row.attempt)).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given retry due while no slots are available When retry handling runs Then the retry is requeued with a slot-exhaustion error", async () => {
    const harness = await startSymphonyHarness({
      issues: [
        buildIssue({ id: "issue-mut-slot-a", identifier: "MUT-SLOT-A", state: "In Progress", priority: 1 }),
        buildIssue({ id: "issue-mut-slot-b", identifier: "MUT-SLOT-B", state: "In Progress", priority: 2 }),
      ],
      maxConcurrentAgents: 1,
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      hooks: {
        beforeRun: 'if [ "$(basename "$PWD")" = "MUT-SLOT-A" ]; then exit 15; fi',
      },
    });
    try {
      const retrying = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.retrying.some(
            (row: any) =>
              row.issue_identifier === "MUT-SLOT-A" &&
              Number(row.attempt) >= 2 &&
              String(row.error).includes("no available orchestrator slots"),
          ),
        35_000,
        "slot-exhaustion retry requeue did not occur",
      );
      const row = retrying.retrying.find((entry: any) => entry.issue_identifier === "MUT-SLOT-A");
      expect(Number(row.attempt)).toBeGreaterThanOrEqual(2);
      expect(String(row.error)).toContain("no available orchestrator slots");
    } finally {
      await harness.stop();
    }
  });

  test("Given continuation retry re-dispatches the same issue When after_create writes a marker Then the marker is written once", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-hook", identifier: "MUT-8", state: "In Progress" })],
      maxTurns: 1,
      codexTurnDelayMs: 200,
      stateRefreshSequenceByIssueId: {
        "issue-mut-hook": ["In Progress", "Paused"],
      },
      hooks: {
        afterCreate: "echo created >> .after_create_count",
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.running.some((row: any) => row.issue_identifier === "MUT-8") ||
          payload.retrying.some((row: any) => row.issue_identifier === "MUT-8"),
        20_000,
        "after_create scenario did not start",
      );

      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0 && payload.counts.retrying === 0,
        40_000,
        "after_create continuation scenario did not settle",
      );

      const markerPath = join(harness.workspaceRoot, "MUT-8", ".after_create_count");
      const markerContents = await loadText(markerPath);
      const count = markerContents
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line === "created").length;
      expect(count).toBe(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given workflow reload lowers poll interval and activates a state When watcher reloads Then issue dispatches without manual refresh", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-reload", identifier: "MUT-9", state: "In Progress" })],
      activeStates: ["Todo"],
      pollIntervalMs: 60_000,
      codexTurnDelayMs: 2_000,
      stateRefreshSequenceByIssueId: {
        "issue-mut-reload": ["Done"],
      },
    });
    try {
      const before = await harness.getState();
      expect(before.counts.running).toBe(0);

      const original = await loadText(harness.workflowPath);
      const updated = original
        .replace("active_states: Todo", "active_states: Todo, In Progress")
        .replace("interval_ms: 60000", "interval_ms: '250'");
      await writeFile(harness.workflowPath, updated, "utf8");

      await waitForStateCondition(
        harness.appBaseUrl,
        () => harness.logs.join("").includes("workflow reloaded"),
        10_000,
        "workflow reload was not observed",
      );

      const after = await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === "MUT-9"),
        20_000,
        "reloaded poll interval and active state did not dispatch issue",
      );
      expect(after.counts.running).toBeGreaterThanOrEqual(1);
    } finally {
      await harness.stop();
    }
  });

  test("Given terminal reconciliation cleans a workspace When before_remove fails Then the workspace is removed and the warning is logged", async () => {
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-term", identifier: "MUT-TERM", state: "In Progress" })],
      codexMode: "stall",
      pollIntervalMs: 250,
      stallTimeoutMs: 60_000,
      turnTimeoutMs: 60_000,
      readTimeoutMs: 250,
      hooks: {
        beforeRemove: "exit 21",
      },
      stateRefreshSequenceByIssueId: {
        "issue-mut-term": ["Done"],
      },
    });
    try {
      await harness.triggerRefresh();
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) =>
          payload.counts.running === 0 &&
          !payload.retrying.some((row: any) => row.issue_identifier === "MUT-TERM"),
        20_000,
        "terminal reconciliation did not stop the run",
      );

      let removed = false;
      try {
        await access(join(harness.workspaceRoot, "MUT-TERM"));
      } catch {
        removed = true;
      }
      expect(removed).toBeTruthy();
      expect(harness.logs.join("")).toContain("before_remove hook failed");
    } finally {
      await harness.stop();
    }
  });

  test("Given codex exits after a completed turn When cleanup runs Then normal completion still settles successfully", async () => {
    const issueIdentifier = "MUT-10";
    const harness = await startSymphonyHarness({
      issues: [buildIssue({ id: "issue-mut-exit", identifier: issueIdentifier, state: "In Progress" })],
      codexMode: "success-and-exit",
      codexTurnDelayMs: 300,
      stateRefreshSequenceByIssueId: {
        "issue-mut-exit": ["Done"],
      },
    });
    try {
      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.running.some((row: any) => row.issue_identifier === issueIdentifier),
        20_000,
        "success-and-exit scenario did not start running",
      );

      await waitForStateCondition(
        harness.appBaseUrl,
        (payload) => payload.counts.running === 0 && payload.counts.retrying === 0,
        20_000,
        "success-and-exit scenario did not settle",
      );

      await access(join(harness.workspaceRoot, issueIdentifier));
    } finally {
      await harness.stop();
    }
  });

  test("Given invalid workflow reload When file watcher processes change Then service keeps last-known-good config active", async ({
    request,
  }) => {
    const harness = await startSymphonyHarness({
      issues: [],
    });
    try {
      const original = await loadText(harness.workflowPath);
      await writeFile(harness.workflowPath, "---\ntracker:\n  kind: linear\n  active_states: [\n", "utf8");

      await waitForStateCondition(
        harness.appBaseUrl,
        (state) => typeof state.generated_at === "string",
        10_000,
        "service became unavailable after invalid reload",
      );

      const response = await request.get(`${harness.appBaseUrl}/api/v1/state`);
      expect(response.ok()).toBeTruthy();

      await writeFile(harness.workflowPath, original, "utf8");
    } finally {
      await harness.stop();
    }
  });
});
