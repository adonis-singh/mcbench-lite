#!/usr/bin/env bash
# Compares an extractor work folder with what today's separate steps made
# (the Node app's first start), file by file, byte for byte: the engine's
# three tables, the block table and every file of the viewer's bake folder.
# Prints one line per file (same, DIFFERENT or MISSING) and fails on any
# difference.
#
#   compare-today.sh <repository> <today's data folder> <work folder>
#
# <repository> is the checkout whose first start made the tables
# (core/minecraft/1.21.10/); <today's data folder> its MCBENCH_DATA_DIR.
# config/core.conf names its own folder (BlueMap's download folder) and is
# compared with that path replaced.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: compare-today.sh <repository> <today's data folder> <work folder>" >&2; exit 2; }
repo=$(cd "$1" && pwd)
today=$(cd "$2" && pwd)
work=$(cd "$3" && pwd)
status=0
same=0
line() { echo "$1 $2"; [ "$1" = same ] && same=$((same + 1)) || status=1; }
compare() {
  if [ ! -f "$2" ]; then line MISSING "$3"
  elif cmp -s "$1" "$2"; then line same "$3"
  else line DIFFERENT "$3"; fi
}
for name in blocks.txt loot.json datafix.txt; do
  compare "$repo/core/minecraft/1.21.10/$name" "$work/out/$name" "$name"
done
table=$(cd "$today/minecraft-live/paper" && ls block-table-*.tsv)
compare "$today/minecraft-live/paper/$table" "$work/out/$table" "$table"
bakes=$(cd "$today/minecraft-live/bluemap" && ls -d ????????????????)
[ "$(echo "$bakes" | wc -l)" -eq 1 ] || { echo "today's data folder holds more than one bake: $bakes" >&2; exit 1; }
bake=$today/minecraft-live/bluemap/$bakes
while IFS= read -r name; do
  if [ "$name" = config/core.conf ]; then
    if [ -f "$work/$name" ] && diff -q <(sed "s#$today/minecraft-live/bluemap/data#DOWNLOADS#" "$bake/$name") <(sed "s#$work/data#DOWNLOADS#" "$work/$name") > /dev/null; then
      line same "$name (its download folder replaced)"
    else
      line DIFFERENT "$name"
    fi
  else
    compare "$bake/$name" "$work/$name" "$name"
  fi
done < <(cd "$bake" && find . -type f | sed 's#^\./##' | LC_ALL=C sort)
echo "$same identical"
exit $status
