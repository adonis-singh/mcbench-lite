#!/usr/bin/env bash
# The shipped data's bundle and lock file from one extractor run
# (README.md, The bundle). Release tooling: the bundle is a file of the
# release machine; publishing it is a release step the owner approves.
#
#   bundle.sh <cache> <work folder> <out folder> [<url>]
#
# <out folder> must not exist; it gets files/ (the shipped files by their
# bundle names), minecraft-data-1.21.10.tar.zst and minecraft-data.lock,
# which is copied to native/minecraft-data.lock when the bundle is released.
# <url> is where the build fetches the bundle from (the release asset's
# address); without it the lock names it not published.
# The mcbench-data tool is run with cargo from native/ ($CARGO, default
# cargo; $CARGO_TARGET_DIR is honoured).
set -euo pipefail
[ $# -eq 3 ] || [ $# -eq 4 ] || { echo "usage: bundle.sh <cache> <work folder> <out folder> [<url>]" >&2; exit 2; }
cache=$(cd "$1" && pwd)
work=$(cd "$2" && pwd)
[ ! -e "$3" ] || { echo "bundle.sh: $3 exists; the out folder must be new" >&2; exit 1; }
mkdir -p "$3"
out=$(cd "$3" && pwd)
url=${4:-not-published-yet}
here=$(cd "$(dirname "$0")" && pwd)
native=$(cd "$here/../.." && pwd)
tool() { (cd "$native" && "${CARGO:-cargo}" run --quiet --release -p mcbench-setup --bin mcbench-data -- "$@"); }
tool stage "$cache" "$work" "$out/files"
tool tar "$out/files" "$out/minecraft-data-1.21.10.tar"
# Level 19, one thread, the frame's checksum on: the same zstd makes the
# same bytes. The lock records its version.
zstd -q -19 -T1 --check "$out/minecraft-data-1.21.10.tar" -o "$out/minecraft-data-1.21.10.tar.zst"
rm "$out/minecraft-data-1.21.10.tar"
version=$(zstd -V | sed -n 's/.*v\([0-9][0-9.]*\).*/\1/p')
tool lock "$out/minecraft-data-1.21.10.tar.zst" "$out/files" "$version" "$url" > "$out/minecraft-data.lock"
tool check "$out/minecraft-data-1.21.10.tar.zst" "$out/minecraft-data.lock" "$out/files" > /dev/null
ls -l "$out/minecraft-data-1.21.10.tar.zst"
grep '^bundle' "$out/minecraft-data.lock"
