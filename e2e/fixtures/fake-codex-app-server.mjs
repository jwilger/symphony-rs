#!/usr/bin/env node

import readline from "node:readline";

function parseDelayMs(argv) {
  const index = argv.findIndex((value) => value === "--delay-ms");
  if (index === -1) {
    return 100;
  }

  const raw = argv[index + 1];
  const parsed = Number.parseInt(raw ?? "", 10);
  if (Number.isNaN(parsed) || parsed < 0) {
    return 100;
  }

  return parsed;
}

function parseMode(argv) {
  const index = argv.findIndex((value) => value === "--mode");
  if (index === -1) {
    return "success";
  }
  const mode = argv[index + 1] ?? "success";
  return mode;
}

const delayMs = parseDelayMs(process.argv.slice(2));
const mode = parseMode(process.argv.slice(2));
let threadCounter = 0;
let turnCounter = 0;

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
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

    if (mode === "unsupported-tool") {
      setTimeout(() => {
        send({
          id: `tool-${turnCounter}`,
          method: "item/tool/call",
          params: {
            name: "unknown_tool",
            input: {},
          },
        });
      }, Math.max(delayMs - 80, 0));
    }

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
    return;
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
    handleRequest(parsed);
  } catch {
    // Ignore malformed input in the fake server.
  }
});
