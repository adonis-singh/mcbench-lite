#!/usr/bin/env bash
# Runs McbenchExtract: every file today's first start makes by running
# Minecraft's code, in ONE JVM (README.md). Release tooling: it runs on the
# release machine, and nothing it writes is committed.
#
#   extract.sh <repository> <cache> <jdk home> <work folder>
#
# <cache> is the folder cache.js fills (README.md, The cache). The work
# folder must not exist; it gets the bake's files as today's bake folder
# holds them (render-layers.tsv, tints.tsv, web/...) and out/ with
# blocks.txt, loot.json, datafix.txt, the block table and night.json. Extra JVM options
# come from $JVM_OPTS.
set -euo pipefail
[ $# -eq 4 ] || { echo "usage: extract.sh <repository> <cache> <jdk home> <work folder>" >&2; exit 2; }
repo=$(cd "$1" && pwd)
cache=$(cd "$2" && pwd)
jdk=$(cd "$3" && pwd)
[ ! -e "$4" ] || { echo "extract.sh: $4 exists; the work folder must be new" >&2; exit 1; }
mkdir -p "$4"
work=$(cd "$4" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
# As server/assets.js MC_VERSION and server/minecraft-dimension.js BIOME;
# prep.js checks both against the repository.
version=1.21.10
biome=minecraft:plains
paper=$cache/paper
bluemap=$cache/bluemap/bluemap-5.22-cli.jar
client=$cache/client.jar
mappings=$cache/bluemap/minecraft-client-$version-mappings.txt
for file in "$bluemap" "$client" "$mappings"; do [ -f "$file" ] || { echo "extract.sh: the cache has no $file (run cache.js)" >&2; exit 1; }; done

classes=$work/.classes
mkdir -p "$classes/server" "$classes/bake" "$work/out"
jar_list() { unzip -p "$paper"/paper-"$version"-*.jar "META-INF/$1" | awk -F'\t' -v d="$paper/$2" '{print d "/" $3}'; }
{ jar_list versions.list versions; jar_list libraries.list libraries; } > "$classes/classpath.txt"
cp=$(paste -sd: "$classes/classpath.txt")
# As server/engine.js (SOURCES), minecraft-live.js and minecraft-tables.js
# (the block table, the data fixer) and bluemap.js (BAKE) compile them; the
# engine's and the bake's lists are read from those files (sources.js).
sources=$(node "$here/sources.js" "$repo")
IFS=$'\t' read -r -a engine <<< "$(echo "$sources" | sed -n 's/^engine\t//p')"
IFS=$'\t' read -r -a bake <<< "$(echo "$sources" | sed -n 's/^bake\t//p')"
"$jdk/bin/javac" --release 21 -proc:none -XDignore.symbol.file -nowarn -classpath "$cp" -d "$classes/server" "${engine[@]}" &
p1=$!
"$jdk/bin/javac" --release 25 -proc:none -nowarn -classpath "$cp" -d "$classes/server" "$repo/server/McbenchBlockTable.java" "$repo/scripts/DataFixReference.java" &
p2=$!
"$jdk/bin/javac" -proc:none -nowarn -classpath "$bluemap" -d "$classes/bake" "${bake[@]}" "$repo/server/McbenchNight.java" "$here/McbenchExtract.java" &
p3=$!
wait $p1
wait $p2
wait $p3
"$jdk/bin/jar" cf "$classes/bake.jar" -C "$classes/bake" .

node "$here/prep.js" "$repo" "$work" "$version" "$biome"
mkdir -p "$work/data"
# BlueMap finds the client jar in its download folder, as today's bake does.
ln -s "$client" "$work/data/minecraft-client-$version.jar"
# sha256sum on Linux, shasum on macOS.
sha256() { if command -v sha256sum > /dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi; }
format=$(sha256 "$repo/server/McbenchBlockTable.java" | cut -c1-8)
table=block-table-$(basename "$paper"/paper-"$version"-*.jar .jar)-$format.tsv
cd "$work"
# shellcheck disable=SC2086
"$jdk/bin/java" -Xmx2G ${JVM_OPTS:-} -cp "$bluemap:$classes/bake.jar" McbenchExtract "$work/out" "$classes/server" "$classes/classpath.txt" \
  "$work/data/minecraft-client-$version.jar" "$mappings" "$version" "$biome" "$table" > "$work/out/stdout.txt" 2> "$work/out/stderr.txt" \
  || { tail -40 "$work/out/stderr.txt" >&2; exit 1; }
grep '^\[extract\]' "$work/out/stderr.txt"
