#!/usr/bin/env bash
# vm-pin.sh <tree> <data bundle> <update key file> <linux files folder> <private folder> <out folder> [<pinned program>]
#
# On adipc: records the engine's pinned profile for x86_64-pc-windows-msvc in
# the Windows VM's hold slot (pgo.md, Recording the engine's profile), then
# times what the profile is worth on Windows, as bench:
#
#   1. windows.ps1 with MCBENCH_PGO_PIN naming the corpus recording; its
#      <out>.profdata and <out>.record come back to <out folder> (function
#      names and counts only; nothing here commits them), and the payload
#      folder to <out folder>/payload, for the installer and archive sizes
#   2. without <pinned program>: an ordinary windows.ps1 build from a second
#      checkout path (\\tree2) with that profile as the tree's pinned one,
#      which must apply it to every engine function (pin.ps1 prove); with
#      <pinned program>: windows.ps1 -NoProfile on the same tree, then
#   3. pgo-replay.js --times of corpus6 on the no-profile program and the
#      pinned build, alternately, three rounds; the sums per round go to
#      <out folder>/times.txt
#
#   <pinned program>  a pinned build's mcbench-lite.exe from an earlier run of
#                     this script on the same tree: step 1 is left out, and
#                     the cache disk's \tree (that run's) is built again by
#                     step 2
#
#   <private folder>  corpus-recording.tar.zst, corpus6.tar.zst and their
#                     .counts: the private benchmark's builds. They go to
#                     C:\mcbpriv on the run's overlay (never the cache disk),
#                     which the trap deletes with the overlay, and nothing of
#                     them is copied back.
set -euo pipefail

fail() { echo "vm-pin.sh: $*" >&2; exit 1; }
[ "$#" = 6 ] || [ "$#" = 7 ] || fail "usage: vm-pin.sh <tree> <data bundle> <update key file> <linux files folder> <private folder> <out folder> [<pinned program>]"
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TREE="$(cd "$1" && pwd)" BUNDLE="$2" KEY="$3" LINUX="$(cd "$4" && pwd)" PRIVATE="$(cd "$5" && pwd)" OUTDIR="$6" PINNED="${7:-}"
for f in corpus-recording.tar.zst corpus-recording.counts corpus6.tar.zst corpus6.counts; do [ -f "$PRIVATE/$f" ] || fail "$PRIVATE has no $f."; done
for f in "$BUNDLE" "$KEY" "$LINUX/mcbench-build-child"; do [ -f "$f" ] || fail "$f does not exist."; done
mkdir -p "$OUTDIR"
OUTDIR="$(cd "$OUTDIR" && pwd)"

source "$TESTDIR/hold.sh"
CACHE="$S5WIN/cache.qcow2"
work=""
on_exit() { if [ -n "$work" ]; then rm -rf "$work"; fi; }
hold_take vm-pin.sh
work="$(mktemp -d "$S5WIN/vm-pin.XXXXXX")"
mkdir -p "$work/in/bundle" "$work/in/linux"
cp "$BUNDLE" "$work/in/bundle/"
cp "$KEY" "$work/in/minisign.pub"
cp "$LINUX/mcbench-build-child" "$work/in/linux/"
echo "http://10.0.2.2:3191/latest.json" >"$work/in/feed"
tar -cf "$work/tree.tar" -C "$TREE" .
[ -f "$CACHE" ] || fail "the cache disk $CACHE does not exist; the instrumented target is on it."

hold_boot -drive file="$CACHE",if=none,id=cache,format=qcow2,cache=writeback,discard=unmap,detect-zeroes=unmap \
  -device virtio-blk-pci,drive=cache
bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb\progs, C:\mcbpriv | Out-Null'
bg_wait to_guest "$TESTDIR/guest/pin.ps1" "$work/tree.tar" "$work/in" C:/mcb/
bg_wait to_guest "$PRIVATE/corpus-recording.tar.zst" "$PRIVATE/corpus-recording.counts" "$PRIVATE/corpus6.tar.zst" "$PRIVATE/corpus6.counts" C:/mcbpriv/
pin() { bg_wait as bench "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\pin.ps1 $*"; }
if [ -n "$PINNED" ]; then
  bg_wait to_guest "$PINNED" C:/mcb/progs/pinned.exe
  echo "vm-pin: the pinned program is $PINNED, sha256 $(sha256sum "$PINNED" | cut -c1-64)"
else
  t0=$SECONDS
  pin pin
  echo "vm-pin: the pinned profile's build took $((SECONDS - t0)) s"
  bg_wait from_guest C:/mcb/pin.profdata "$OUTDIR/x86_64-pc-windows-msvc.profdata"
  bg_wait from_guest C:/mcb/pin.record "$OUTDIR/x86_64-pc-windows-msvc.record"
  bg_wait from_guest C:/mcb/pin "$OUTDIR/payload"
  (cd "$OUTDIR" && ls -l x86_64-pc-windows-msvc.* payload && sha256sum x86_64-pc-windows-msvc.* payload/*)
fi
if [ -n "$PINNED" ]; then
  t0=$SECONDS
  pin noprof
  echo "vm-pin: the build without a profile took $((SECONDS - t0)) s"
  pin times 3 | tee "$OUTDIR/times.txt"
else
  # The new profile from a second checkout path must match every engine function.
  t0=$SECONDS
  pin prove | tee "$OUTDIR/prove.log"
  echo "vm-pin: the build from the second checkout path took $((SECONDS - t0)) s"
  grep -a "pgo: the engine's profile applied" "$OUTDIR/prove.log"
fi
echo "vm-pin: done"
