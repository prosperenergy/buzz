#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const runtime = process.argv[2];
if (!runtime) {
  console.error("usage: patch-claude-adapter.mjs <runtime-root>");
  process.exit(64);
}

const packageRoot = path.join(runtime, "npm/node_modules/@agentclientprotocol/claude-agent-acp");
const packageJson = JSON.parse(fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"));
if (packageJson.version !== "0.65.0") {
  throw new Error(`refusing to patch unreviewed claude-agent-acp ${packageJson.version}`);
}

function replaceExactly(file, original, replacement, alreadyPatched) {
  const current = fs.readFileSync(file, "utf8");
  if (current.includes(alreadyPatched)) return false;
  const count = current.split(original).length - 1;
  if (count !== 1) throw new Error(`expected exactly one reviewed patch site in ${file}, found ${count}`);
  fs.writeFileSync(file, current.replace(original, replacement), { mode: 0o600 });
  return true;
}

const acpFile = path.join(packageRoot, "dist/acp-agent.js");
const settingsFile = path.join(packageRoot, "dist/settings.js");
const acpPatched = replaceExactly(
  acpFile,
  '            settingSources: ["user", "project", "local"],',
  '            settingSources: process.env.BUZZ_CLAUDE_ISOLATED === "1"\n                ? []\n                : ["user", "project", "local"],',
  'settingSources: process.env.BUZZ_CLAUDE_ISOLATED === "1"',
);
const settingsPatched = replaceExactly(
  settingsFile,
  '    async loadAllSettings() {\n        try {',
  '    async loadAllSettings() {\n        if (process.env.BUZZ_CLAUDE_ISOLATED === "1") {\n            this.effective = {\n                permissions: { defaultMode: "default" },\n                enabledPlugins: {},\n            };\n            return;\n        }\n        try {',
  'if (process.env.BUZZ_CLAUDE_ISOLATED === "1")',
);

console.log(JSON.stringify({ ok: true, package: packageJson.name, version: packageJson.version, patched: acpPatched || settingsPatched }));
