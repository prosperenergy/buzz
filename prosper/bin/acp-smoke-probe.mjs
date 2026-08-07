#!/usr/bin/env node

import { spawn } from "node:child_process";
import readline from "node:readline";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [, , command, expected, ...args] = process.argv;
if (!command || !expected) {
  console.error("usage: acp-smoke-probe.mjs <command> <expected-token> [args...]");
  process.exit(64);
}

const root = process.env.BUZZ_PROSPER_ROOT || path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const cwd = `${root}/prosper/.runtime/workspaces/smoke`;
fs.mkdirSync(cwd, { recursive: true, mode: 0o700 });
const child = spawn(command, args, { cwd, env: process.env, stdio: ["pipe", "pipe", "pipe"] });
let nextId = 1;
const pending = new Map();
const observedStrings = [];
const agentTextChunks = [];
const stderrTail = [];

function request(method, params) {
  const id = nextId++;
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
function respond(id, result) {
  child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`);
}
function collectStrings(value) {
  if (typeof value === "string") observedStrings.push(value);
  else if (Array.isArray(value)) for (const item of value) collectStrings(item);
  else if (value && typeof value === "object") for (const item of Object.values(value)) collectStrings(item);
}

readline.createInterface({ input: child.stdout }).on("line", (line) => {
  let message;
  try { message = JSON.parse(line); } catch { return; }
  const streamedText = message?.params?.update?.content?.text;
  if (message?.params?.update?.sessionUpdate === "agent_message_chunk" && typeof streamedText === "string") {
    agentTextChunks.push(streamedText);
  }
  collectStrings(message.params);
  collectStrings(message.result);
  if (Object.hasOwn(message, "id") && (Object.hasOwn(message, "result") || Object.hasOwn(message, "error"))) {
    const waiter = pending.get(message.id);
    if (!waiter) return;
    pending.delete(message.id);
    if (message.error) waiter.reject(new Error(message.error.message || "ACP request failed"));
    else waiter.resolve(message.result);
    return;
  }
  if (Object.hasOwn(message, "id") && message.method) {
    if (message.method === "session/request_permission") respond(message.id, { outcome: { outcome: "cancelled" } });
    else child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "Client method unavailable in isolated smoke test" } })}\n`);
  }
});
readline.createInterface({ input: child.stderr }).on("line", (line) => {
  stderrTail.push(line.replace(/[A-Za-z0-9_-]{32,}/g, "<redacted>"));
  if (stderrTail.length > 12) stderrTail.shift();
});

const timeout = setTimeout(() => {
  child.kill("SIGTERM");
  console.log(JSON.stringify({ ok: false, error: "timeout", stderrTail }));
  process.exit(124);
}, 240_000);

try {
  const initialized = await request("initialize", {
    protocolVersion: 1,
    clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
    clientInfo: { name: "prosper-buzz-smoke-probe", version: "1.0.0" },
  });
  const session = await request("session/new", { cwd, mcpServers: [], _meta: { sessionTitle: "Prosper Buzz harness smoke" } });
  if (initialized?.configOptions?.some((option) => option.id === "mode")) {
    await request("session/set_config_option", { sessionId: session.sessionId, configId: "mode", value: "default" }).catch(() => undefined);
  }
  const promptResult = await request("session/prompt", {
    sessionId: session.sessionId,
    prompt: [{ type: "text", text: `Reply with exactly ${expected}. Do not use tools.` }],
  });
  await new Promise((resolve) => setTimeout(resolve, 250));
  const ok = observedStrings.some((value) => value.includes(expected)) || agentTextChunks.join("").includes(expected);
  clearTimeout(timeout);
  child.kill("SIGTERM");
  console.log(JSON.stringify({
    ok,
    expected,
    stopReason: promptResult?.stopReason ?? null,
    agent: initialized?.agentInfo ?? null,
    observed: ok ? [] : observedStrings.filter((value) => value.length <= 240).slice(-20).map((value) => value.replace(/[A-Za-z0-9_-]{32,}/g, "<redacted>")),
    stderrTail: ok ? [] : stderrTail,
  }));
  process.exit(ok ? 0 : 1);
} catch (error) {
  clearTimeout(timeout);
  child.kill("SIGTERM");
  console.log(JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error), stderrTail }));
  process.exit(1);
}
