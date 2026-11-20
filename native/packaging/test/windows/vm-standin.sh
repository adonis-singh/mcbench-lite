#!/usr/bin/env bash
# vm-standin.sh <stand-in payload folder> <stand-in installer>
#
# On adipc, the first short hold of the Windows VM (about 20 minutes, no build):
# the base image's inventory, then the stand-in installer for real as the
# standard user plain (silent install timed, files, install-source, shortcut,
# HKCU entry; a second install over the first; an install whose new files
# cannot be written (a read-only file in the way); the refusal while the stand-in
# mcbench-lite.exe runs; /SOURCE=winget; the silent uninstall timed; the profile
# and HKCU before and after), and a screenshot of the guest's desktop through
# QMP. The stand-in payload is a mcbench-lite.exe that is a console program
# sleeping ten minutes (so it can be running during the refusal test), a
# stand-in mcbench-build-child, and a version file (the installer's version and
# a newline), as installer.sh requires.
#
# Results go to ~/mcbench-native-scratch/s5/win/results/vm-standin-<time>/.
set -euo pipefail

fail() { echo "vm-standin.sh: $*" >&2; exit 1; }
[ "$#" = 2 ] || fail "usage: vm-standin.sh <stand-in payload folder> <stand-in installer>"
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAYLOAD="$(cd "$1" && pwd)" INSTALLER="$2"
[ -f "$INSTALLER" ] || fail "$INSTALLER does not exist."

source "$TESTDIR/hold.sh"
# The guest's results come back on every exit, before the trap stops the VM.
on_exit() { if [ "$hold_started" = 1 ]; then from_guest C:/mcb/results "$OUT/guest"; fi; }
OUT="$S5WIN/results/vm-standin-$(date '+%Y%m%d-%H%M%S')"
mkdir -p "$OUT"
(cd "$PAYLOAD" && sha256sum -- *) >"$OUT/expect.txt"
hold_take vm-standin.sh
t_hold=$SECONDS
hold_boot
echo "vm-standin: booted to ssh in $((SECONDS - t_hold)) s"
step() { local user="$1" script="$2"; shift 2; echo "vm-standin: $user: $script $*"; bg_wait as "$user" "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\$script $*"; }

bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb\results | Out-Null; icacls C:\mcb /grant "plain:(OI)(CI)M" | Out-Null'
bg_wait to_guest "$TESTDIR/guest/vm-test.ps1" "$TESTDIR/guest/standin.ps1" "$TESTDIR/guest/build.ps1" "$TESTDIR/../../build/windows.ps1" "$INSTALLER" "$OUT/expect.txt" C:/mcb/
setup="C:\\mcb\\$(basename "$INSTALLER")"

step bench standin.ps1 inventory | tee "$OUT/inventory.txt"
step bench standin.ps1 parse C:\\mcb\\vm-test.ps1 C:\\mcb\\standin.ps1 C:\\mcb\\build.ps1 C:\\mcb\\windows.ps1 | tee "$OUT/parse.txt" || true
screenshot "$OUT/desktop.png"
step plain vm-test.ps1 listing before
step plain vm-test.ps1 install "$setup" C:\\mcb\\expect.txt
step plain vm-test.ps1 install "$setup" C:\\mcb\\expect.txt
step plain standin.ps1 write-fails "$setup"
step plain vm-test.ps1 install "$setup" C:\\mcb\\expect.txt
step plain standin.ps1 refused "$setup"
step plain standin.ps1 data-folder
step plain vm-test.ps1 uninstall
step plain standin.ps1 winget "$setup"
step plain vm-test.ps1 uninstall
step plain vm-test.ps1 listing after
step plain vm-test.ps1 compare before after | tee "$OUT/compare.txt" || true
echo "vm-standin: done in $((SECONDS - t_hold)) s of hold; results in $OUT"
