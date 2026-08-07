#!/bin/zsh
set -euo pipefail
set +x

if [[ -z "${BUZZ_PRIVATE_KEY:-}" ]]; then
  if [[ "${BUZZ_OWNER_NSEC_STDIN:-0}" != 1 ]]; then
    print -u2 "owner signing key unavailable: set BUZZ_PRIVATE_KEY or pipe it with BUZZ_OWNER_NSEC_STDIN=1"
    exit 78
  fi
  IFS= read -r owner
  [[ "$owner" == nsec1* ]] || { print -u2 "invalid owner signing key"; exit 78; }
  export BUZZ_PRIVATE_KEY="$owner"
  unset owner
fi
export BUZZ_RELAY_URL="wss://prosper.communities.buzz.xyz"
exec /Users/craigstratton/.local/bin/buzz "$@"
