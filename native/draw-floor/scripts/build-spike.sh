#!/bin/sh
# On the Mac, from the stage-two worktree: the renderer spike at commit
# 31f64e6 (branch claude/sharp-mendeleev-e55a37) with direct-vulkan.patch
# applied, copied to adipc and built there (shot and window, release).
#   native/draw-floor/scripts/build-spike.sh
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
SPIKE_REPO=$(git -C "$here" rev-parse --show-toplevel)
SPIKE_COMMIT=31f64e6
work=$(mktemp -d)
git -C "$SPIKE_REPO" archive "$SPIKE_COMMIT" native/spikes/renderer renderer | tar -x -C "$work"
(cd "$work" && patch -p1 < "$here/direct-vulkan.patch")
ssh adipc 'mkdir -p ~/mcbench-native-scratch/s2-floor/src ~/mcbench-native-scratch/s2-floor/scripts'
rsync -a --delete "$work/" adipc:mcbench-native-scratch/s2-floor/src/
rsync -a "$here/scripts/" adipc:mcbench-native-scratch/s2-floor/scripts/
/bin/rm -rf "$work"
ssh adipc '. ~/mcbench-native-scratch/s2-floor/scripts/env.sh && cd $R/src/native/spikes/renderer && cargo build --release --bin shot --bin window'
