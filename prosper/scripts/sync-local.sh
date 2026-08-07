#!/bin/zsh
set -euo pipefail

root="/Users/craigstratton/Projects/prosper/buzz"
lock="$root/.git/prosper-buzz-sync.lock"
if ! mkdir "$lock" 2>/dev/null; then
  exit 0
fi
trap 'rmdir "$lock" 2>/dev/null || true' EXIT

cd "$root"
before=$(git rev-parse HEAD)
git fetch --prune origin main

if [[ -n "$(git status --porcelain)" ]]; then
  print -u2 "Prosper Buzz sync skipped: local worktree is dirty"
  exit 0
fi
git merge --ff-only origin/main
after=$(git rev-parse HEAD)

if [[ "$before" != "$after" ]]; then
  changed=$(git diff --name-only "$before" "$after")
  if print -r -- "$changed" | grep -Eq '^prosper/(fleet\.json|global-agent-config\.json|personas/|harnesses/|bin/|runtime/|tools/fleet-provisioner/)'; then
    "$root/prosper/scripts/provision-fleet.sh"
  fi
fi

if /Users/craigstratton/bin/redis-prosper PING >/dev/null 2>&1; then
  /Users/craigstratton/bin/redis-prosper XADD prosper.fleet.activity MAXLEN '~' 2000 '*' \
    kind buzz_repo_sync actor sam status ok commit "$after" >/dev/null 2>&1 || true
fi
