#!/usr/bin/env bash
# vm-first.sh <release folder>
#
# On the VM's host: the ways a person installs one Windows release, as the
# standard user plain, in a fresh overlay of the Windows 11 VM's hold slot
# (hold.sh, plain.sh). This script serves a staging site from its own folder
# at http://10.0.2.2:3191/ (the installer, its SHA256SUMS and the tree's
# native/site/install.ps1). Before every screenshot the Start menu is closed
# (guest/vm-test.ps1's escape).
#
#   1. install.ps1: `irm <site>/install.ps1 | iex` in a PowerShell window on
#      plain's desktop, with MCBENCH_LITE_RELEASES naming the site; the
#      install's checks; the console windows and the setup screen of the
#      first window; the same command again while the app it started
#      runs, which must pass on the installer's refusal; five true first
#      starts (first-start, timed to the first_frame line, server.pid looked
#      for after each quit); the uninstall; then
#      the site's installer with one byte changed, which must be refused
#      with nothing installed
#   2. the browser route: Microsoft Edge downloads the installer; its
#      Downloads panel is driven through UI Automation (Downloads, More
#      actions, Keep, Show more, Keep anyway, with a screenshot before each
#      click); when the file is not kept, the download is SIMULATED (the
#      installer with a Zone.Identifier of ZoneId 3). Explorer opens the file
#      as a double-click does; SmartScreen's dialog is answered More info,
#      then Run anyway (or the security warning, Run); the installer's Close
#      is pressed; the install's checks, with no administrator prompt
#   3. the uninstall and the comparison of plain's profile and HKCU before
#      and after
#
#   <release folder>  mcbench-lite-setup-x64.exe and
#                     mcbench-lite-windows-x86_64.tar.gz (the payload the
#                     installer must put in place)
#
# Edge's first-run pages are turned off by the policy HideFirstRunExperience
# in the overlay (HKLM, set as bench), so the download is the first thing a
# person sees; no other Edge setting is changed.
#
# Results go to ~/mcbench-native-scratch/s5/win/results/vm-first-<time>/.
set -euo pipefail

fail() { echo "vm-first.sh: $*" >&2; exit 1; }
[ "$#" = 1 ] || fail "usage: vm-first.sh <release folder>"
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REL="$(cd "$1" && pwd)"
ARCHIVE=mcbench-lite-windows-x86_64.tar.gz
SETUP=mcbench-lite-setup-x64.exe
for f in "$REL/$SETUP" "$REL/$ARCHIVE"; do [ -f "$f" ] || fail "$f is missing."; done
PORT=3191

source "$TESTDIR/hold.sh"
OUT="$S5WIN/results/vm-first-$(date '+%Y%m%d-%H%M%S')"
mkdir -p "$OUT"
work=""
server_pid=""
on_exit() {
  if [ "$hold_started" = 1 ]; then from_guest C:/mcb/results "$OUT/guest"; fi
  if [ -n "$server_pid" ]; then kill "$server_pid"; fi
  if [ -n "$work" ]; then rm -rf "$work"; fi
}
hold_take vm-first.sh
work="$(mktemp -d "$S5WIN/vm-first.XXXXXX")"
mkdir "$work/payload"
tar -xzf "$REL/$ARCHIVE" -C "$work/payload"
(cd "$work/payload" && sha256sum -- *) >"$work/expect.txt"
SITE="$work/site"
mkdir "$SITE"
cp "$REL/$SETUP" "$TESTDIR/../../../site/install.ps1" "$SITE/"
(cd "$SITE" && sha256sum "$SETUP") >"$SITE/SHA256SUMS"
python3 -I -m http.server "$PORT" --bind 127.0.0.1 --directory "$SITE" >"$OUT/server.log" 2>&1 &
server_pid=$!

