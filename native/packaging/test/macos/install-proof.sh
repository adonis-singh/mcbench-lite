#!/bin/bash
# The install of a staging release on this Mac with no window: install.sh
# --no-open from the staging site into a scratch HOME, what was installed,
# `headless contract` from the installed bundle, and the updater's start-time
# check on a copy of the install whose version file names another version.
#
#   native/packaging/test/macos/install-proof.sh <staging> <other version> <scratch> <release port> <site port>
#
# <staging> is server.py's folder (releases/LATEST, releases/download/<tag>/,
# site/), made by release.sh runs whose --base and --site are
# http://127.0.0.1:<release port> and http://127.0.0.1:<site port>.
# <other version> is written into the copy's Contents/Resources/version.
# <scratch> is an empty or missing folder.
#
# It never runs open or the app's window. The copy's signature is broken by
# the edit, as any edit of a signed bundle breaks it; the copy is only
# started from the shell, which the seal does not gate.
set -euo pipefail
die() { echo "install-proof.sh: $*" >&2; exit 2; }
[ $# -eq 5 ] || die "usage: install-proof.sh <staging> <other version> <scratch> <release port> <site port>"
staging=$1 other=$2 scratch=$3 release_port=$4 site_port=$5
here=$(cd "$(dirname "$0")" && pwd)
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

step "server.py on 127.0.0.1:$release_port and :$site_port, latest $(cat "$staging/releases/LATEST")"
python3 -I "$here/server.py" "$staging" "$release_port" "$site_port" 2> "$scratch/server.log" &
server=$!
trap 'kill "$server" 2> /dev/null || true' EXIT
for _ in $(seq 50); do curl -fsS -o /dev/null "http://127.0.0.1:$site_port/install.sh" 2> /dev/null && break; sleep 0.1; done
mkdir -p "$home"

step "curl -fsSL http://127.0.0.1:$site_port/install.sh | sh -s -- --no-open"
(cd "$home" && curl -fsSL "http://127.0.0.1:$site_port/install.sh" | HOME="$home" sh -s -- --no-open)
app="$home/Applications/mcbench-lite.app"
echo "install-source: $(cat "$app/Contents/Resources/install-source")"
echo "version: $(cat "$app/Contents/Resources/version")"
echo "quarantine attributes: $(xattr -lr "$app" | grep -c com.apple.quarantine || true)"
codesign --verify --deep --strict "$app" && echo "codesign --verify --deep --strict: valid"
grep -E '"(GET|HEAD)' "$scratch/server.log" | sed 's/^.*\] //'

step "headless contract from the installed bundle"
HOME="$home" MCBENCH_DATA_DIR="$home/data" "$app/Contents/MacOS/mcbench-lite" headless contract | shasum -a 256
echo "beside the payload: $(cd "$home/Applications" && ls -A | tr '\n' ' ')"

step "a copy whose version file says $other: headless contract"
mkdir -p "$scratch/copy/Applications"
cp -R "$app" "$scratch/copy/Applications/"
copy="$scratch/copy/Applications/mcbench-lite.app"
printf '%s\n' "$other" > "$copy/Contents/Resources/version"
set +e
HOME="$home" MCBENCH_DATA_DIR="$home/data" "$copy/Contents/MacOS/mcbench-lite" headless contract > "$scratch/copy-stdout.txt" 2> "$scratch/copy-stderr.txt"
s=$?
set -e
echo "stderr: $(cat "$scratch/copy-stderr.txt")"
echo "stdout: $(wc -c < "$scratch/copy-stdout.txt" | tr -d ' ') bytes"
echo "exit status $s"
[ "$s" = 2 ] || die "the copy's headless contract exited with $s, not 2"

kill "$server"; wait "$server" 2> /dev/null || true; trap - EXIT
echo "server $server stopped"
