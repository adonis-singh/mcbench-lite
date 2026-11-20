#!/bin/bash
# The macOS packaging proof that runs on a Mac with no window: bundle.sh and
# archive.sh on stand-in executables, the archive served from 127.0.0.1,
# fetched with curl into a scratch HOME and unpacked into its Applications
# folder, then what that installed bundle carries and does.
#
#   native/packaging/test/macos/host-proof.sh <mcbench-lite> <mcbench-build-child> <scratch> <port>
#
# <mcbench-lite> and <mcbench-build-child> are the stand-ins (any build of the
# two programs of one architecture; they are copied, never written). The
# archive is mcbench-lite-macos-<arch>.tar.gz of their architecture; only an
# arm64 one can be started on an Apple silicon Mac. <scratch> is an
# empty or missing folder. <port> is the local port the archive is served on.
#
# It never runs open, Finder, hdiutil or the app's window: the only programs
# of the bundle it starts are `mcbench-lite headless contract`, which opens no
# window, from a bundle that carries no quarantine attribute. The copy with a
# quarantine attribute, as a browser download would carry it, is assessed
# with spctl only, because starting a quarantined program makes Gatekeeper
# show its dialog on the screen; its rejection is why the only macOS route is
# the curl | sh command, which leaves no quarantine attribute.
set -euo pipefail
die() { echo "host-proof.sh: $*" >&2; exit 2; }
[ $# -eq 4 ] || die "usage: host-proof.sh <mcbench-lite> <mcbench-build-child> <scratch> <port>"
lite=$1 child=$2 scratch=$3 port=$4
macos="$(cd "$(dirname "$0")/../../macos" && pwd)"
[ ! -e "$scratch" ] || [ -z "$(ls -A "$scratch")" ] || die "$scratch is not empty"
mkdir -p "$scratch"
scratch=$(cd "$scratch" && pwd)
home="$scratch/home"
# The account's real home is never used: the scratch home must be inside
# <scratch> and must not be the real home (COMMON rule of 2026-11-17).
real_home=$(dscl . -read "/Users/$(id -un)" NFSHomeDirectory | awk '{ print $2 }')
[ -n "$real_home" ] || die "dscl gave no home for $(id -un)"
[ "$home" != "$real_home" ] || die "the scratch home $home is the account's real home"
case "$home" in "$scratch"/*) ;; *) die "the scratch home $home is outside $scratch" ;; esac
step() { printf '\n== %s\n' "$*"; }
# Runs a command that may fail and prints its exit status after its output.
status() { set +e; "$@" 2>&1; local s=$?; set -e; echo "(exit status $s)"; }

step "stand-ins"
for f in "$lite" "$child"; do echo "$f: $(lipo -archs "$f"), $(stat -f %z "$f") bytes"; done
# The tree's version, as build/macos.sh reads it.
version=$(awk '/^\[workspace.package\]/ { p = 1; next } /^\[/ { p = 0 } p && $1 == "version" { gsub(/"/, "", $3); print $3 }' "$macos/../../Cargo.toml")
case "$(lipo -archs "$lite")" in arm64) asset=mcbench-lite-macos-aarch64.tar.gz ;; x86_64) asset=mcbench-lite-macos-x86_64.tar.gz ;; *) die "$lite is not one arm64 or x86_64 executable" ;; esac
time=1794873600 # 2026-11-17 00:00:00 UTC

step "bundle.sh $version"
"$macos/bundle.sh" "$lite" "$child" "$version" "$scratch/build"
app="$scratch/build/mcbench-lite.app"
(cd "$scratch/build" && find mcbench-lite.app | LC_ALL=C sort)
plutil -p "$app/Contents/Info.plist"
echo "install-source: $(cat "$app/Contents/Resources/install-source")"
printf 'version: '; od -c "$app/Contents/Resources/version" | head -1
[ "$(cat "$app/Contents/Resources/version")" = "$version" ] || die "Contents/Resources/version is not $version"
( "$macos/bundle.sh" "$lite" "$child" 0.1 "$scratch/badversion" || true ) 2>&1

step "archive.sh, twice: the same bytes"
"$macos/archive.sh" "$app" "$time" "$scratch/release"
"$macos/archive.sh" "$app" "$time" "$scratch/again"
shasum -a 256 "$scratch/release/$asset" "$scratch/again/$asset"
cmp "$scratch/release/$asset" "$scratch/again/$asset"
echo "identical"
tar -tvzf "$scratch/release/$asset"
echo "AppleDouble entries: $(tar -tzf "$scratch/release/$asset" | grep -c '/\._' || true)"
echo "header formats: $(gzip -dc "$scratch/release/$asset" | file -b -)"
(cd "$scratch/release" && shasum -a 256 "$asset" > SHA256SUMS && cat SHA256SUMS)

step "serve on 127.0.0.1:$port, fetch with curl into a scratch HOME"
python3 -I -m http.server "$port" --bind 127.0.0.1 --directory "$scratch/release" > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2> /dev/null || true' EXIT
for _ in $(seq 50); do curl -fsS -o /dev/null "http://127.0.0.1:$port/SHA256SUMS" 2> /dev/null && break; sleep 0.1; done
mkdir -p "$home/Downloads" "$home/Applications"
(
  export HOME="$home"
  cd "$HOME/Downloads"
  curl -fsSL -O "http://127.0.0.1:$port/SHA256SUMS"
  curl -fsSL -O "http://127.0.0.1:$port/$asset"
  shasum -a 256 -c SHA256SUMS
  echo "xattr of the fetched archive:"; xattr -l "$asset"
  if xattr -p com.apple.quarantine "$asset" > /dev/null 2>&1; then
    die "the fetched archive carries a quarantine attribute, so its programs are not started here"
  fi
  tar -xzf "$asset" -C "$HOME/Applications"
)
kill "$server"; wait "$server" 2> /dev/null || true; trap - EXIT
echo "server $server stopped"
installed="$home/Applications/mcbench-lite.app"

step "xattr -lr of the installed bundle"
xattr -lr "$installed"
echo "(end of xattr)"

step "codesign --verify --deep --strict --verbose=2"
codesign --verify --deep --strict --verbose=2 "$installed" 2>&1

step "codesign -dvv and the entitlements of the main executable"
codesign -dvv "$installed" 2>&1 | grep -E '^(Identifier|Format|CodeDirectory|Signature|Info.plist|Sealed Resources|Runtime|TeamIdentifier)'
codesign -d --entitlements - "$installed/Contents/MacOS/mcbench-lite" 2>&1
codesign -dvv "$installed/Contents/MacOS/mcbench-build-child" 2>&1 | grep -E '^(Identifier|CodeDirectory|Signature)'

step "spctl --assess --type execute -vv"
status spctl --assess --type execute -vv "$installed"

step "a file added after signing breaks the signature"
cp -R "$installed" "$scratch/tampered.app"
echo extra > "$scratch/tampered.app/Contents/Resources/extra"
status codesign --verify --deep --strict --verbose=2 "$scratch/tampered.app"
cp -R "$installed" "$scratch/rewritten.app"
printf 'edited\n' > "$scratch/rewritten.app/Contents/Resources/install-source"
status codesign --verify --deep --strict --verbose=2 "$scratch/rewritten.app"
cp -R "$installed" "$scratch/reversioned.app"
printf '9.9.9\n' > "$scratch/reversioned.app/Contents/Resources/version"
status codesign --verify --deep --strict --verbose=2 "$scratch/reversioned.app"

step "headless contract from the installed bundle, started from the shell"
HOME="$home" MCBENCH_DATA_DIR="$home/data" "$installed/Contents/MacOS/mcbench-lite" headless contract > "$scratch/contract-installed.json"
HOME="$home" MCBENCH_DATA_DIR="$home/data" "$lite" headless contract > "$scratch/contract-standin.json"
shasum -a 256 "$scratch/contract-installed.json" "$scratch/contract-standin.json"
cmp "$scratch/contract-installed.json" "$scratch/contract-standin.json"
echo "identical, $(stat -f %z "$scratch/contract-installed.json") bytes"

step "a copy with a quarantine attribute: spctl only"
mkdir -p "$scratch/quarantined"
cp -R "$installed" "$scratch/quarantined/"
xattr -r -w com.apple.quarantine "0083;00000000;Safari;" "$scratch/quarantined/mcbench-lite.app"
xattr -l "$scratch/quarantined/mcbench-lite.app"
status spctl --assess --type execute -vv "$scratch/quarantined/mcbench-lite.app"
status codesign --verify --deep --strict --verbose=2 "$scratch/quarantined/mcbench-lite.app"

