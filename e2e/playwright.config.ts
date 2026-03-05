import fs from "node:fs";
import path from "node:path";
import { defineConfig } from "@playwright/test";

function resolveChromiumExecutableFromNixStore(): string | undefined {
  const browsersRoot = process.env.PLAYWRIGHT_BROWSERS_PATH;
  if (!browsersRoot) {
    return undefined;
  }

  let entries: string[] = [];
  try {
    entries = fs.readdirSync(browsersRoot);
  } catch {
    return undefined;
  }

  const preferred = entries
    .filter((entry) => entry.startsWith("chromium-"))
    .sort()
    .reverse();

  const fallbacks = entries
    .filter((entry) => entry.startsWith("chromium_headless_shell-"))
    .sort()
    .reverse();

  for (const entry of [...preferred, ...fallbacks]) {
    const base = path.join(browsersRoot, entry);
    const candidates = [
      path.join(base, "chrome-linux", "chrome"),
      path.join(base, "chrome-linux64", "chrome"),
      path.join(base, "chrome-linux", "headless_shell"),
      path.join(base, "chrome-headless-shell-linux64", "chrome-headless-shell"),
      path.join(base, "chrome-mac", "Chromium.app", "Contents", "MacOS", "Chromium"),
      path.join(base, "chrome-win", "chrome.exe"),
    ];

    const match = candidates.find((candidate) => fs.existsSync(candidate));
    if (match) {
      return match;
    }
  }

  return undefined;
}

const executablePath = resolveChromiumExecutableFromNixStore();

export default defineConfig({
  testDir: "./tests",
  workers: 1,
  fullyParallel: false,
  timeout: 120_000,
  expect: {
    timeout: 15_000,
  },
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
    launchOptions: executablePath ? { executablePath } : undefined,
  },
  reporter: [["list"]],
});
