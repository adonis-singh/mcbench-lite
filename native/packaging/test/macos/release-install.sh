#!/bin/bash
# The macOS install test of an assembled release: the page's command,
# install.sh, against the release folder served from 127.0.0.1, into a
# scratch home folder, then its --uninstall.
#
#   native/packaging/test/macos/release-install.sh <release folder> <scratch> <port>
#
# <release folder> is release.sh's <out>/release (it must hold this Mac's
# mcbench-lite-macos-<arch>.tar.gz and SHA256SUMS). <scratch> is a missing or
# empty folder; the home folder and the temporary folder install.sh sees are
# inside it, so nothing outside it is installed. <port> is the local port the
# release is served on (3190 to 3199 on the owner's Mac).
#
# It checks that the installed bundle's signature verifies, that its
# install-source is "script", that it carries no quarantine attribute, and
# that after --uninstall the scratch home folder's listing is the one before
# the install. It never opens the app (install.sh --no-open) and shows
# nothing on the screen. The server is stopped by its pid on every exit.
#
# Two things of install.sh reach outside the scratch home folder:
# --uninstall runs `lsregister -u` on the scratch bundle (Launch Services'
# own database), and removes the shader cache of the bundle identifier in
# `getconf DARWIN_USER_CACHE_DIR`, which HOME does not move, so on a Mac
# where the app is installed for real that cache goes too.
set -euo pipefail
die() { echo "release-install.sh: $*" >&2; exit 1; }
[ $# -eq 3 ] || { echo "usage: native/packaging/test/macos/release-install.sh <release folder> <scratch> <port>" >&2; exit 2; }
release=$1 scratch=$2 port=$3
[ "$(uname -s)" = Darwin ] || die "this test runs on macOS"
[ -f "$release/SHA256SUMS" ] || die "$release holds no SHA256SUMS: give release.sh's <out>/release"
[ ! -e "$scratch" ] || [ -z "$(ls -A "$scratch")" ] || die "$scratch is not empty"
install_sh="$(cd "$(dirname "$0")/../../../site" && pwd)/install.sh"
mkdir -p "$scratch/home" "$scratch/tmp"
scratch=$(cd "$scratch" && pwd)
release=$(cd "$release" && pwd)
home=$scratch/home
app=$home/Applications/mcbench-lite.app

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$release" > "$scratch/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2> /dev/null; wait "$server" 2> /dev/null || true' EXIT
for _ in $(seq 50); do
  curl -fsS -o /dev/null "http://127.0.0.1:$port/SHA256SUMS" 2> /dev/null && break
  sleep 0.1
done
curl -fsS -o /dev/null "http://127.0.0.1:$port/SHA256SUMS" || die "the server on 127.0.0.1:$port did not answer; its log is $scratch/server.log"

listing() { (cd "$home" && find . | LC_ALL=C sort) > "$scratch/$1.list"; }
run() { env HOME="$home" TMPDIR="$scratch/tmp" MCBENCH_LITE_RELEASES="http://127.0.0.1:$port" sh "$install_sh" "$@"; }

listing before
echo "== install"
start=$(date +%s)
run --no-open
echo "installed in $(($(date +%s) - start)) s"
codesign --verify --deep --strict "$app" || die "the installed bundle's signature does not verify"
echo "signature: verifies"
[ "$(cat "$app/Contents/Resources/install-source")" = script ] || die "install-source is $(cat "$app/Contents/Resources/install-source"), not script"
echo "install-source: script"
! xattr -r -p com.apple.quarantine "$app" > /dev/null 2>&1 || die "the installed bundle carries a quarantine attribute"
echo "quarantine: none"
listing installed

echo "== uninstall"
run --uninstall
listing after
if ! diff "$scratch/before.list" "$scratch/after.list" > "$scratch/left.diff"; then
  cat "$scratch/left.diff"
  die "the home folder differs after --uninstall from before the install (above)"
fi
echo "home folder after --uninstall: as before the install ($(wc -l < "$scratch/installed.list" | tr -d ' ') paths while installed)"
