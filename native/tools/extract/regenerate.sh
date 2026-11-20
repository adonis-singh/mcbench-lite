#!/usr/bin/env bash
# The regenerate-and-compare test (README.md): reruns the extractor where
# the jars are cached and requires byte equality with the tracked lock
# file, file by file and for the bundle.
#
#   regenerate.sh <repository> <cache> <jdk home> <scratch folder>
#
# <scratch folder> must not exist; it keeps the run (work/ and bundle/) for
# a look at what differs, and is deleted by whoever made it. Fails, naming
# each file that differs, unless everything is the lock's.
set -euo pipefail
[ $# -eq 4 ] || { echo "usage: regenerate.sh <repository> <cache> <jdk home> <scratch folder>" >&2; exit 2; }
repo=$(cd "$1" && pwd)
[ ! -e "$4" ] || { echo "regenerate.sh: $4 exists; the scratch folder must be new" >&2; exit 1; }
mkdir -p "$4"
scratch=$(cd "$4" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
"$here/extract.sh" "$repo" "$2" "$3" "$scratch/work" > "$scratch/extract.log"
"$here/bundle.sh" "$2" "$scratch/work" "$scratch/bundle" "$(sed -n 's/^url\t//p' "$repo/native/minecraft-data.lock")" > "$scratch/bundle.log"
native=$(cd "$here/../.." && pwd)
(cd "$native" && "${CARGO:-cargo}" run --quiet --release -p mcbench-setup --bin mcbench-data -- check "$scratch/bundle/minecraft-data-1.21.10.tar.zst" "$repo/native/minecraft-data.lock" "$scratch/bundle/files")
