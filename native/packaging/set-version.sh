#!/bin/bash
# native/packaging/set-version.sh: sets the version of the app.
#
#   native/packaging/set-version.sh <MAJOR.MINOR.PATCH>
#
# Writes the version into native/Cargo.toml's [workspace.package] (every
# crate under native/crates/ takes it with version.workspace = true) and
# into native/build-child/Cargo.toml's [package], and brings the entries of
# the workspace's own crates in native/Cargo.lock,
# native/build-child/Cargo.lock and native/draw/Cargo.lock (the drawing
# crate is a workspace of its own, keeps its own version, and depends on
# mcbench-render by path) to it: an entry with no source line whose
# name is a crate of native/crates/ or mcbench-build-child. No other line of
# either lock changes (mcbench-core, the engine under core/, keeps its own
# version), and nothing reaches the network, so `cargo metadata --locked
# --offline` passes afterwards in both folders.
#
# A release is cut from a commit whose version equals its tag. A local
# update test runs this on a scratch copy of the tree to make a second
# version.
set -euo pipefail
die() { echo "set-version.sh: $*" >&2; exit 1; }
[ $# -eq 1 ] || { echo "usage: native/packaging/set-version.sh <MAJOR.MINOR.PATCH>" >&2; exit 2; }
version=$1
[[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "the version $version is not MAJOR.MINOR.PATCH"
native=$(cd "$(dirname "$0")/.." && pwd)

# The workspace's own crates: every native/crates/*/Cargo.toml that takes
# the workspace's version, and the build child.
names=$(for manifest in "$native"/crates/*/Cargo.toml; do
  awk '/^\[package\]/ { p = 1; next } /^\[/ { p = 0 }
       p && /^name = / { gsub(/"/, "", $3); name = $3 }
       p && /^version\.workspace = true/ { own = 1 }
       END { if (own) print name; else { print FILENAME " does not take version.workspace = true" > "/dev/stderr"; exit 1 } }' "$manifest"
done | tr '\n' ' ')
names="$names mcbench-build-child"

# set_key <file> <section> <version>: the version line of one section.
set_key() {
  local file=$1 section=$2 tmp
  tmp=$(mktemp "$file.XXXXXX")
  awk -v section="$section" -v version="$version" '
    $0 == section { p = 1; print; next }
    /^\[/ { p = 0 }
    p && /^version = "/ { print "version = \"" version "\""; done++; next }
    { print }
    END { if (done != 1) exit 1 }' "$file" > "$tmp" || { /bin/rm -f "$tmp"; die "$file has no version line in $section"; }
  cat "$tmp" > "$file"
  /bin/rm -f "$tmp"
}

# set_lock <Cargo.lock>: the version of each entry named in $names that has
# no source line (a path crate). Reads one [[package]] block at a time.
set_lock() {
  local lock=$1 tmp
  tmp=$(mktemp "$lock.XXXXXX")
  awk -v names="$names" -v version="$version" '
    BEGIN { n = split(names, list, " "); for (i = 1; i <= n; i++) own[list[i]] = 1 }
    function flush(   i) {
      for (i = 1; i <= len; i++) {
        if (ours && !sourced && block[i] ~ /^version = "/) block[i] = "version = \"" version "\""
        print block[i]
      }
      if (ours && !sourced) changed++
      len = 0; ours = 0; sourced = 0
    }
    /^\[\[package\]\]$/ { flush() }
    { block[++len] = $0 }
    /^name = "/ { name = $3; gsub(/"/, "", name); ours = (name in own) }
    /^source = "/ { sourced = 1 }
    END { flush(); if (!changed) exit 1 }' "$lock" > "$tmp" || { /bin/rm -f "$tmp"; die "$lock holds no crate of the workspace"; }
  cat "$tmp" > "$lock"
  /bin/rm -f "$tmp"
}

set_key "$native/Cargo.toml" "[workspace.package]"
set_key "$native/build-child/Cargo.toml" "[package]"
set_lock "$native/Cargo.lock"
set_lock "$native/build-child/Cargo.lock"
set_lock "$native/draw/Cargo.lock"
echo "set-version.sh: $version in native/Cargo.toml, native/build-child/Cargo.toml and the three Cargo.lock files"