hold_boot
ME=vm-first
source "$TESTDIR/plain.sh"
bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb | Out-Null; icacls C:\mcb /grant "plain:(OI)(CI)M" | Out-Null; $e = "HKLM:\SOFTWARE\Policies\Microsoft\Edge"; New-Item -Force $e | Out-Null; Set-ItemProperty $e HideFirstRunExperience 1 -Type DWord'
bg_wait to_guest "$TESTDIR/guest/vm-test.ps1" "$REL/$SETUP" "$work/expect.txt" C:/mcb/
plain_desktop
[ "$DESKTOP" = 1 ] || fail "plain's desktop did not start, so nothing of this test can run."
settle_before

BASE="http://10.0.2.2:$PORT"
# snap NAME: the Start menu closed, then the screen.
snap() {
  step bench desktop escape || echo "vm-first: the Start menu could not be closed before $1"
  screenshot "$OUT/$1"
}

# 1. install.ps1.
step bench desktop ps-install ok "$BASE" 0
sleep 10  # the app install.ps1 started opens its window
snap 1-ps-install.png
step plain check-install C:\\mcb\\expect.txt
# The first window install.ps1 opened: its console windows (none of the
# app's), the setup screen's WSL sentence on this machine without WSL.
step bench desktop consoles
step bench desktop wsl-screen || echo "vm-first: wsl-screen found no text; read 1-ps-install.png"
step bench desktop ps-install running "$BASE" 2
snap 1b-ps-install-while-running.png
step bench desktop quit
# Five true first starts of that install.
echo "vm-first: the host's load before the first starts: $(cut -d' ' -f1-3 /proc/loadavg)"
for n in 1 2 3 4; do step bench desktop first-start "$n" close; done
step bench desktop first-start 5 keep
snap 1d-first-start.png
step bench desktop quit
echo "vm-first: the host's load after the first starts: $(cut -d' ' -f1-3 /proc/loadavg)"
step plain first-summary
step plain ps-close ok
step plain ps-close running
step plain uninstall
python3 -I -c 'import sys; b = bytearray(open(sys.argv[1], "rb").read()); b[len(b) // 2] ^= 0xff; open(sys.argv[1], "wb").write(b)' "$SITE/$SETUP"
step bench desktop ps-install changed "$BASE" 1
snap 1c-ps-install-changed-byte.png
step plain not-installed
step plain ps-close changed
cp "$REL/$SETUP" "$SITE/$SETUP"

# 2. The browser route.
# edge_keep: Edge's Downloads panel, a screenshot before each click.
edge_keep() {
  snap 2a-edge-panel.png
  step bench desktop click downloads 10 || return 1
  snap 2b-edge-downloads.png
  step bench desktop click more 10 || return 1
  snap 2c-edge-more.png
  step bench desktop click keep 10 || return 1
  snap 2d-edge-keep.png
  if step bench desktop find show-more 5; then
    step bench desktop click show-more 10 || return 1
    snap 2e-edge-show-more.png
  fi
  step bench desktop click keep-anyway 15 || return 1
  sleep 3
  snap 2f-edge-kept.png
}
if step bench desktop edge "$BASE/$SETUP"; then
  snap 2-edge.png
  echo "vm-first: Edge kept the download without being asked"
elif edge_keep && step plain downloaded; then
  echo "vm-first: Edge kept the download after Keep"
else
  snap 2-edge-not-kept.png
  echo "vm-first: SIMULATED: Edge did not keep the file; the mark of the web is set by the test"
  step plain motw C:\\mcb\\$SETUP
fi
step bench desktop edge-close
step bench desktop open-explorer
snap 3-opened.png
if step bench desktop find smartscreen 20; then
  snap 4-smartscreen.png
  step bench desktop click more-info 15
  snap 5-more-info.png
  step bench desktop click run-anyway 15
elif step bench desktop find warning 3; then
  snap 4-security-warning.png
  step bench desktop click run 15
else
  echo "vm-first: no SmartScreen dialog and no security warning appeared"
fi
sleep 8
snap 6-installer.png
step bench desktop finish-setup 120
step plain check-install C:\\mcb\\expect.txt
step plain forget-download

# 3. The uninstall and the comparison.
step plain uninstall
step plain listing after
step plain compare before after | tee "$OUT/compare.txt"
echo "vm-first: done; results in $OUT"
