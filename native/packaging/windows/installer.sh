#!/usr/bin/env bash
# installer.sh <payload folder> <version> <out folder>
#
# Builds <out folder>/mcbench-lite-setup-x64.exe, the per-user NSIS
# installer (installer.nsi beside this script), from a payload folder that
# native/packaging/build/windows.ps1 made. The installer packs every file of
# the payload folder: this script lists them into payload.nsh, the one list
# the installer, its running check and its uninstaller read.
#
#   <version>   MAJOR.MINOR.PATCH
#
# Where makensis runs:
#   Linux (adipc)    NSIS 3.11-1 from Debian 13's snapshot of 2026-11-04 in a
#                    container of debian:13 pinned by digest, --rm --cpus 1,
#                    named mcbn-s5-win-nsis; it needs Docker and the network
#                    (apt-get fetches nsis each run, about 2 MB).
#   Windows (a GitHub runner, the VM; Git for Windows' bash)
#                    makensis from NSIS 3 on the PATH. From PowerShell:
#                    bash native/packaging/windows/installer.sh <payload> <version> <out>
set -euo pipefail

fail() { echo "installer.sh: $*" >&2; exit 1; }

[ "$#" = 3 ] || fail "usage: installer.sh <payload folder> <version> <out folder>"
payload="$1" version="$2" out="$3"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "the version $version is not MAJOR.MINOR.PATCH."
[ -d "$payload" ] || fail "the payload folder $payload does not exist."
[ -f "$payload/mcbench-lite.exe" ] || fail "the payload folder $payload has no mcbench-lite.exe."
[ -f "$payload/version" ] || fail "the payload folder $payload has no version file."
[ "$(cat "$payload/version"; echo x)" = "$version
x" ] || fail "the payload's version file holds '$(head -c 40 "$payload/version")', not $version and a newline."
mkdir -p "$out"

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
payload="$(cd "$payload" && pwd)"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# payload.nsh: PAYLOAD_FILE_LIST runs a macro on each payload file's name.
size=0
{
  echo '; Written by installer.sh from the payload folder; not tracked.'
  echo '!macro PAYLOAD_FILE_LIST MACRO'
  for path in "$payload"/* "$payload"/.[!.]*; do
    [ -e "$path" ] || continue
    name="${path##*/}"
    [ -f "$path" ] && [ ! -L "$path" ] || fail "the payload folder holds $name, which is not a plain file; the installer packs plain files only."
    [[ "$name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] || fail "the payload file name $name has characters other than letters, digits, '.', '_' and '-'."
    case "$name" in install-source | uninstall.exe) fail "the payload holds $name, which the installer writes itself." ;; esac
    echo "  !insertmacro \${MACRO} \"$name\""
    size=$((size + $(wc -c <"$path")))
  done
  echo '!macroend'
} >"$work/payload.nsh"
size_kb=$(((size + 1023) / 1024))
exe="mcbench-lite-setup-x64.exe"

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    command -v makensis >/dev/null || fail "makensis is not on the PATH; install NSIS 3."
    makensis -V3 -INPUTCHARSET UTF8 -DVERSION="$version" \
      -DPAYLOAD="$(cygpath -w "$payload")" -DPAYLOAD_NSH="$(cygpath -w "$work/payload.nsh")" \
      -DSIZE_KB="$size_kb" -DOUTFILE="$(cygpath -w "$out/$exe")" "$(cygpath -w "$here/installer.nsi")"
    ;;
  Linux)
    image="debian@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6"
    snapshot="http://snapshot.debian.org/archive/debian/20261104T212339Z"
    docker run --rm --cpus 1 --name mcbn-s5-win-nsis \
      -v "$here/..:/packaging:ro" -v "$payload:/payload:ro" -v "$work:/work:ro" -v "$out:/out" \
      "$image" sh -euc "
        rm -f /etc/apt/sources.list.d/debian.sources
        echo 'deb [check-valid-until=no] $snapshot trixie main' >/etc/apt/sources.list
        apt-get -qq update >/dev/null
        DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends nsis=3.11-1 >/dev/null
        makensis -V3 -INPUTCHARSET UTF8 -DVERSION=$version -DPAYLOAD=/payload \
          -DPAYLOAD_NSH=/work/payload.nsh -DSIZE_KB=$size_kb -DOUTFILE=/out/$exe /packaging/windows/installer.nsi
        chown $(id -u):$(id -g) /out/$exe"
    ;;
  *) fail "makensis runs here on Linux (in a container) or on Windows, and this is $(uname -s)." ;;
esac

[ -f "$out/$exe" ] || fail "makensis wrote no $out/$exe."
echo "installer.sh: $out/$exe, $(wc -c <"$out/$exe") bytes, sha256 $(sha256sum "$out/$exe" | cut -d' ' -f1); payload $size bytes"
