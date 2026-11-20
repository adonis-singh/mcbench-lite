#!/usr/bin/env bash
# lock.sh: a Windows VM slot's LOCK on adipc. Two VMs can run at once, one
# per slot, named by WINVM_SLOT (no default; env.sh has the table): check is
# for native/check.sh --windows only, hold for every other use. Each slot's VM
# has one user at a time, and every use of it (native/check.sh --windows-vm, a
# runner that calls start.sh and stop.sh, a person at a shell) takes the
# slot's LOCK first and releases it when the VM is stopped.
#
#   lock.sh take LINE [SECONDS]  writes LINE to the LOCK, which must not exist
#                                (bash's noclobber, set -C, so two takers never
#                                both win). When the LOCK is held it prints the
#                                holder's line and exits 1; with SECONDS it
#                                first waits up to that long, polling every 30 s
#                                and printing the holder's line each time it
#                                changes.
#   lock.sh release LINE         removes the LOCK when its line is LINE, the
#                                same line the caller took it with; a LOCK with
#                                any other line, or none, is left as it is and
#                                the call exits 1. Release is by the line, not
#                                by a pid, because the process that releases
#                                (a trap, a later shell) need not be the one
#                                that took it.
#
# LINE is one line that says who holds the VM and how to see whether it still
# runs: by convention it names the taker's program, its pid and when it
# started, for example
#   native/check.sh --windows, tree ..., pid 2185065, started 2026-11-17 10:10:56
#
# The slot's LOCK is the file LOCK-<slot> in the folder above this script's
# own folder: for the installed copy ~/mcbench-native-scratch/winvm/scripts/lock.sh
# that is ~/mcbench-native-scratch/winvm/LOCK-check or LOCK-hold. There is no
# other way to name it, so a copy of this script in another folder (a test)
# has LOCKs of its own.
#
# A stale LOCK (its holder died without releasing it): when the pid its line
# names is not running (ps -p <pid> prints no process), remove the file with
# rm. Never remove a LOCK whose holder runs.
#
# The repository's copy is native/tools/winvm/lock.sh; native/check.sh fails
# when the installed one differs from it.
set -euo pipefail

case "${WINVM_SLOT:-}" in
  check|hold) ;;
  *)
    echo "lock.sh: refused: WINVM_SLOT $([ -n "${WINVM_SLOT:-}" ] && echo "is '$WINVM_SLOT', not a slot" || echo "is not set"); name the VM's slot: export WINVM_SLOT=check for native/check.sh --windows, export WINVM_SLOT=hold for every other use; each slot has its own LOCK" >&2
    exit 2
    ;;
esac
LOCK="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/LOCK-$WINVM_SLOT"

stale_help() {
  echo "lock.sh: when the pid that line names is not running (ps -p <pid> prints no process), the LOCK is stale: remove it with rm $LOCK" >&2
}

case "${1:-}" in
  take)
    line="${2:?lock.sh take LINE [SECONDS]: no LINE}"
    wait="${3:-0}"
    deadline=$((SECONDS + wait))
    seen=""
    until (set -C; echo "$line" >"$LOCK") 2>/dev/null; do
      holder="$(cat "$LOCK" 2>/dev/null || true)"
      if [ "$SECONDS" -ge "$deadline" ]; then
        if [ "$wait" = 0 ]; then
          echo "lock.sh: the $WINVM_SLOT slot's LOCK $LOCK is held: $holder" >&2
        else
          echo "lock.sh: the $WINVM_SLOT slot's LOCK $LOCK was held for $wait s: $holder" >&2
        fi
        stale_help
        exit 1
      fi
      if [ "$holder" != "$seen" ]; then echo "lock.sh: waiting for the Windows VM's $WINVM_SLOT slot; $LOCK says: $holder"; seen="$holder"; fi
      left=$((deadline - SECONDS))
      sleep $((left < 30 ? left : 30))
    done
    ;;
  release)
    line="${2:?lock.sh release LINE: no LINE}"
    [ -f "$LOCK" ] || { echo "lock.sh: no LOCK to release at $LOCK" >&2; exit 1; }
    holder="$(cat "$LOCK")"
    [ "$holder" = "$line" ] || { echo "lock.sh: the LOCK $LOCK is not this caller's, left as it is; it says: $holder" >&2; exit 1; }
    rm -f "$LOCK"
    ;;
  *)
    echo "lock.sh: usage: lock.sh take LINE [SECONDS] | lock.sh release LINE" >&2
    exit 2
    ;;
esac
