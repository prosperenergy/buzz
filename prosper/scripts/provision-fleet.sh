#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$root"
app_data="/Users/craigstratton/Library/Application Support/xyz.block.buzz.app"
was_running=0
reopened=0

if pgrep -x buzz-desktop >/dev/null 2>&1; then
  was_running=1
fi

restart_buzz() {
  if (( was_running == 1 && reopened == 0 )); then
    open -g -a Buzz >/dev/null 2>&1 || true
  fi
}
trap restart_buzz EXIT

"$root/prosper/scripts/install-runtime.sh"
mkdir -p "$app_data/custom_harnesses"
for harness in "$root"/prosper/harnesses/*.json; do
  install -m 600 "$harness" "$app_data/custom_harnesses/${harness:t}"
done

if (( was_running == 1 )); then
  osascript -e 'tell application id "xyz.block.buzz.app" to quit' >/dev/null 2>&1 || true
  for _ in {1..40}; do
    pgrep -x buzz-desktop >/dev/null 2>&1 || break
    sleep 0.25
  done
  if pgrep -x buzz-desktop >/dev/null 2>&1; then
    pkill -TERM -x buzz-desktop
    for _ in {1..20}; do
      pgrep -x buzz-desktop >/dev/null 2>&1 || break
      sleep 0.25
    done
  fi
fi
pgrep -x buzz-desktop >/dev/null 2>&1 && { print -u2 "Buzz did not stop cleanly"; exit 1; }

global_source="$root/prosper/global-agent-config.json"
global_target="$app_data/agents/global-agent-config.json"
if [[ ! -f "$global_target" ]] || ! cmp -s "$global_source" "$global_target"; then
  if [[ -f "$global_target" ]]; then
    mkdir -p "$app_data/agents/backups"
    stamp=$(date -u +%Y%m%dT%H%M%SZ)
    cp -p "$global_target" "$app_data/agents/backups/global-agent-config.before-prosper-provision-$stamp.json"
    chmod 600 "$app_data/agents/backups/global-agent-config.before-prosper-provision-$stamp.json"
  fi
  install -m 600 "$global_source" "$global_target"
fi

source "$root/bin/activate-hermit"
cargo run --quiet -p prosper-buzz-fleet-provisioner -- apply

open -g -a Buzz
reopened=1
for _ in {1..120}; do
  if pgrep -x buzz-desktop >/dev/null 2>&1; then
    sleep 2
    break
  fi
  sleep 0.25
done
pgrep -x buzz-desktop >/dev/null 2>&1 || { print -u2 "Buzz failed to reopen"; exit 1; }

# Newly generated agent keys are written only to the owner-mode agent store,
# then the signed Buzz process migrates them into its Keychain blob and strips
# the inline fields. Do not call the structural doctor until that round trip is
# complete.
for _ in {1..120}; do
  if ! grep -q 'private_key_nsec\|nsec1' "$app_data/agents/managed-agents.json"; then
    break
  fi
  sleep 0.5
done
if grep -q 'private_key_nsec\|nsec1' "$app_data/agents/managed-agents.json"; then
  print -u2 "Buzz did not finish signed Keychain migration within 60 seconds"
  exit 1
fi

cargo run --quiet -p prosper-buzz-fleet-provisioner -- doctor
trap - EXIT
