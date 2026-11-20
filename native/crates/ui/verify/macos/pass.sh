#!/bin/bash
# pass.sh <run> <part>...: one guest session for the shell and chat pass.
# Boots a fresh $VM from $BASE (refused on battery), installs guestctl and
# $BIN/mcbench-ui-dev, starts the Japanese input method once from the input
# menu (a fresh clone does not start it from guestctl), runs the parts
# (checks.py's, or gate.py's checks written gate:<check>), and stops and
# deletes the guest, also when a part fails.
#   pass.sh s2 gate:launch gate:tree gate:text gate:actions gate:events gate:keys gate:select gate:cell gate:cache gate:ime gate:window gate:vo
#   pass.sh s2 sidebar archive filters palette bench
#   pass.sh s2 chat attach pickers gallery keys window settings fatal crash lag
#   pass.sh s2 vo-sidebar vo-chat vo-pickers
#   pass.sh s2 frame focus held hidden sets pulse frames lag crash shots
# APP_ENV (for example EXPERIMENT_CVDISPLAYLINK=1 with an experiment build) is
# put before the app's command line.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/vm/env.sh"
run=$1; shift
"$here/vm/up.sh" > /dev/null
trap '"$here/vm/down.sh"' EXIT
"$here/vm/install.sh" "$BIN/guestctl" "$BIN/libguestactivate.dylib" "$BIN/mcbench-ui-dev"
gx /usr/local/rig/guestctl click 1183 13; sleep 1; gx /usr/local/rig/guestctl click 1234 72; sleep 1
gx /usr/local/rig/guestctl input-select com.apple.keylayout.US
# The guest shows an "App Background Activity" banner after boot; its
# Notification Center window takes the screen's clicks and the app's key
# status until it is closed (hover, then its close button).
gx /usr/local/rig/guestctl move 1200 100; sleep 0.8; gx /usr/local/rig/guestctl click 1083 52; sleep 1
for part in "$@"; do
  case "$part" in
    gate:*) python3 -I "$here/gate.py" "$run" launch "${part#gate:}" ;;
    *) python3 -I "$here/checks.py" "$run" "$part" ;;
  esac
done
