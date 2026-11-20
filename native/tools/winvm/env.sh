# Sourced by every winvm script on adipc. Everything lives under $R.
#
# Two VMs can run at once, one per slot, named by WINVM_SLOT (no default):
#   check  native/check.sh --windows only; it owns the shared cache disk
#          ~/mcbench-native-scratch/check-windows/cache.qcow2
#   hold   every other use (long passes by other sessions, an agent's own overlay)
# check.sh uses the check slot; everything else takes the hold slot; each slot
# has its own LOCK. Each slot has its own LOCK ($R/LOCK-<slot>), its own state
# in $R/vm/<slot>/ (QEMU's pid file, the monitor, VNC and swtpm sockets,
# swtpm's pid file, start.sh's and reset.sh's run overlay with its TPM state),
# its own ssh port and four host threads of its own; both boot overlays of the
# same never-written bases in $R/vm/bases. QEMU and swtpm run in systemd user
# scopes under mcbench.slice (vm_scope), pinned to the slot's threads.
#
#   slot   ssh port  host threads (physical cores)  memory default
#   check  3155      28-31 (cores 12-15)             16G
#   hold   3156      20-23 (cores 4-7)               12G
#
# The VM of a slot has one user at a time, who holds that slot's LOCK
# (scripts/lock.sh). Every start, stop, reset and snapshot passes vm_running
# (start.sh and every caller of vm_start through it, stop.sh, reset.sh and
# snapshot.sh directly), so the check lives there, in one place: it exits,
# refusing, unless $WINVM_LOCK_LINE is the slot's LOCK's line. A caller's
# sequence, for the hold slot:
#   export WINVM_SLOT=hold
#   line="<who, overlay, pid, start time>"; lock.sh take "$line" [seconds]
#   export WINVM_LOCK_LINE="$line"; start.sh ...; ...; stop.sh; lock.sh release "$line"
# A caller sources this file only after its take, so it loads what is
# installed when its turn comes.
# The repository's copy is native/tools/winvm/env.sh; native/check.sh fails
# when the installed one differs from it.
set -euo pipefail
R=$HOME/mcbench-native-scratch/winvm
case "${WINVM_SLOT:-}" in
  check) SSH_PORT=3155; CORES=28-31; MEM=${WINVM_MEM:-16G} ;;
  hold) SSH_PORT=3156; CORES=20-23; MEM=${WINVM_MEM:-12G} ;;
  *)
    echo "winvm: refused: WINVM_SLOT $([ -n "${WINVM_SLOT:-}" ] && echo "is '$WINVM_SLOT', not a slot" || echo "is not set"); name the VM's slot: export WINVM_SLOT=check for native/check.sh --windows, export WINVM_SLOT=hold for every other use; each slot has its own LOCK" >&2
    exit 1
    ;;
esac
SLOT=$WINVM_SLOT
export TMPDIR=$R/tmp
mkdir -p "$TMPDIR"
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
VM=$R/vm
BASES=$VM/bases
SLOTDIR=$VM/$SLOT
RUN=$SLOTDIR/run
LOGS=$R/logs
ISO=$R/iso
WIN_ISO=$ISO/win11-ent-eval-26h2-en-us.iso
VIRTIO_ISO=$ISO/virtio-win-0.1.302.iso
UNATTEND_ISO=$VM/unattend.iso
SWTPM=$R/prefix/bin/swtpm
PY="python3 -I"          # qmp.py: stdlib only
VENV=$R/venv              # venv.sh: pycdlib, virt-firmware (install.sh), numpy+pillow (pngdiff.py)
QMP_SOCK=$SLOTDIR/qmp.sock
SSH_KEY=$VM/ssh/winvm_ed25519
CPUS=${WINVM_CPUS:-12}
# std = QEMU standard VGA (Windows: Microsoft Basic Display Adapter);
# virtio = virtio-vga (Windows: viogpudo, a display-only driver; Direct3D stays on WARP).
DISPLAY_DEV=${WINVM_DISPLAY:-std}
RES=${WINVM_RES:-1920x1080}

qmp() { $PY "$HERE/qmp.py" "$QMP_SOCK" "$@"; }

