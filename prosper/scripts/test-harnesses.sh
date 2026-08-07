#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
if [[ ! -x "$root/prosper/.runtime/npm/node_modules/.bin/claude-agent-acp" || \
      ! -x "$root/prosper/.runtime/npm/node_modules/.bin/codex-acp" ]]; then
  "$root/prosper/scripts/install-runtime.sh" >/dev/null
fi

typeset -A harness_commands
harness_commands[claude]="$root/prosper/bin/claude-buzz-acp"
harness_commands[codex]="$root/prosper/bin/codex-buzz-acp"
harness_commands[copilot]="$root/prosper/bin/copilot-buzz-acp"
harness_commands[grok]="$root/prosper/bin/grok-buzz-acp"
harness_commands[gemini]="$root/prosper/bin/gemini-buzz-acp"
harness_commands[goose]="$root/prosper/bin/goose-buzz-acp"

targets=("${@:-gemini codex goose grok}")
for target in $=targets; do
  command="${harness_commands[$target]:-}"
  [[ -n "$command" ]] || { print -u2 "unknown harness: $target"; exit 64; }
  token="PROSPER_BUZZ_${target:u}_ACP_OK"
  /Users/craigstratton/.npm-global/bin/node "$root/prosper/bin/acp-smoke-probe.mjs" "$command" "$token"
done
