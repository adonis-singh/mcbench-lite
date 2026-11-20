# Sourced by the gate's VM scripts.
#   RIG_HOME  the rig folder (tools/, vm/), required
#   VM        the throwaway clone every test runs in (default rig-s4)
#   BASE      the image it is cloned from (default rig-base-2, the baked base)
#   OUT       this agent's scratch: share/, logs/, hostprobe/, evidence/
: "${RIG_HOME:?set RIG_HOME to the rig folder (tools/, vm/)}"
export TART_HOME="$RIG_HOME/vm"
TART="$RIG_HOME/tools/tart-2.41.1/tart.app/Contents/MacOS/tart"
VM=${VM:-rig-s4}
BASE=${BASE:-rig-base-2}
: "${OUT:?set OUT to the scratch folder (share/, logs/, hostprobe/, evidence/ go there)}"
SHARE="$OUT/share"
BIN=${BIN:-$OUT/bin}
mkdir -p "$SHARE" "$OUT/logs" "$OUT/hostprobe" "$OUT/evidence"
gx() { "$TART" exec "$VM" "$@"; }            # a command in the guest's GUI session
gxi() { "$TART" exec -i "$VM" "$@"; }        # the same, with stdin
