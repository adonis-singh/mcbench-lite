#!/bin/bash
# Writes mcbench-lite-macos-<arch>.tar.gz, the archive install.sh and the
# updater fetch, from a signed mcbench-lite.app (bundle.sh). <arch> is
# aarch64 or x86_64, read from the bundle's main executable (lipo -archs).
#
#   native/packaging/macos/archive.sh <mcbench-lite.app> <time> <out>
#
# <time> is the seconds since 1970 every entry carries (build/macos.sh passes
# the release commit's time, so a new release's bundle has a newer date than
# the one it replaces, which Launch Services reads to notice the change).
#
# The archive is deterministic: bsdtar reads its entries from an mtree list
# in sorted order (LC_ALL=C), each with a fixed mode (0755 for folders and
# executables, 0644 for the rest), owner 0:0 with no names, and <time>; the
# ustar format has no extended headers, --no-xattrs and --no-mac-metadata keep
# extended attributes and AppleDouble ._ entries out, and COPYFILE_DISABLE=1
# is set as well; gzip -n writes no name or time. The bundle is at the root.
# mcbench-feed pack (the updater crate) replaces this script once it exists.
set -euo pipefail
die() { echo "archive.sh: $*" >&2; exit 2; }
[ $# -eq 3 ] || die "usage: archive.sh <mcbench-lite.app> <time> <out>"
app=$1 time=$2 out=$3
[ "$(basename "$app")" = mcbench-lite.app ] || die "$app is not named mcbench-lite.app"
[[ $time =~ ^[0-9]+$ ]] || die "the time $time is not a number of seconds"
codesign --verify --deep --strict "$app"
case "$(lipo -archs "$app/Contents/MacOS/mcbench-lite")" in
  arm64) asset=mcbench-lite-macos-aarch64 ;;
  x86_64) asset=mcbench-lite-macos-x86_64 ;;
  *) die "$app/Contents/MacOS/mcbench-lite is not one arm64 or x86_64 executable" ;;
esac
mkdir -p "$out"
out=$(cd "$out" && pwd)
export COPYFILE_DISABLE=1 LC_ALL=C

cd "$(dirname "$app")"
{
  echo '#mtree'
  find ./mcbench-lite.app | sort | while IFS= read -r path; do
    case "$path" in *[!A-Za-z0-9._/+-]*) die "$path has a character the mtree list would need to escape" ;; esac
    if [ -L "$path" ]; then die "$path is a symbolic link"
    elif [ -d "$path" ]; then echo "$path type=dir mode=0755 uid=0 gid=0 time=$time"
    elif [ -x "$path" ]; then echo "$path type=file mode=0755 uid=0 gid=0 time=$time contents=$path"
    else echo "$path type=file mode=0644 uid=0 gid=0 time=$time contents=$path"
    fi
  done
} > "$out/$asset.mtree"
tar -cf - --format ustar -s ',^\./,,' --no-xattrs --no-mac-metadata --uname '' --gname '' "@$out/$asset.mtree" \
  | gzip -n -9 > "$out/$asset.tar.gz"
/bin/rm -f "$out/$asset.mtree"
