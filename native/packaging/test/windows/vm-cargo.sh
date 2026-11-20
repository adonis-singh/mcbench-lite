#!/usr/bin/env bash
# vm-cargo.sh <tree> <log folder> -- <cargo arguments...>
#
# On adipc: runs one cargo command of the native workspace in the Windows 11
# VM's hold slot (hold.sh), as bench, for a fast loop on one crate, for
# example
#   vm-cargo.sh tree logs -- test -p mcbench-updater --all-features --locked --no-fail-fast
#
#   <tree>        a tree brought to adipc by content (COMMON's git ls-files and
#                 rsync line: native core renderer server README.md LICENSE NOTICE.md)
#   <log folder>  where cargo-<time>.log goes, with the command's whole output,
#                 whether it passes or fails
#
# The guest keeps a copy of the tree (\ctree) and a target folder (\ctarget)
# on stage five's own cache disk, ~/mcbench-native-scratch/s5/win/cache.qcow2
# (a sparse 40 GiB qcow2, NTFS volume mcbs5cache, made when missing; never the
# check's cache disk), with cargo's registry (\cargo). The tree goes into the
# guest by content, as native/check.sh's Windows path does: only files whose
# bytes changed are written (with the time now), so cargo's freshness holds
# and a second run is incremental. cargo runs from native\ with the toolchain
# of native\rust-toolchain.toml, 4 jobs and 4 test threads (the slot's 4 host
# threads), CARGO_INCREMENTAL=0 and no debug information, as the check runs.
#
# The script's exit status is cargo's.
set -euo pipefail

fail() { echo "vm-cargo.sh: $*" >&2; exit 2; }
[ "$#" -ge 4 ] && [ "$3" = -- ] || fail "usage: vm-cargo.sh <tree> <log folder> -- <cargo arguments...>"
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TREE="$(cd "$1" && pwd)"
mkdir -p "$2"
LOGS="$(cd "$2" && pwd)"
shift 3
[ -f "$TREE/native/Cargo.toml" ] || fail "$TREE has no native/Cargo.toml."
LOG="$LOGS/cargo-$(date '+%Y%m%d-%H%M%S').log"

source "$TESTDIR/hold.sh"
CACHE="$S5WIN/cache.qcow2"
work=""
on_exit() { if [ -n "$work" ]; then rm -rf "$work"; fi; }
hold_take vm-cargo.sh
work="$(mktemp -d "$S5WIN/vm-cargo.XXXXXX")"
tar -cf "$work/tree.tar" -C "$TREE" .
printf '%s\n' "$@" >"$work/args.txt"
[ -f "$CACHE" ] || { qemu-img create -q -f qcow2 "$CACHE" 40G; echo "vm-cargo: made the cache disk $CACHE"; }

t_hold=$SECONDS
hold_boot -drive file="$CACHE",if=none,id=cache,format=qcow2,cache=writeback,discard=unmap,detect-zeroes=unmap \
  -device virtio-blk-pci,drive=cache
bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb | Out-Null'
bg_wait to_guest "$TESTDIR/guest/cargo.ps1" "$work/tree.tar" "$work/args.txt" C:/mcb/
bg_wait as bench 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\cargo.ps1 cache'
bg_wait as bench 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\cargo.ps1 unpack'
t0=$SECONDS
set +e
bg_wait as bench 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\cargo.ps1 cargo' 2>&1 | tee "$LOG"
code=${PIPESTATUS[0]}
set -e
echo "vm-cargo: cargo $* exited with $code after $((SECONDS - t0)) s ($((SECONDS - t_hold)) s of VM time so far); the cache disk takes $(du -h "$CACHE" | cut -f1); log $LOG" | tee -a "$LOG"
exit "$code"
