#!/usr/bin/env node

import { writeFileSync } from "node:fs";
import readline from "node:readline";

function parseFlag(argv, flag) {
  const index = argv.findIndex((value) => value === flag);
  if (index === -1) {
    return null;
  }

  return argv[index + 1] ?? null;
}

function parseDelayMs(argv) {
  const raw = parseFlag(argv, "--delay-ms");
  if (raw === null) {
    return 100;
  }

  const parsed = Number.parseInt(raw, 10);
  if (Number.isNaN(parsed) || parsed < 0) {
    return 100;
  }

  return parsed;
}

function parseMode(argv) {
  return parseFlag(argv, "--mode") ?? "success";
}

function parseTranscriptPath(argv) {
  return parseFlag(argv, "--transcript-path");
}

const argv = process.argv.slice(2);
const delayMs = parseDelayMs(argv);
const mode = parseMode(argv);
const transcriptPath = parseTranscriptPath(argv);
let threadCounter = 0;
let turnCounter = 0;
let pendingRequest = null;
const turnInputs = [];

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function persistTranscript() {
  if (!transcriptPath) {
    return;
  }

  writeFileSync(transcriptPath, JSON.stringify({ turnInputs }, null, 2));
}

function recordTurnInput(message) {
  const inputs = Array.isArray(message?.params?.input) ? message.params.input : [];
  const prompt = inputs
    .filter((item) => item?.type === "text" && typeof item.text === "string")
    .map((item) => item.text)
    .join("\n");
  const title = typeof message?.params?.title === "string" ? message.params.title : null;

  turnInputs.push({ prompt, title });
  persistTranscript();
}

function usagePayload() {
  return {
    input_tokens: 120,
    output_tokens: 80,
    total_tokens: 200,
  };
}

function rateLimitPayload() {
  return {
    requests_remaining: 42,
    reset_at: new Date(Date.now() + 60_000).toISOString(),
  };
}

function failTurn(message) {
  send({
    method: "turn/failed",
    params: {
      message,
    },
  });
}

function scheduleTokenUsage() {
  setTimeout(() => {
    send({
      method: "thread/tokenUsage/updated",
      params: {
        total_token_usage: usagePayload(),
        rate_limits: rateLimitPayload(),
        message: "token usage updated",
      },
    });
  }, Math.max(delayMs - 50, 0));
}

function scheduleCompletionForMode() {
  scheduleTokenUsage();
  setTimeout(() => {
    if (mode === "turn-failed") {
      send({
        method: "turn/failed",
        params: {
          message: "turn failed",
        },
      });
      return;
    }

    if (mode === "turn-cancelled") {
      send({
        method: "turn/cancelled",
        params: {
          message: "turn cancelled",
        },
      });
      return;
    }

    send({
      method: "turn/completed",
      params: {
        message: "turn completed",
      },
    });
  }, delayMs);
}

function scheduleSuccessAfterResponse() {
  scheduleTokenUsage();
  setTimeout(() => {
    send({
      method: "turn/completed",
      params: {
        message: "turn completed",
      },
    });
  }, delayMs);
}

function expectRequestResponse(kind, id, validator) {
  if (pendingRequest !== null) {
    clearTimeout(pendingRequest.timeout);
  }

  pendingRequest = {
    id,
    kind,
    validator,
    timeout: setTimeout(() => {
      if (pendingRequest?.id !== id) {
        return;
      }

      pendingRequest = null;
      failTurn(`${kind} response timeout`);
    }, Math.max(delayMs * 4, 1_000)),
  };
}

function maybeHandlePendingResponse(message) {
  if (pendingRequest === null || message.method !== undefined || message.id !== pendingRequest.id) {
    return false;
  }

  const { kind, validator, timeout } = pendingRequest;
  clearTimeout(timeout);
  pendingRequest = null;

  if (!validator(message)) {
    failTurn(`${kind} response invalid`);
    return true;
  }

  scheduleSuccessAfterResponse();
  return true;
}

function sendNotificationWithId() {
  setTimeout(() => {
    send({
      id: `notification-${turnCounter}`,
      method: "item/stream/update",
      params: {
        message: "id-bearing notification",
      },
    });
    scheduleCompletionForMode();
  }, Math.max(delayMs - 80, 0));
}

function sendUnsupportedToolRequest() {
  const toolCallId = `tool-${turnCounter}`;
  setTimeout(() => {
    send({
      id: toolCallId,
      method: "item/tool/call",
      params: {
        name: "unknown_tool",
        input: {},
      },
    });
    expectRequestResponse(
      "unsupported_tool_call",
      toolCallId,
      (response) =>
        response?.result?.success === false &&
        response?.result?.error === "unsupported_tool_call",
    );
  }, Math.max(delayMs - 80, 0));
}

function sendLinearMultiOperationRequest() {
  const toolCallId = `tool-${turnCounter}`;
  setTimeout(() => {
    send({
      id: toolCallId,
      method: "item/tool/call",
      params: {
        name: "linear_graphql",
        input: {
          query:
            "query Viewer { viewer { id } } mutation UpdateIssue { issueUpdate(id: \"1\", input: {}) { success } }",
          variables: {},
        },
      },
    });
    expectRequestResponse(
      "expected_single_operation",
      toolCallId,
      (response) =>
        response?.result?.success === false &&
        response?.result?.error === "expected_single_operation",
    );
  }, Math.max(delayMs - 80, 0));
}

function sendApprovalRequest() {
  const approvalId = `approval-${turnCounter}`;
  setTimeout(() => {
    send({
      id: approvalId,
      method: "item/approval/request",
      params: {
        message: "approve command",
      },
    });
    expectRequestResponse(
      "approval",
      approvalId,
      (response) => response?.result?.approved === true,
    );
  }, Math.max(delayMs - 80, 0));
}

function handleRequest(message) {
  const id = message.id;
  const method = message.method ?? "";

  if (method === "initialize") {
    send({
      id,
      result: {
        serverInfo: {
          name: "fake-codex-app-server",
          version: "1.0.0",
        },
      },
    });
    return;
  }

  if (method === "thread/start") {
    threadCounter += 1;
    send({
      id,
      result: {
        thread: {
          id: `thread-${threadCounter}`,
        },
      },
    });
    return;
  }

  if (method === "turn/start") {
    turnCounter += 1;
    recordTurnInput(message);

    send({
      id,
      result: {
        turn: {
          id: `turn-${turnCounter}`,
        },
      },
    });

    if (mode === "stall") {
      return;
    }

    if (mode === "input-required") {
      setTimeout(() => {
        send({
          method: "item/tool/requestUserInput",
          params: {
            message: "user input is required",
          },
        });
      }, delayMs);
      return;
    }

    if (mode === "approval-required") {
      sendApprovalRequest();
      return;
    }

    if (mode === "id-notification") {
      sendNotificationWithId();
      return;
    }

    if (mode === "unsupported-tool") {
      sendUnsupportedToolRequest();
      return;
    }

    if (mode === "linear-multi-operation") {
      sendLinearMultiOperationRequest();
      return;
    }

    scheduleCompletionForMode();
  }
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Number.POSITIVE_INFINITY,
});

rl.on("line", (line) => {
  const trimmed = line.trim();
  if (trimmed.length === 0) {
    return;
  }

  try {
    const parsed = JSON.parse(trimmed);
    if (maybeHandlePendingResponse(parsed)) {
      return;
    }
    handleRequest(parsed);
  } catch {
    // Ignore malformed input in the fake server.
  }
});
