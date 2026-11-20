#!/usr/bin/env bash
# vm-test.sh <release A folder> <release B folder>|- no-wsl|wsl
#
# With - for release B the run is the install part only: no feed is served,
# so the app's updater finds the feed unreachable, and the copy and update
# steps are left out.
#
# On the VM's host: the Windows install route end to end in a fresh overlay of the
# Windows 11 VM, as the standard user plain: install, first window, refusal
# while running, update, uninstall, and the profile and HKCU before and after.
# The update needs no click. What the app does (native/crates/updater/LATEST.md
# and src/flow.rs): at a start, the updater's thread asks the feed for
# latest.json and, for a newer version, downloads and verifies it within
# seconds and leaves it waiting beside the payload folder
# (%LOCALAPPDATA%\Programs\.mcbench-lite.update and .update.json); the NEXT
# start swaps it in before the core and runs the new program, whose core then
# removes the .old entries and writes DisplayVersion. A refusal is written to
# update.log once and remembered beside the install by the sha256 of the
# latest.json bytes that led to it. So the test, in this order:
#   A installed; the feed offers A: a start asks for latest.json only, and
#     nothing changes or is logged
#   the feed offers a latest.json whose entry's size is not the signed one: a
#     start refuses it, A stays whole, the reason is in update.log
#   the feed offers an archive with one byte changed: a start downloads it and
#     refuses it the same way (its latest.json is B's re-serialized, so its
#     bytes differ from B's own and the refusal is not remembered for B's)
#   the feed offers B: a copy of the app marked direct in another folder
#     updates itself and leaves DisplayVersion alone; then the install: a
#     start leaves B waiting, the window is closed, the next start swaps, and
#     the payload, the running program, DisplayVersion, the .old entries, the
#     waiting files and update.log are checked
#   MCBENCH_UI_LOG on the updated app, the uninstall, the after-listing
# Nothing may ask for an administrator during install or start (no consent.exe,
# the UAC prompt's program, may run).
#
# The machine, one per run:
#   no-wsl   the base as it is, where WSL is not installed: the first window
#            shows the setup screen with its WSL step (the owner's sentence
#            "To use bash and build, Windows needs WSL.", found through UI
#            Automation)
#   wsl      WSL installed first, as bench (wsl --install --no-distribution,
#            which downloads WSL, then the restart the plain logon needs
#            anyway), so WSL works before plain installs the app; a separate
#            run, so the no-wsl run never waits on it
# It takes the VM's hold slot through hold.sh. How long it holds the VM and
# what the base must have: README.md.
#
#   <release A folder>  a staging release of version A: mcbench-lite-setup-x64.exe
#                       and mcbench-lite-windows-x86_64.tar.gz (whose files are the
#                       payload the installer must put in place)
#   <release B folder>  a staging release of a newer version B: latest.json and
#                       mcbench-lite-windows-x86_64.tar.gz, served to the guest
#                       over QEMU's user network as the update feed
#
# The feed offering A is B's latest.json with A's version: the app reads
# nothing after a version equal to its own, so its other fields are never read.
#
# Version A's mcbench-lite.exe must be a staging build (windows.ps1 with
# MCBENCH_UPDATE_FEED=$FEED and MCBENCH_UPDATE_KEY_FILE naming the staging
# key that signed release B), so its updater reads release B from the host.
#
# Results (logs, listings, timings, screenshots) go to
# ~/mcbench-native-scratch/s5/win/results/vm-test-<time>/.
set -euo pipefail

fail() { echo "vm-test.sh: $*" >&2; exit 1; }
[ "$#" = 3 ] || fail "usage: vm-test.sh <release A folder> <release B folder>|- no-wsl|wsl"
MACHINE="$3"
case "$MACHINE" in no-wsl | wsl) ;; *) fail "the machine $MACHINE is neither no-wsl nor wsl." ;; esac
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
A="$(cd "$1" && pwd)"
ARCHIVE=mcbench-lite-windows-x86_64.tar.gz
UPDATE=1
if [ "$2" = - ]; then UPDATE=0; else B="$(cd "$2" && pwd)"; fi
for f in "$A/mcbench-lite-setup-x64.exe" "$A/$ARCHIVE"; do [ -f "$f" ] || fail "$f is missing."; done
if [ "$UPDATE" = 1 ]; then for f in "$B/latest.json" "$B/$ARCHIVE"; do [ -f "$f" ] || fail "$f is missing."; done; fi
# The staging feed as the guest sees the host: QEMU's user network maps
# 10.0.2.2 to the host's 127.0.0.1. The port is in stage five's 3190-3199.
FEED_PORT=3191
FEED="http://10.0.2.2:$FEED_PORT/latest.json"
if [ "$UPDATE" = 1 ]; then VERSION_B="$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["version"])' "$B/latest.json")"; fi

