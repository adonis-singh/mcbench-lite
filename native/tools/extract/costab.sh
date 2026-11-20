#!/usr/bin/env bash
# The COS_TAB check (McbenchCosTab.java, README.md): Minecraft's own
# Mth.atan2 with the shipped table and with an arm64 JVM's, over a fixed
# sample of minecart movements; fails if any float yaw differs.
#
#   costab.sh <cache> <jdk home> <blocks.txt>
#
# Release tooling, run beside regenerate.sh. Extra JVM options come from
# $JVM_OPTS.
set -euo pipefail
[ $# -eq 3 ] || { echo "usage: costab.sh <cache> <jdk home> <blocks.txt>" >&2; exit 2; }
cache=$(cd "$1" && pwd)
jdk=$(cd "$2" && pwd)
table=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
paper=$cache/paper
jar_list() { unzip -p "$paper"/paper-1.21.10-*.jar "META-INF/$1" | awk -F'\t' -v d="$paper/$2" '{print d "/" $3}'; }
cp=$( { jar_list versions.list versions; jar_list libraries.list libraries; } | paste -sd:)
classes=$(mktemp -d)
trap 'rm -rf "$classes"' EXIT
"$jdk/bin/javac" -nowarn -cp "$cp" -d "$classes" "$here/McbenchCosTab.java"
# shellcheck disable=SC2086
"$jdk/bin/java" ${JVM_OPTS:-} -cp "$cp:$classes" McbenchCosTab "$table"
