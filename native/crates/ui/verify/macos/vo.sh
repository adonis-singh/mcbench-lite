#!/bin/bash
# vo.sh <name> <step>...: VoiceOver driven by real keys in the guest. Each
# step is a guestctl key spec "<code> <mods>" (VO = ctrl,alt); after each,
# the last spoken phrase (AppleScript to VoiceOver, enabled in rig-base-2)
# and a screenshot of the caption panel are recorded to
# $OUT/evidence/<name>/vo.jsonl and NN.png. VoiceOver must already be on.
set -euo pipefail
. "$(dirname "$0")/vm/env.sh"
name=$1; shift
d="$OUT/evidence/$name"; mkdir -p "$d"; : > "$d/vo.jsonl"
i=0
for step in "$@"; do
  gx /usr/local/rig/guestctl key $step
  sleep "${VO_WAIT:-0.5}"
  phrase=$(gx osascript -e 'tell application "VoiceOver" to get content of last phrase' 2>&1 || true)
  gx sh -c 'screencapture -x -R100,700,620,150 /tmp/v.png && cat /tmp/v.png' > "$d/$(printf %02d $i).png"
  python3 -I -c 'import json,sys; print(json.dumps({"step": int(sys.argv[1]), "keys": sys.argv[2], "phrase": sys.argv[3]}, ensure_ascii=False))' "$i" "$step" "$phrase" | tee -a "$d/vo.jsonl"
  i=$((i+1))
done
