#!/usr/bin/env bash
# vm-wsl.sh <release folder> <sandbox assets folder> plain|direct|wsl
#
# On the VM's host: one Windows release, as the standard user plain, on one
# machine:
#   plain  the hold slot's usual base (no WSL): install.ps1, which starts the
#          app through Explorer; the PowerShell window that ran it shows no
#          [renderer] line; that window is closed, and 10 s later the app
#          still runs and shows its window (screenshots before and after)
#   direct the usual base: the installer run silently, then a PowerShell
#          window runs the installed mcbench-lite.exe directly; the app's
#          window shows, the prompt comes back, the window shows no line of
#          the app's; that window is closed, and 10 s later the app still
#          runs and shows its window (screenshot); quit, then the uninstall
#          (the data folder stays)
#   wsl    the base win11-tools-wsl (WSL installed once; through hold.sh's
#          WINVM_HOLD_BASE): the steps below
#
#   1. install.ps1 from this script's staging site (as vm-first.sh serves it,
#      http://10.0.2.2:3191/), which starts the app: its first window, a
#      screenshot, its console windows
#   2. through the installed program's headless turn (guest/vm-test.ps1's
#      wsl-turn): one build call and two bash calls, the first timed; the
#      sandbox assets come from <sandbox assets folder>, served at
#      http://10.0.2.2:3192/assets/ (the address the release was built
#      with, MCBENCH_SANDBOX_ASSETS_URL), whose requests go to assets.log
#
#   <release folder>        mcbench-lite-setup-x64.exe
#   <sandbox assets folder> guest-base-x86_64.tar.zst and rootfs-x86_64.tar.zst
#
# Results go to ~/mcbench-native-scratch/s5/win/results/vm-wsl-<time>/.
set -euo pipefail

fail() { echo "vm-wsl.sh: $*" >&2; exit 1; }
[ "$#" = 3 ] || fail "usage: vm-wsl.sh <release folder> <sandbox assets folder> plain|direct|wsl"
MODE="$3"
case "$MODE" in plain | direct | wsl) ;; *) fail "the run $MODE is none of plain, direct and wsl." ;; esac
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REL="$(cd "$1" && pwd)"
ASSETS="$(cd "$2" && pwd)"
SETUP=mcbench-lite-setup-x64.exe
for f in "$REL/$SETUP" "$REL/mcbench-lite-windows-x86_64.tar.gz" "$ASSETS/guest-base-x86_64.tar.zst" "$ASSETS/rootfs-x86_64.tar.zst"; do [ -f "$f" ] || fail "$f is missing."; done

# With WSL installed, Windows runs as the root partition of its own Hyper-V
# under KVM and stalls after ssh first answers when it has more virtual
# processors than the slot's four host threads (as the check slot found).
if [ "$MODE" = wsl ]; then export WINVM_HOLD_BASE=win11-tools-wsl WINVM_CPUS=4; fi
source "$TESTDIR/hold.sh"
OUT="$S5WIN/results/vm-wsl-$MODE-$(date '+%Y%m%d-%H%M%S')"
mkdir -p "$OUT"
work=""
server_pids=()
on_exit() {
  if [ "$hold_started" = 1 ]; then from_guest C:/mcb/results "$OUT/guest"; fi
  for p in "${server_pids[@]}"; do kill "$p"; done
  if [ -n "$work" ]; then rm -rf "$work"; fi
}
hold_take vm-wsl.sh
work="$(mktemp -d "$S5WIN/vm-wsl.XXXXXX")"
SITE="$work/site"
mkdir -p "$SITE" "$work/served/assets"
cp "$REL/$SETUP" "$TESTDIR/../../../site/install.ps1" "$SITE/"
(cd "$SITE" && sha256sum "$SETUP") >"$SITE/SHA256SUMS"
ln -s "$ASSETS/guest-base-x86_64.tar.zst" "$ASSETS/rootfs-x86_64.tar.zst" "$work/served/assets/"
python3 -I -m http.server 3191 --bind 127.0.0.1 --directory "$SITE" >"$OUT/server.log" 2>&1 &
server_pids+=($!)
python3 -I -m http.server 3192 --bind 127.0.0.1 --directory "$work/served" >"$OUT/assets.log" 2>&1 &
server_pids+=($!)

hold_boot
ME=vm-wsl
source "$TESTDIR/plain.sh"
bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb | Out-Null; icacls C:\mcb /grant "plain:(OI)(CI)M" | Out-Null'
mkdir "$work/payload"
tar -xzf "$REL/mcbench-lite-windows-x86_64.tar.gz" -C "$work/payload"
(cd "$work/payload" && sha256sum -- *) >"$work/expect.txt"
bg_wait to_guest "$TESTDIR/guest/vm-test.ps1" "$REL/$SETUP" "$work/expect.txt" C:/mcb/
plain_desktop
[ "$DESKTOP" = 1 ] || fail "plain's desktop did not start, so nothing of this test can run."
if [ "$MODE" = wsl ]; then bg_wait as bench 'wsl.exe --status; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }'; fi
snap() {
  step bench desktop escape || echo "vm-wsl: the Start menu could not be closed before $1"
  screenshot "$OUT/$1"
}

if [ "$MODE" = direct ]; then
  step plain install C:\\mcb\\$SETUP C:\\mcb\\expect.txt
  step bench desktop ps-direct
  snap 1-direct-start.png
  step bench desktop console-text
  step bench desktop close-console
  sleep 10
  step bench desktop app-alive
  snap 2-after-closing-powershell.png
  step bench desktop quit
  step plain uninstall
  echo "vm-wsl: done; results in $OUT"
  exit 0
fi
step bench desktop ps-install ok http://10.0.2.2:3191 0
sleep 10  # the app install.ps1 started opens its window
snap 1-first-window.png
step bench desktop consoles
if [ "$MODE" = plain ]; then
  step bench desktop console-text
  step bench desktop close-console
  sleep 10
  step bench desktop app-alive
  snap 2-after-closing-powershell.png
  step bench desktop quit
else
  step bench desktop quit
  step plain ps-close ok
  step bench desktop wsl-turn
fi
echo "vm-wsl: the assets server's log:"
cat "$OUT/assets.log"
echo "vm-wsl: done; results in $OUT"
