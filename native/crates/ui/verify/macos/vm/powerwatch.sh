#!/bin/bash
# powerwatch.sh <tart pid>: started by up.sh. Every 10 minutes while the
# guest runs, looks at the Mac's power source; on battery it writes
# $OUT/logs/$VM-on-battery (with pmset's line), which gate.py and checks.py
# read before each check and then stop the guest (vm/down.sh). Ends with the
# guest.
set -uo pipefail
. "$(dirname "$0")/env.sh"
tart=$1
while kill -0 "$tart" 2>/dev/null; do
  if ! pmset -g batt | head -1 | grep -q 'AC Power'; then
    { date; pmset -g batt | head -2; } > "$OUT/logs/$VM-on-battery"
  fi
  sleep 600
done
