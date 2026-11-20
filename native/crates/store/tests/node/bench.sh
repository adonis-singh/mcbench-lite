#!/bin/sh
# Alternates Node's and this crate's store timings, each run on a fresh
# copy-on-write clone of a data folder; prints one JSON line per run.
#
#   bench.sh <data folder> <scratch> <checkout> <rust test binary> <runs>
set -e
data=$1 scratch=$2 checkout=$3 binary=$4 runs=$5
here=$(cd "$(dirname "$0")" && pwd)
clone() {
  rm -rf "$scratch/bench-clone"
  if [ "$(uname)" = Darwin ]; then cp -c -R "$data" "$scratch/bench-clone"; else cp -a --reflink="${REFLINK:-auto}" "$data" "$scratch/bench-clone"; fi
  # The clone's own writeback, flushed before anything is timed: on btrfs the
  # first commit into a reflinked file that is not yet on disk waits for it
  # (about 380 ms on adipc, the same for Node and Rust).
  if [ "$(uname)" != Darwin ]; then sync -f "$scratch/bench-clone"; else sync; fi
}
i=0
while [ "$i" -lt "$runs" ]; do
  clone
  printf 'node %s ' "$(uptime | sed 's/.*load average[s]*: //')"
  MCBENCH_NODE_DIFF=$checkout MCBENCH_DATA_DIR=$scratch/bench-clone MCBENCH_TOOLS_DIR=$scratch/tools-empty node "$here/bench.js"
  clone
  printf 'rust %s ' "$(uptime | sed 's/.*load average[s]*: //')"
  MCBENCH_BENCH_DATA=$scratch/bench-clone "$binary" --ignored --exact store_timings --nocapture --test-threads 1 2>/dev/null | grep -o '{.*}' || echo failed
  i=$((i + 1))
done
rm -rf "$scratch/bench-clone"
