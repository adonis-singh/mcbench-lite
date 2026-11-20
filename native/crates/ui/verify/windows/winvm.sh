#!/bin/bash
# winvm.sh: drive the accessibility checks in the Windows VM from any machine that can ssh to the
# VM's host. See README.md in this folder.
#
# Environment (no defaults for paths, so nothing machine-specific is committed):
#   WINVM_SSH       ssh destination of the VM's host (e.g. the name in your ~/.ssh/config)
#   WINVM_LOCK_LINE the line you took the VM's LOCK with (winvm/scripts/lock.sh take "$line");
#                   every command refuses to run unless the LOCK holds exactly this line, and the
#                   shared runners get it exported
#   WINVM_WORK      folder on that host beside winvm/ (winvm/scripts are the shared runners), where
#                   `ps/`, `src/` and `evidence/` are kept, and whose `scripts/` holds the copy of
#                   start.sh and stop.sh whose env.sh points RUN at your own overlay and pins QEMU
#                   to your CPUs (the shared env.sh fixes RUN to the shared run/ and pins nothing)
#
#   winvm.sh ps FILE.ps1 [ARGS...]      copy a PowerShell script to C:\s4\ps and run it over SSH
#   winvm.sh probe ARGS...              uia-probe ARGS in the desktop session (guest/probe.ps1)
#   winvm.sh push LOCAL_DIR NAME        replace C:\s4\NAME with LOCAL_DIR (target/ left out)
#   winvm.sh build NAME [CARGO_ARGS]    cargo build --release in C:\s4\NAME (target C:\t\NAME)
#   winvm.sh shots GUEST_DIR NAME       C:\s4\GUEST_DIR\*.bmp -> $WINVM_WORK/evidence/NAME/*.png
#   winvm.sh vm SCRIPT ARGS...          a shared runner: key.sh, type.sh, click.sh, screenshot.sh ...
#   winvm.sh keys DELAY_S COMBO...      QMP key combos one at a time, DELAY_S apart (for screen readers)
#   winvm.sh move X Y                   absolute pointer move to guest pixel X,Y
#   winvm.sh start | stop               the VM on your overlay (WINVM_WORK/scripts/start.sh, stop.sh)
set -euo pipefail
: "${WINVM_SSH:?set WINVM_SSH}" "${WINVM_WORK:?set WINVM_WORK}" "${WINVM_LOCK_LINE:?set WINVM_LOCK_LINE}"
line=$(printf '%q' "$WINVM_LOCK_LINE")
ssh "$WINVM_SSH" "[ \"\$(cat '$WINVM_WORK/../winvm/LOCK' 2>/dev/null)\" = $line ]" \
  || { echo "winvm.sh: the VM's LOCK does not hold WINVM_LOCK_LINE; nothing sent" >&2; exit 3; }
here=$(cd "$(dirname "$0")" && pwd)
op=${1:?usage: see the header of winvm.sh}; shift
shared="$WINVM_WORK/../winvm/scripts"

remote() { ssh "$WINVM_SSH" "export WINVM_LOCK_LINE=$line && cd '$shared' && $1"; }

case $op in
  ps)
    f=$1; shift; b=$(basename "$f")
    ssh "$WINVM_SSH" "mkdir -p '$WINVM_WORK/ps'"
    scp -q "$f" "$WINVM_SSH:$WINVM_WORK/ps/$b"
    remote "./ssh.sh 'New-Item -ItemType Directory -Force C:\\s4\\ps | Out-Null' && ./copy.sh '$WINVM_WORK/ps/$b' g:C:/s4/ps/$b && ./ssh.sh 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\\s4\\ps\\$b $*'"
    ;;
  probe)
    # Each argument is quoted for cmd.exe; arguments must not contain double quotes.
    args=$(printf '"%s" ' "$@")
    tmp=$(mktemp); printf '%s' "$args" > "$tmp"
    ssh "$WINVM_SSH" "mkdir -p '$WINVM_WORK/ps'"
    scp -q "$tmp" "$WINVM_SSH:$WINVM_WORK/ps/probe-args.txt"; rm -f "$tmp"
    scp -q "$here/guest/probe.ps1" "$WINVM_SSH:$WINVM_WORK/ps/probe.ps1"
    remote "./copy.sh '$WINVM_WORK/ps/probe.ps1' g:C:/s4/ps/probe.ps1 && ./copy.sh '$WINVM_WORK/ps/probe-args.txt' g:C:/s4/ps/probe-args.txt && ./ssh.sh 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\\s4\\ps\\probe.ps1 -ArgsFile C:\\s4\\ps\\probe-args.txt'" | tr -d '\r'
    ;;
  push)
    src=$1; name=$2
    ssh "$WINVM_SSH" "rm -rf '$WINVM_WORK/src/$name' && mkdir -p '$WINVM_WORK/src/$name'"
    COPYFILE_DISABLE=1 tar --no-xattrs -C "$src" --exclude target -cf - . | ssh "$WINVM_SSH" "tar -C '$WINVM_WORK/src/$name' -xf -"
    remote "./ssh.sh 'if (Test-Path C:\\s4\\$name) { Remove-Item -Recurse -Force C:\\s4\\$name }' && ./copy.sh '$WINVM_WORK/src/$name' g:C:/s4/"
    ;;
  build)
    name=$1; shift
    remote "./ssh.sh 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\\winvm\\build.ps1 -Dir C:\\s4\\$name -Args \"--release $*\"'"
    ;;
  shots)
    g=$1; name=$2
    remote "mkdir -p '$WINVM_WORK/evidence/$name' '$WINVM_WORK/tmp/bmp-$name' && ./copy.sh g:C:/s4/$g/. '$WINVM_WORK/tmp/bmp-$name/' && python3 -I -c '
import sys, pathlib
from PIL import Image
src, dst = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
for b in sorted(src.glob(\"*.bmp\")):
    Image.open(b).convert(\"RGB\").save(dst / (b.stem + \".png\"), optimize=True)
' '$WINVM_WORK/tmp/bmp-$name' '$WINVM_WORK/evidence/$name' && rm -rf '$WINVM_WORK/tmp/bmp-$name' && ./ssh.sh 'Remove-Item C:\\s4\\$g\\*.bmp'"
    ;;
  vm)
    s=$1; shift
    remote "./$s $*"
    ;;
  keys)
    d=$1; shift
    remote "for k in $*; do ./key.sh \$k; sleep $d; done"
    ;;
  move)
    remote "source ./env.sh && qmp move $1 $2"
    ;;
  start | stop)
    ssh "$WINVM_SSH" "export WINVM_LOCK_LINE=$line && cd '$WINVM_WORK/scripts' && ./$op.sh"
    ;;
  *) echo "unknown command $op; see the header of winvm.sh" >&2; exit 2 ;;
esac
