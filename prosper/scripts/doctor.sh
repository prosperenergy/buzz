#!/bin/zsh
set -euo pipefail

root="${BUZZ_PROSPER_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"
cd "$root"
source "$root/bin/activate-hermit"
cargo run --quiet -p prosper-buzz-fleet-provisioner -- doctor

version=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' /Applications/Buzz.app/Contents/Info.plist)
pid=$(pgrep -x buzz-desktop | head -1 || true)
branch=$(git -C "$root" branch --show-current)
head=$(git -C "$root" rev-parse --short=12 HEAD)
jq -n --arg version "$version" --arg pid "$pid" --arg branch "$branch" --arg head "$head" \
  '{ok:true,appVersion:$version,appRunning:($pid|length>0),appPid:$pid,repoBranch:$branch,repoHead:$head}'