source "$TESTDIR/hold.sh"
OUT="$S5WIN/results/vm-test-$MACHINE-$(date '+%Y%m%d-%H%M%S')"
mkdir -p "$OUT"
work=""
server_pid=""
on_exit() {
  if [ "$hold_started" = 1 ]; then from_guest C:/mcb/results "$OUT/guest"; fi
  for t in "$OUT"/guest/*.time; do
    if [ -f "$t" ]; then echo "vm-test: $(cat "$t") ms"; fi
  done
  if [ -n "$server_pid" ]; then kill "$server_pid"; fi
  if [ -n "$work" ]; then rm -rf "$work"; fi
}
hold_take vm-test.sh
work="$(mktemp -d "$S5WIN/vm-test.XXXXXX")"

# The payload each release must leave installed: sha256sum lines of its archive's files.
expect() {
  mkdir "$work/$1"
  tar -xzf "$2/$ARCHIVE" -C "$work/$1"
  (cd "$work/$1" && sha256sum -- *) >"$work/expect-$1.txt"
}
expect A "$A"
VERSION_A="$(cat "$work/A/version")"
FEED_DIR="$work/feed"
# offer A|value|byte|B: what the feed serves from now on (above). Each file is
# written beside and renamed into place, so a request never reads half of one.
offer() {
  local latest="$FEED_DIR/.latest.json" archive="$FEED_DIR/.$ARCHIVE"
  case "$1" in
    A) python3 -I -c 'import json, sys; d = json.load(open(sys.argv[1])); d["version"] = sys.argv[2]; json.dump(d, sys.stdout, indent=2)' "$B/latest.json" "$VERSION_A" >"$latest"
       rm -f "$FEED_DIR/$ARCHIVE" ;;
    value) python3 -I -c 'import json, sys; d = json.load(open(sys.argv[1])); d["assets"]["windows-x86_64"]["size"] += 1; json.dump(d, sys.stdout, indent=2)' "$B/latest.json" >"$latest"
       cp "$B/$ARCHIVE" "$archive" ;;
    byte) python3 -I -c 'import json, sys; json.dump(json.load(open(sys.argv[1])), sys.stdout, indent=1)' "$B/latest.json" >"$latest"
       python3 -I -c 'import sys; b = bytearray(open(sys.argv[1], "rb").read()); b[len(b) // 2] ^= 0xff; open(sys.argv[2], "wb").write(b)' "$B/$ARCHIVE" "$archive" ;;
    B) cp "$B/latest.json" "$latest"
       cp "$B/$ARCHIVE" "$archive" ;;
  esac
  mv -f "$latest" "$FEED_DIR/latest.json"
  if [ -f "$archive" ]; then mv -f "$archive" "$FEED_DIR/$ARCHIVE"; fi
  echo "vm-test: the feed offers $1: $(sha256sum "$FEED_DIR/latest.json" | cut -c1-16) latest.json$( [ -f "$FEED_DIR/$ARCHIVE" ] && echo ", $(sha256sum "$FEED_DIR/$ARCHIVE" | cut -c1-16) $ARCHIVE")"
}
# The feed's requests since feed_mark, from http.server's log.
feed_mark() { feed_at=$(wc -l <"$OUT/feed.log"); }
feed_new() { tail -n +"$((feed_at + 1))" "$OUT/feed.log" | grep -F '"GET ' || true; }
# wait_feed: up to 120 s for the app to ask for latest.json since feed_mark.
wait_feed() {
  local t0=$SECONDS
  until [[ "$(feed_new)" == *'"GET /latest.json '* ]]; do
    (( SECONDS - t0 < 120 )) || fail "the app asked the feed for no latest.json within 120 s of its start."
    sleep 2
  done
}
# feed_got yes|no: whether the archive was asked for since feed_mark, which
# must be the given answer; prints the requests.
feed_got() {
  local got=no
  [[ "$(feed_new)" == *"\"GET /$ARCHIVE "* ]] && got=yes
  feed_new | sed 's/^/vm-test: feed: /'
  [ "$got" = "$1" ] || fail "the archive was asked for: $got, where the test expects $1."
}
if [ "$UPDATE" = 1 ]; then
  expect B "$B"
  mkdir "$FEED_DIR"
  offer A
  python3 -I -m http.server "$FEED_PORT" --bind 127.0.0.1 --directory "$FEED_DIR" >"$OUT/feed.log" 2>&1 &
  server_pid=$!
fi

hold_boot
ME=vm-test
source "$TESTDIR/plain.sh"

bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb | Out-Null; icacls C:\mcb /grant "plain:(OI)(CI)M" | Out-Null'
bg_wait to_guest "$TESTDIR/guest/vm-test.ps1" "$A/mcbench-lite-setup-x64.exe" "$work"/expect-*.txt C:/mcb/

if [ "$MACHINE" = wsl ]; then
  echo "vm-test: installing WSL as bench"
  bg_wait as bench 'wsl.exe --install --no-distribution; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }'
fi
plain_desktop
if [ "$MACHINE" = wsl ]; then bg_wait as bench 'wsl.exe --status; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }'; fi

settle_before
step plain install C:\\mcb\\mcbench-lite-setup-x64.exe C:\\mcb\\expect-A.txt

# The copy marked direct in C:\mcb\copy updates itself to B and leaves the
# installer's uninstall entry alone (its InstallLocation is not the copy's).
# It shares the installed app's data folder, as a person's second copy would,
# and never runs beside it. Its part reports and goes on when it fails, so the
# install's own update still runs.
copy_update() {
  local dir='C:\mcb\copy\mcbench-lite'
  start_window copy "$dir\\mcbench-lite.exe" || return 1
  shot 1c-copy-window.png
  step plain wait-download "$dir" || return 1
  step bench desktop quit || return 1
  start_window copy-next "$dir\\mcbench-lite.exe" || return 1
  step plain updated C:\\mcb\\expect-B.txt "$dir" "$VERSION_B" || return 1
  step plain display-version same || return 1
  step bench desktop quit
}

if [ "$DESKTOP" = 1 ]; then
# The first start, from the Start menu shortcut, with the feed offering A
# when there is a feed.
if [ "$UPDATE" = 1 ]; then step plain log-mark first; feed_mark; fi
start_window first || { step plain app-logs || true; fail "the app's first window did not appear; the screen is in no-window-first.png"; }
shot 1-first-window.png
# The setup screen's WSL sentence through UI Automation; the screenshot is the
# evidence when the window exposes no text, so the run goes on either way.
if [ "$MACHINE" = no-wsl ]; then step bench desktop wsl-screen || echo "vm-test: wsl-screen found no text; read 1-first-window.png"; fi
step plain refused C:\\mcb\\mcbench-lite-setup-x64.exe
# The check has run once latest.json was asked for; a download would follow
# at once.
if [ "$UPDATE" = 1 ]; then wait_feed; sleep 5; fi
step bench desktop quit
step plain app-logs
if [ "$UPDATE" = 1 ]; then
  echo "vm-test: with the feed offering A, the start downloads nothing"
  feed_got no
  step plain as-is C:\\mcb\\expect-A.txt installed "$VERSION_A"
  step plain log-none first
  # The two refusals while A is installed: each start refuses, A stays whole.
  for kind in value byte; do
    offer "$kind"
    step plain log-mark "$kind"
    feed_mark
    start_window "refused-$kind" || fail "the app's window did not appear with the feed offering $kind; the screen is in no-window-refused-$kind.png"
    step plain log-new "$kind" "$kind"
    step bench desktop quit
    if [ "$kind" = value ]; then feed_got no; else feed_got yes; fi
    step plain as-is C:\\mcb\\expect-A.txt installed "$VERSION_A"
  done
  offer B
  step plain copy-direct
  copy_update || { echo "vm-test: FAILED the copy marked direct did not update; its screens are in 1c-copy-window.png or no-window-copy*.png"; step plain app-logs || true; }
  step bench desktop quit if-running
  # The install: this start downloads B and leaves it waiting; the next one
  # swaps it in, with nobody asked.
  step plain log-mark b
  feed_mark
  start_window b-download || fail "the app's window did not appear with the feed offering B; the screen is in no-window-b-download.png"
  shot 1b-download-start.png
  step plain wait-download installed
  step bench desktop quit
  feed_got yes
  start_window b-swapped || fail "the start after B's download showed no window; the screen is in no-window-b-swapped.png"
  step plain updated C:\\mcb\\expect-B.txt installed "$VERSION_B"
  step plain display-version "$VERSION_B"
  step plain log-none b
  shot 2-after-update.png
  step bench desktop quit
  # MCBENCH_UI_LOG on the updated app: its renderer and fonts lines.
  step bench desktop ui-log
  step plain app-logs
fi
fi
step plain uninstall
step plain listing after
step plain compare before after | tee "$OUT/compare.txt"
echo "vm-test: done ($MACHINE); results in $OUT"
