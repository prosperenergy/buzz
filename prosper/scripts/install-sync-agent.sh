#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
label="energy.prosper.buzz-fleet-sync"
target="/Users/craigstratton/Library/LaunchAgents/$label.plist"
domain="gui/$(id -u)"

mkdir -p /Users/craigstratton/Library/LaunchAgents /Users/craigstratton/Library/Logs/Prosper
install -m 600 "$root/prosper/$label.plist" "$target"
launchctl bootout "$domain/$label" >/dev/null 2>&1 || true
launchctl bootstrap "$domain" "$target"
launchctl enable "$domain/$label"
launchctl kickstart -k "$domain/$label"
launchctl print "$domain/$label" | grep -E 'state =|last exit code =|runs ='
