#!/usr/bin/env node

import { spawn } from "node:child_process";
import readline from "node:readline";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = process.env.BUZZ_PROSPER_ROOT || path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const isolatedHome = `${root}/prosper/.runtime/homes/grok`;
const grokHome = `${isolatedHome}/.grok`;
const grokBinary = "/Users/craigstratton/.grok/bin/grok";

const child = spawn(grokBinary, ["agent", "--no-leader", "stdio"], {
  env: {
    ...process.env,
    HOME: isolatedHome,
    GROK_HOME: grokHome,
    GROK_MEMORY: "0",
    GROK_SUBAGENTS: "0",
    GROK_WORKFLOWS: "0",
    GROK_WEB_FETCH: "0",
    GROK_SANDBOX: "workspace",
    GROK_TELEMETRY_ENABLED: "0",
    GROK_TELEMETRY_TRACE_UPLOAD: "0",
    GROK_TELEMETRY_MIXPANEL_ENABLED: "0",
    GROK_FEEDBACK_ENABLED: "0",
    GROK_CURSOR_SKILLS_ENABLED: "false",
    GROK_CURSOR_RULES_ENABLED: "false",
    GROK_CURSOR_AGENTS_ENABLED: "false",
    GROK_CURSOR_MCPS_ENABLED: "false",
    GROK_CURSOR_HOOKS_ENABLED: "false",
    GROK_CURSOR_SESSIONS_ENABLED: "false",
    GROK_CLAUDE_SKILLS_ENABLED: "false",
    GROK_CLAUDE_RULES_ENABLED: "false",
    GROK_CLAUDE_AGENTS_ENABLED: "false",
    GROK_CLAUDE_MCPS_ENABLED: "false",
    GROK_CLAUDE_HOOKS_ENABLED: "false",
    GROK_CLAUDE_SESSIONS_ENABLED: "false",
    GROK_CODEX_SESSIONS_ENABLED: "false",
  },
  stdio: ["pipe", "pipe", "inherit"],
});

const internalAuthId = "prosper-buzz-grok-cached-auth";
let initializeId;
let initializeLine;
let authenticating = false;
let authenticated = false;
const bufferedAgentLines = [];
const emit = (line) => process.stdout.write(`${line}\n`);

readline.createInterface({ input: process.stdin }).on("line", (line) => {
  try {
    const message = JSON.parse(line);
    if (message?.method === "initialize" && message?.id !== undefined) initializeId = message.id;
  } catch {
    // Forward malformed input so the native ACP server can report it.
  }
  child.stdin.write(`${line}\n`);
});

readline.createInterface({ input: child.stdout }).on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    emit(line);
    return;
  }

  if (!authenticated && message?.id === initializeId && message?.result) {
    initializeLine = line;
    const methods = Array.isArray(message.result.authMethods) ? message.result.authMethods : [];
    const preferred = message.result?._meta?.defaultAuthMethodId;
    const methodId =
      (typeof preferred === "string" && methods.some((method) => method?.id === preferred) && preferred) ||
      methods.find((method) => method?.id === "cached_token")?.id;
    if (!methodId) {
      authenticated = true;
      emit(initializeLine);
      initializeLine = undefined;
      return;
    }
    authenticating = true;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: internalAuthId, method: "authenticate", params: { methodId } })}\n`);
    return;
  }

  if (authenticating && message?.id === internalAuthId) {
    if (message.error) {
      process.stderr.write(`Grok cached authentication failed: ${message.error.message ?? "unknown error"}\n`);
      child.kill("SIGTERM");
      process.exitCode = 1;
      return;
    }
    authenticating = false;
    authenticated = true;
    emit(initializeLine);
    initializeLine = undefined;
    for (const buffered of bufferedAgentLines.splice(0)) emit(buffered);
    return;
  }

  if (authenticating) bufferedAgentLines.push(line);
  else emit(line);
});

child.on("error", (error) => {
  process.stderr.write(`Failed to start Grok Build: ${error.message}\n`);
  process.exitCode = 1;
});
child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  process.exitCode = code ?? 1;
});
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) process.on(signal, () => child.kill(signal));
