#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
runtime="$root/prosper/.runtime"
npm_source="$root/prosper/runtime/npm"

umask 077
mkdir -p "$runtime/npm" "$runtime/homes/codex/.codex" "$runtime/homes/grok/.grok" \
  "$runtime/homes/goose/.config" "$runtime/homes/goose/.local/share" \
  "$runtime/logs/codex" "$runtime/logs/goose-codex" "$runtime/workspaces/smoke"

cp "$npm_source/package.json" "$runtime/npm/package.json"
cp "$npm_source/package-lock.json" "$runtime/npm/package-lock.json"
npm ci --prefix "$runtime/npm" --no-audit --no-fund
/Users/craigstratton/.npm-global/bin/node "$root/prosper/scripts/patch-claude-adapter.mjs" "$runtime"

install -m 600 "$root/prosper/runtime/codex-config.toml" "$runtime/homes/codex/.codex/config.toml"
if [[ ! -f /Users/craigstratton/.codex/auth.json ]]; then
  print -u2 "Codex first-party auth is missing"
  exit 1
fi
ln -sfn /Users/craigstratton/.codex/auth.json "$runtime/homes/codex/.codex/auth.json"

if [[ ! -f /Users/craigstratton/.grok/auth.json ]]; then
  print -u2 "Grok first-party auth is missing"
  exit 1
fi
ln -sfn /Users/craigstratton/.grok/auth.json "$runtime/homes/grok/.grok/auth.json"

chmod 700 "$root"/prosper/bin/* "$root"/prosper/scripts/*.sh
for binary in \
  "$runtime/npm/node_modules/.bin/claude-agent-acp" \
  "$runtime/npm/node_modules/.bin/codex-acp" \
  /Users/craigstratton/.npm-global/bin/gemini \
  /Users/craigstratton/.local/bin/goose \
  /Users/craigstratton/.local/bin/copilot \
  /Users/craigstratton/.grok/bin/grok; do
  [[ -x "$binary" ]] || { print -u2 "required executable missing: $binary"; exit 1; }
done

print '{"ok":true,"runtime":"installed","secretsCopied":false}'