WINVM_LOCK=$R/LOCK-$SLOT
vm_lock_held() {
  local held
  held=$(cat "$WINVM_LOCK" 2>/dev/null) || held=""
  if [ -z "$held" ] || [ "${WINVM_LOCK_LINE:-}" != "$held" ]; then
    echo "winvm: refused: the $SLOT slot's LOCK $WINVM_LOCK $([ -n "$held" ] && echo "says: $held" || echo "does not exist"), and WINVM_LOCK_LINE $([ -n "${WINVM_LOCK_LINE:-}" ] && echo "is: $WINVM_LOCK_LINE" || echo "is not set"); with WINVM_SLOT=$SLOT exported, take the slot first with lock.sh take \"\$line\" [seconds], then export WINVM_LOCK_LINE=\"\$line\", and after stop.sh release it with lock.sh release \"\$line\"" >&2
    exit 1
  fi
}

vm_running() { vm_lock_held; [ -f "$SLOTDIR/qemu.pid" ] && kill -0 "$(cat "$SLOTDIR/qemu.pid")" 2>/dev/null; }

# Preconditions of vm_start; each fails at once with a sentence.
#
# Memory: adipc is the owner's desktop and kiosk, so a VM starts only when
# MemAvailable is at least the slot's MEM plus 8 GiB.
vm_memory_free() {
  local avail_kib need_kib
  case "$MEM" in
    *G) need_kib=$(( ${MEM%G} * 1024 * 1024 )) ;;
    *M) need_kib=$(( ${MEM%M} * 1024 )) ;;
    *) echo "winvm: refused: the $SLOT slot's memory is '$MEM'; WINVM_MEM is <n>G or <n>M" >&2; exit 1 ;;
  esac
  need_kib=$(( need_kib + 8 * 1024 * 1024 ))
  avail_kib=$(awk '/^MemAvailable:/ { print $2 }' /proc/meminfo)
  if [ "$avail_kib" -lt "$need_kib" ]; then
    echo "winvm: refused: adipc has $(awk -v k="$avail_kib" 'BEGIN { printf "%.1f", k / 1048576 }') GB available, and the $SLOT slot's VM needs $MEM plus 8 GB left for the owner's desktop; stop the other VM or wait" >&2
    exit 1
  fi
}
# The check slot's cache disk: two QEMU processes writing one qcow2 corrupt
# it, so a hold slot's VM never opens it, by any spelling of its path (each
# word of every argument is resolved with realpath, and an existing file is
# also compared by device and inode, which a symlink or a hard link keeps),
# directly or as a backing file of a disk it opens.
CHECK_CACHE=$HOME/mcbench-native-scratch/check-windows/cache.qcow2
vm_not_check_cache() {
  local cache_path cache_id word file
  cache_path=$(realpath -m "$CHECK_CACHE")
  cache_id=$(stat -L -c %d:%i "$CHECK_CACHE" 2>/dev/null) || cache_id=""
  while IFS= read -r word; do
    [ -n "$word" ] || continue
    { echo "$word"
      if [ -f "$word" ] && qemu-img info -U "$word" >/dev/null 2>&1; then
        qemu-img info -U --backing-chain --output=json "$word" | python3 -I -c 'import json, sys
for image in json.load(sys.stdin): print(image["filename"])' 2>/dev/null
      fi
    } | while IFS= read -r file; do
      if [ "$(realpath -m -- "$file")" = "$cache_path" ] \
        || { [ -n "$cache_id" ] && [ -e "$file" ] && [ "$(stat -L -c %d:%i -- "$file")" = "$cache_id" ]; }; then
        echo "winvm: refused: the hold slot's VM would open $file, which is the check slot's cache disk $CHECK_CACHE; only native/check.sh in the check slot attaches it" >&2
        exit 1
      fi
    done || exit 1
  done < <(printf '%s\n' "$@" | tr ',={}"' '\n\n\n\n\n')
}

# vm_scope WHAT COMMAND...: runs COMMAND in a systemd user scope of its own,
# mcbench-winvm-<slot>-<what>-<caller's pid>.scope in mcbench-vm.slice, a child of
# mcbench.slice: the slice whose CPU quota bounds all this work on adipc, so
# a VM's use counts in that total instead of being reserved outside it. A
# program that makes itself a daemon stays in the scope, which ends with its
# last process and is collected then, also when that process was killed.
vm_scope() {
  local what=$1; shift
  systemd-run --user --scope --quiet --collect --slice=mcbench-vm.slice \
    --unit="mcbench-winvm-$SLOT-$what-$$" -- "$@"
}

