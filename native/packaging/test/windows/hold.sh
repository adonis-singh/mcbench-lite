# hold.sh: the take, the boot, the stop and the release of the Windows VM's
# hold slot for stage five's VM scripts (vm-test.sh, vm-build.sh), sourced by
# them on adipc from a tree brought there by content (COMMON's rsync line).
#
# The sequence, in the VM owner's lines (native/tools/winvm/env.sh, the two
# slots of 2026-11-17):
#   export WINVM_SLOT=hold                         first, before anything else
#   line="<who>, overlay <path>, pid $$, started $(date '+%Y-%m-%d %H:%M:%S')"
#   bash ~/mcbench-native-scratch/winvm/scripts/lock.sh take "$line" 3600
#   export WINVM_LOCK_LINE="$line"
#   source ~/mcbench-native-scratch/winvm/scripts/env.sh     only after the take
#   an overlay of our own on the never-written base win11-tools, with our own
#     copies of its OVMF vars and TPM state
#   vm_start <disk> <vars> <tpm> [qemu args]; wait_ssh 900
#   ssh and scp through $(ssh_opts) (the hold slot's port, 3156), as bench
#     (administrator) or plain (standard user)
#   bash .../stop.sh; bash .../lock.sh release "$line"   in one exit trap
#
# The hold slot's VM runs on host threads 20-23, pinned by vm_start in
# systemd scopes under mcbench.slice (no taskset or scope here), with 12G unless WINVM_MEM says otherwise; those threads are
# outside the shared pool, so a hold is the slot's LOCK only. vm_start
# refuses with a sentence when adipc has less memory available than the
# slot's size plus 8 GB: the script then ends through the trap; run it again
# later, never in a loop. QEMU is stopped only by stop.sh, through the slot's
# pid file that vm_start wrote. The check slot's cache disk is never attached
# (vm_start refuses it); vm-build.sh attaches stage five's own.
#
# One trap (hold_release, on EXIT) runs the caller's on_exit, stops the VM,
# deletes the overlay and releases the LOCK, on every exit, failed or
# interrupted.

export WINVM_SLOT=hold
WINVM="$HOME/mcbench-native-scratch/winvm"
S5WIN="$HOME/mcbench-native-scratch/s5/win"
OVERLAY="$S5WIN/run"
# The base the overlay is made on: win11-tools (no WSL) unless the caller
# names another in WINVM_HOLD_BASE, such as win11-tools-wsl (WSL installed
# once; native/README.md, The WSL tests).
HOLD_BASE="${WINVM_HOLD_BASE:-win11-tools}"
HOLD_REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../tools/winvm" && pwd)"
hold_locked=0
hold_made=0
hold_started=0

hold_release() {
  set +e
  # A second INT, TERM or HUP while this runs would run the 'exit 130' trap
  # again and end the release half done (2026-11-18 09:37: two TERMs left the
  # LOCK with a dead line and the overlay in place); from here they are ignored.
  trap '' PIPE INT TERM HUP
  if declare -F on_exit >/dev/null; then on_exit; fi
  if [ "$hold_started" = 1 ]; then bash "$WINVM/scripts/stop.sh"; fi
  if [ "$hold_made" = 1 ]; then rm -rf "$OVERLAY"; fi
  if [ "$hold_locked" = 1 ]; then bash "$WINVM/scripts/lock.sh" release "$HOLD_LINE"; fi
}

# hold_take WHO: takes the hold slot's LOCK (waiting up to an hour), sources
# env.sh and makes this run's overlay. The trap is set before the take.
hold_take() {
  local who="$1"
  trap hold_release EXIT
  trap 'exit 130' INT TERM HUP PIPE
  # The VM goes only through the repository's scripts (native/check.sh's rule).
  for script in lock.sh env.sh start.sh stop.sh; do
    cmp -s "$HOLD_REPO/$script" "$WINVM/scripts/$script" || { echo "$who: $WINVM/scripts/$script is missing or differs from this tree's native/tools/winvm/$script; the VM's owner installs the scripts" >&2; exit 1; }
  done
  HOLD_LINE="s5-win $who, overlay $OVERLAY ($HOLD_BASE), pid $$, started $(date '+%Y-%m-%d %H:%M:%S')"
  bash "$WINVM/scripts/lock.sh" take "$HOLD_LINE" 3600 || { echo "$who: the Windows VM's hold slot was not free within 3600 s" >&2; exit 1; }
  hold_locked=1
  export WINVM_LOCK_LINE="$HOLD_LINE"
  # env.sh: vm_start, vm_running, wait_ssh, ssh_opts, qmp, BASES, SLOTDIR (it sets its own HERE).
  source "$WINVM/scripts/env.sh"
  if vm_running; then echo "$who: the hold slot's VM runs (qemu pid $(cat "$SLOTDIR/qemu.pid")) although its LOCK was free" >&2; exit 1; fi
  hold_made=1
  rm -rf "$OVERLAY"
  mkdir -p "$OVERLAY"
  qemu-img create -q -f qcow2 -b "$BASES/$HOLD_BASE.qcow2" -F qcow2 "$OVERLAY/disk.qcow2"
  cp "$BASES/$HOLD_BASE.vars.fd" "$OVERLAY/vars.fd"
  cp -r "$BASES/$HOLD_BASE.tpm" "$OVERLAY/tpm"
  # A base kept read-only (the -wsl one is) gives read-only copies; the
  # overlay's firmware variables and TPM state must be writable.
  chmod -R u+w "$OVERLAY/vars.fd" "$OVERLAY/tpm"
}

# hold_boot [qemu arguments]: starts the VM on the overlay and waits for ssh.
hold_boot() {
  # COMMON's rule for adipc: no VM start while MemAvailable is under 12 GB
  # (vm_start itself refuses under the slot's size plus 8 GB).
  local avail_kib
  avail_kib=$(awk '/^MemAvailable:/ { print $2 }' /proc/meminfo)
  [ "$avail_kib" -ge $((12 * 1024 * 1024)) ] || { echo "hold.sh: adipc has $((avail_kib / 1048576)) GB available, under the 12 GB a VM start needs; run again later" >&2; exit 1; }
  hold_started=1
  vm_start "$OVERLAY/disk.qcow2" "$OVERLAY/vars.fd" "$OVERLAY/tpm" "$@"
  wait_ssh 900
}

# A guest command runs in the background and is waited for, so a TERM or INT
# reaches the trap at once (native/check.sh's bg_wait).
bg_wait() { "$@" & wait $!; }
# as USER COMMAND: PowerShell in the guest over ssh as bench or plain; 60 s for
# the banner, as check.sh (ssh takes an option's first value).
as() { local user="$1"; shift; ssh -o ConnectTimeout=60 $(ssh_opts) "$user@127.0.0.1" "$@"; }
# to_guest SRC... DEST and from_guest SRC DEST, as bench; guest paths like C:/mcb/x.
to_guest() { local dest="${*: -1}"; scp -q -r -o ConnectTimeout=60 $(ssh_opts | sed 's/-p /-P /') "${@:1:$#-1}" "bench@127.0.0.1:$dest"; }
from_guest() { scp -q -r -o ConnectTimeout=60 $(ssh_opts | sed 's/-p /-P /') "bench@127.0.0.1:$1" "$2"; }
# screenshot OUT.png: QEMU's screendump of the guest's display through the
# slot's QMP socket (env.sh's qmp, which runs the installed winvm/scripts/qmp.py).
screenshot() { qmp screendump "$(realpath -m "$1")"; }
