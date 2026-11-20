#!/bin/bash
# Stops and deletes $VM, then records the host probe after.
set -euo pipefail
. "$(dirname "$0")/env.sh"
[ "$VM" != rig-base ] && [ "$VM" != "$BASE" ] || { echo "refusing to delete the base image" >&2; exit 1; }
w="$OUT/logs/$VM.powerwatch.pid"
if [ -f "$w" ]; then kill "$(cat "$w")" 2>/dev/null || true; /bin/rm -f "$w"; fi
"$TART" stop "$VM" --timeout 30 || true
"$TART" delete "$VM"
"$(dirname "$0")/hostcheck.sh" "$VM-after-down" >/dev/null