# vm_start DISK VARS TPMDIR [extra qemu args...]: swtpm and QEMU pinned to the
# slot's host threads, each in a scope of its own under mcbench.slice.
vm_start() {
  local disk=$1 vars=$2 tpm=$3; shift 3
  if vm_running; then echo "VM already running in slot $SLOT (pid $(cat "$SLOTDIR/qemu.pid"))" >&2; return 1; fi
  if [ "$SLOT" = hold ]; then vm_not_check_cache "$disk" "$vars" "$tpm" "$@"; fi
  vm_memory_free
  mkdir -p "$tpm" "$LOGS" "$SLOTDIR"
  rm -f "$SLOTDIR/swtpm.sock" "$QMP_SOCK" "$SLOTDIR/vnc.sock"
  vm_scope swtpm env LD_LIBRARY_PATH="$R/prefix/lib" taskset -c "$CORES" "$SWTPM" socket --tpm2 \
    --tpmstate dir="$tpm" \
    --ctrl type=unixio,path="$SLOTDIR/swtpm.sock" \
    --pid file="$SLOTDIR/swtpm.pid" \
    --log file="$LOGS/swtpm-$SLOT.log",level=1 \
    --terminate --daemon
  local w=${RES%x*} h=${RES#*x} vga
  case $DISPLAY_DEV in
    std) vga="-device VGA,vgamem_mb=64,edid=on,xres=$w,yres=$h" ;;
    virtio) vga="-device virtio-vga,edid=on,xres=$w,yres=$h" ;;
    *) echo "unknown WINVM_DISPLAY=$DISPLAY_DEV" >&2; return 1 ;;
  esac
  # -cpu host exposes svm (nested virtualisation: Hyper-V / WSL2 in the guest).
  vm_scope qemu taskset -c "$CORES" qemu-system-x86_64 -name "winvm-$SLOT" \
    -machine q35,smm=on,accel=kvm,hpet=off \
    -global driver=cfi.pflash01,property=secure,value=on \
    -global ICH9-LPC.disable_s3=1 \
    -drive if=pflash,format=raw,unit=0,readonly=on,file=/usr/share/edk2/x64/OVMF_CODE.secboot.4m.fd \
    -drive if=pflash,format=raw,unit=1,file="$vars" \
    -cpu host,+svm,hv_relaxed,hv_vapic,hv_spinlocks=0x1fff,hv_time \
    -smp "$CPUS",sockets=1,cores="$CPUS",threads=1 -m "$MEM" \
    -rtc base=utc,clock=host \
    -chardev socket,id=chrtpm,path="$SLOTDIR/swtpm.sock" -tpmdev emulator,id=tpm0,chardev=chrtpm -device tpm-crb,tpmdev=tpm0 \
    -drive file="$disk",if=none,id=d0,format=qcow2,cache=writeback,discard=unmap,detect-zeroes=unmap \
    -device virtio-blk-pci,drive=d0,bootindex=1 \
    -netdev user,id=n0,hostfwd=tcp:127.0.0.1:$SSH_PORT-:22 \
    -device virtio-net-pci,netdev=n0,romfile= \
    -vga none $vga \
    -device qemu-xhci -device usb-tablet \
    -display none -vnc unix:"$SLOTDIR/vnc.sock" \
    -qmp unix:"$QMP_SOCK",server=on,wait=off \
    -pidfile "$SLOTDIR/qemu.pid" -daemonize \
    "$@"
  echo "VM started in slot $SLOT: qemu pid $(cat "$SLOTDIR/qemu.pid"), swtpm pid $(cat "$SLOTDIR/swtpm.pid"), host threads $CORES, ssh port $SSH_PORT"
}

ssh_opts() {
  echo -i "$SSH_KEY" -p "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR -o ConnectTimeout=5 -o IdentitiesOnly=yes
}

# wait_ssh SECONDS: poll until sshd answers a command.
wait_ssh() {
  local limit=$1 s=$SECONDS
  while (( SECONDS - s < limit )); do
    if ssh $(ssh_opts) bench@127.0.0.1 'echo ok' 2>/dev/null | grep -q ok; then
      echo "ssh up after $((SECONDS - s))s"; return 0
    fi
    sleep 3
  done
  echo "ssh not up after ${limit}s" >&2; return 1
}
