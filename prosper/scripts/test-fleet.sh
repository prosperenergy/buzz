#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
owner_cli="$root/prosper/scripts/with-owner-key.sh"
store="/Users/craigstratton/Library/Application Support/xyz.block.buzz.app/agents/managed-agents.json"
results_dir="$root/prosper/test-results"
stamp=$(date -u +%Y%m%dT%H%M%SZ)
since=$(date +%s)
mkdir -p "$results_dir"

set +x
if [[ -z "${BUZZ_PRIVATE_KEY:-}" ]]; then
  if [[ "${BUZZ_OWNER_NSEC_STDIN:-0}" != 1 ]]; then
    print -u2 "live fleet test needs the owner signing key on stdin (BUZZ_OWNER_NSEC_STDIN=1)"
    exit 78
  fi
  IFS= read -r owner
  [[ "$owner" == nsec1* ]] || { print -u2 "invalid owner signing key"; exit 78; }
  export BUZZ_PRIVATE_KEY="$owner"
  unset owner BUZZ_OWNER_NSEC_STDIN
fi

"$root/prosper/scripts/doctor.sh" >/dev/null
channel_json=$($owner_cli channels create --name "prosper-fleet-smoke-$stamp" --type stream --visibility private --description "Private end-to-end verification for Prosper's six Buzz agents")
channel=$(print -r -- "$channel_json" | jq -er '.id // .channel.id // .channel_id')

typeset -A pubkeys tokens passed
for name in Hanna Sam Burtha Kayla Maddi Oma; do
  slug="prosper:${name:l}"
  pubkeys[$name]=$(jq -er --arg slug "$slug" '.[] | select(.persona_id == $slug) | .pubkey' "$store")
  tokens[$name]="PROSPER_BUZZ_${name:u}_LIVE_OK_$stamp"
  passed[$name]=0
  $owner_cli channels add-member --channel "$channel" --pubkey "${pubkeys[$name]}" --role bot >/dev/null
done

sleep 5
for name in Hanna Sam Burtha Kayla Maddi Oma; do
  started=$(date +%s)
  print "live_test agent=$name stage=sent"
  content="@$name Use the Buzz CLI to reply in this thread with exactly ${tokens[$name]}. Publish it with buzz messages send; do not add other words."
  $owner_cli messages send --channel "$channel" --content "$content" --mention "${pubkeys[$name]}" >/dev/null
  deadline=$(( started + 180 ))
  while (( $(date +%s) < deadline )); do
    if response=$($owner_cli messages search --query "${tokens[$name]}" --author "${pubkeys[$name]}" --since "$since" --limit 20 2>/dev/null); then
      if print -r -- "$response" | jq -e --arg token "${tokens[$name]}" '.. | strings | select(contains($token))' >/dev/null 2>&1; then
        passed[$name]=1
        print "live_test agent=$name result=pass elapsed_seconds=$(( $(date +%s) - started ))"
        break
      fi
    fi
    sleep 5
  done
  (( passed[$name] == 1 )) || print -u2 "live_test agent=$name result=timeout"
done

summary='[]'
failed=0
for name in Hanna Sam Burtha Kayla Maddi Oma; do
  ok=false
  if (( passed[$name] == 1 )); then ok=true; else failed=$(( failed + 1 )); fi
  summary=$(print -r -- "$summary" | jq --arg name "$name" --arg pubkey "${pubkeys[$name]}" --arg token "${tokens[$name]}" --argjson ok "$ok" '. + [{name:$name,pubkey:$pubkey,expected:$token,ok:$ok}]')
done

output=$(print -r -- "$summary" | jq --arg channel "$channel" --arg stamp "$stamp" --argjson failed "$failed" '{ok:($failed == 0),channelId:$channel,stamp:$stamp,failed:$failed,agents:.}')
print -r -- "$output" | tee "$results_dir/live-$stamp.json"
if (( failed == 0 )); then
  $owner_cli channels archive --channel "$channel" >/dev/null
else
  print -u2 "Live test channel left open for diagnosis: $channel"
  exit 1
fi
