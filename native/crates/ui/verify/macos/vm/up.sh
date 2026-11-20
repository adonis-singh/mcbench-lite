#!/bin/bash
# Clones $BASE to $VM (APFS clone, 0.1 s), boots it with no window, no Dock
# icon (Tart's prohibited activation policy under --no-graphics), no audio, no
# clipboard sharing, $SHARE mounted at "/Volumes/My Shared Files/s4", then
# waits for the guest agent. Records the host probe before boot. Prints the
# tart pid. Never touches rig-base.
set -euo pipefail
. "$(dirname "$0")/env.sh"
[ "$VM" != rig-base ] && [ "$VM" != "$BASE" ] || { echo "refusing to run the base image itself" >&2; exit 1; }
# The guest runs only on mains: refused on battery, and vm/powerwatch.sh
# (started below) looks again every 10 minutes while it runs.
pmset -g batt | head -1 | grep -q 'AC Power' || { echo "on battery: not booting the guest" >&2; exit 1; }
pmset -g batt | head -2 > "$OUT/logs/$VM-power.txt"
/bin/rm -f "$OUT/logs/$VM-on-battery"
"$(dirname "$0")/hostcheck.sh" "$VM-before-boot" >/dev/null
"$TART" delete "$VM" 2>/dev/null || true
"$TART" clone "$BASE" "$VM"
nohup "$TART" run "$VM" --no-graphics --no-audio --no-clipboard --dir "s4:$SHARE" > "$OUT/logs/$VM.log" 2>&1 &
pid=$!
echo "$pid" > "$OUT/logs/$VM.pid"
for i in $(seq 1 600); do
  if gx true >/dev/null 2>&1; then
    nohup "$(dirname "$0")/powerwatch.sh" "$pid" > /dev/null 2>&1 &
    echo $! > "$OUT/logs/$VM.powerwatch.pid"
    echo "$pid"; exit 0
  fi
  kill -0 $pid 2>/dev/null || { echo "tart run exited: see $OUT/logs/$VM.log" >&2; exit 1; }
  sleep 0.5
done
echo "guest agent did not answer within 300 s" >&2
exit 1
