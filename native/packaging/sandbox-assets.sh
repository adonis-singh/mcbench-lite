#!/bin/bash
# native/packaging/sandbox-assets.sh: the bash sandbox's release assets
# (native/packaging/sandbox-assets.md).
#
#   sandbox-assets.sh build [--only guest-base|rootfs] <repository> <out> [<releases address>]
#   sandbox-assets.sh check <repository> <out> [--child <Linux build child>]
#   sandbox-assets.sh fetch <repository> <out>
#
# build runs the two recipes for both architectures and writes
#
#   <out>/release/rootfs-x86_64.tar.zst, rootfs-aarch64.tar.zst
#   <out>/release/guest-base-x86_64.tar.zst, guest-base-aarch64.tar.zst
#   <out>/release/passt-<version>.tar.gz     pasta's source, which the root filesystem holds
#   <out>/release/sandbox-assets.lock        the lock, to replace native/sandbox-assets.lock
#   <out>/release/SHA256SUMS                 over every other file in release/
#   <out>/lists/                             what each asset holds, for the source bundle
#   <out>/work/guest/, <out>/work/rootfs/    the recipes' own output folders
#
# and prints the release's name, sandbox-assets-<first 16 hex digits of the
# sha256 of the lock's lines above its url line>. The lock's url line is
# <releases address>/<that name> when the address is given (for GitHub,
# https://github.com/adonis-singh/mcbench-lite/releases/download), else
# not-published-yet. With --only, the other kind's lines come from the
# tracked native/sandbox-assets.lock (for rootfs there must be one), and
# release/ holds only the kind that was built.
#
# check rebuilds the guest bases into <out>/work/guest and requires each to
# be the tracked lock's, byte for byte; a root filesystem file given in
# <out>/release/ must have the lock's sha256 and size. With --child, the
# Linux build child the Windows payload ships (the release's, built against
# the Linux release's glibc) must find every shared object it names
# (readelf's NEEDED) in the x86_64 guest base just rebuilt, whose userland
# the WSL distro is imported from: a missing one fails the check, named.
# Needs readelf (binutils) for that step.
#
# fetch downloads every asset the tracked lock names from its url into <out>
# and checks each against the lock.
#
# The guest recipe builds the static agent with cargo: CARGO_HOME,
# RUSTUP_HOME and CARGO_TARGET_DIR are the caller's, rustup is on PATH with
# both musl targets added. The root filesystem recipe needs Docker with qemu
# for arm64; this script names every Docker object it makes mcbn-s5-*.
set -euo pipefail
USAGE="usage: sandbox-assets.sh build [--only guest-base|rootfs] <repository> <out> [<releases address>] | check <repository> <out> [--child <Linux build child>] | fetch <repository> <out>"
die() { echo "sandbox-assets.sh: $*" >&2; exit 1; }
sha() { if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -c1-64; else shasum -a 256 "$1" | cut -c1-64; fi; }
size() { wc -c < "$1" | tr -d ' '; }

COMMAND=${1:?$USAGE}
shift
ONLY=""
if [ "$COMMAND" = build ] && [ "${1:-}" = --only ]; then
  ONLY=${2:?$USAGE}
  case "$ONLY" in guest-base | rootfs) ;; *) die "--only takes guest-base or rootfs, not $ONLY" ;; esac
  shift 2
fi
REPO=$(cd "${1:?$USAGE}" && pwd)
OUT=$(mkdir -p "${2:?$USAGE}" && cd "$2" && pwd)
ADDRESS=${3:-}
CHILD=""
if [ "$COMMAND" = check ] && [ -n "${3:-}" ]; then
  [ "$3" = --child ] || die "$USAGE"
  [ -f "${4:?$USAGE}" ] || die "--child names no file: $4"
  CHILD=$(cd "$(dirname "$4")" && pwd)/$(basename "$4")
  ADDRESS=""
fi
LOCK="$REPO/native/sandbox-assets.lock"
RECIPES="$REPO/native/crates/sandbox"
ARCHES="x86_64 aarch64"
deb_arch() { case "$1" in x86_64) echo amd64 ;; aarch64) echo arm64 ;; esac; }

# The lock's lines of one kind (rootfs or guest-base), in the lock's order.
lock_lines() { awk -F'\t' -v kind="$1" '$1 == kind' "$LOCK"; }

# The package lists of one asset in release/, under lists/.
lists() {
  python3 -I - "$1" "$2" "$OUT/lists" << 'PYTHON'
import json, os, subprocess, sys, tarfile
kind_arch, asset, lists = sys.argv[1], sys.argv[2], sys.argv[3]
kind, arch = kind_arch.split(":")
zstd = subprocess.Popen(["zstd", "-dc", asset], stdout=subprocess.PIPE)
tar = tarfile.open(fileobj=zstd.stdout, mode="r|")
if kind == "guest-base":
    manifest = None
    for member in tar:
        if member.name == "manifest.json":
            manifest = json.load(tar.extractfile(member))
    if manifest is None:
        sys.exit(f"sandbox-assets.sh: {asset} holds no manifest.json")
    rows = []
    for package in manifest["packages"].values():
        # Debian's pool name: <package>_<version>_<architecture>.deb, an epoch's colon written %3a.
        name, version, deb_arch = package["file"].removesuffix(".deb").split("_")
        rows.append(f"{name}\t{version.replace('%3a', ':')}\t{deb_arch}\t{package['file']}\t{package['sha256']}\n")
    with open(os.path.join(lists, f"guest-base-{arch}.packages.tsv"), "w") as out:
        out.write(f"# The Debian packages in guest-base-{arch}.tar.zst, from its manifest.json: snapshot.debian.org {manifest['debian_snapshot']}\n")
        out.write("# package\tversion\tarchitecture\tfile\tsha256\n")
        out.writelines(sorted(rows))
else:
    status = None
    pip = []
    for member in tar:
        if member.name == "rootfs/var/lib/dpkg/status":
            status = tar.extractfile(member).read()
        elif member.isfile() and member.name.endswith(".dist-info/METADATA") and "/site-packages/" in member.name:
            fields = {}
            for line in tar.extractfile(member).read().decode().splitlines():
                if not line:
                    break
                key, _, value = line.partition(": ")
                fields.setdefault(key, value)
            pip.append(f"{fields['Name']}\t{fields['Version']}\t{os.path.dirname(member.name).removeprefix('rootfs/')}\n")
    if status is None:
        sys.exit(f"sandbox-assets.sh: {asset} holds no rootfs/var/lib/dpkg/status")
    with open(os.path.join(lists, f"rootfs-{arch}.dpkg-status"), "wb") as out:
        out.write(status)
    with open(os.path.join(lists, f"rootfs-{arch}.pip.tsv"), "w") as out:
        out.write(f"# The Python distributions pip installed in rootfs-{arch}.tar.zst (each site-packages *.dist-info)\n")
        out.write("# name\tversion\tfolder\n")
        out.writelines(sorted(pip))
if zstd.wait() != 0:
    sys.exit(f"sandbox-assets.sh: zstd could not read {asset}")
PYTHON
}

build_guest_bases() {
  for arch in $ARCHES; do
    echo "[assets] guest base $arch" >&2
    bash "$RECIPES/guest/build.sh" "$OUT/work/guest" "$arch" >&2
  done
}

build() {
  [ ! -e "$OUT/release" ] || die "$OUT/release already exists; give a new <out>"
  if [ "$ONLY" = rootfs ]; then
    [ -f "$LOCK" ] || die "--only rootfs takes the guest bases' lines from native/sandbox-assets.lock, and there is none"
    [ -n "$(lock_lines guest-base)" ] || die "native/sandbox-assets.lock has no guest-base lines for --only rootfs to keep"
  fi
  mkdir -p "$OUT/release" "$OUT/lists" "$OUT/work"
  # The guest bases first (minutes, no Docker), then the root filesystems
  # (the arm64 one under qemu takes long); the lock lists the root
  # filesystems first.
  local rootfs_lines="$OUT/work/rootfs-lines" guest_lines="$OUT/work/guest-base-lines"
  if [ "$ONLY" = rootfs ]; then
    lock_lines guest-base > "$guest_lines"
  else
    build_guest_bases
    : > "$guest_lines"
    for arch in $ARCHES; do
      local file="guest-base-$arch.tar.zst"
      cp "$OUT/work/guest/mcbench-sandbox-guest-$arch.tar.zst" "$OUT/release/$file"
      printf 'guest-base\t%s\t%s\t%s\t%s\n' "$arch" "$file" "$(sha "$OUT/release/$file")" "$(size "$OUT/release/$file")" >> "$guest_lines"
      lists "guest-base:$arch" "$OUT/release/$file"
    done
  fi

  if [ "$ONLY" = guest-base ]; then
    if [ -f "$LOCK" ]; then lock_lines rootfs > "$rootfs_lines"; else : > "$rootfs_lines"; fi
  else
    for arch in $ARCHES; do
      echo "[assets] root filesystem $arch" >&2
      MCBENCH_DOCKER_PREFIX=mcbn-s5- bash "$RECIPES/rootfs/build.sh" "$OUT/work/rootfs" "$(deb_arch "$arch")" >&2
      cp "$OUT/work/rootfs/mcbench-sandbox-rootfs-$(deb_arch "$arch").tar.zst" "$OUT/release/rootfs-$arch.tar.zst"
    done
    local passt=("$OUT"/work/rootfs/passt-*.tar.gz)
    [ "${#passt[@]}" = 1 ] || die "$OUT/work/rootfs holds ${#passt[@]} passt tarballs, not one"
    cp "${passt[0]}" "$OUT/release/"
    : > "$rootfs_lines"
    for arch in $ARCHES; do
      local file="rootfs-$arch.tar.zst"
      printf 'rootfs\t%s\t%s\t%s\t%s\n' "$arch" "$file" "$(sha "$OUT/release/$file")" "$(size "$OUT/release/$file")" >> "$rootfs_lines"
      lists "rootfs:$arch" "$OUT/release/$file"
    done
  fi

  local body="$OUT/work/lock-body"
  {
    echo "# The bash sandbox's release assets (native/packaging/sandbox-assets.md), written by"
    echo "# native/packaging/sandbox-assets.sh build and never edited by hand. Each line: kind,"
    echo "# architecture, file name, sha256, size in bytes; url is the folder that holds the files."
    cat "$rootfs_lines" "$guest_lines"
  } > "$body"
  local name="sandbox-assets-$(sha "$body" | cut -c1-16)"
  local url=not-published-yet
  [ -z "$ADDRESS" ] || url="${ADDRESS%/}/$name"
  { cat "$body"; printf 'url\t%s\n' "$url"; } > "$OUT/release/sandbox-assets.lock"

  (cd "$OUT/release" && for file in *; do [ "$file" = SHA256SUMS ] || echo "$(sha "$file")  $file"; done) > "$OUT/work/SHA256SUMS"
  mv "$OUT/work/SHA256SUMS" "$OUT/release/SHA256SUMS"
  cat "$OUT/release/sandbox-assets.lock"
  echo "release $name"
}

check() {
  [ -f "$LOCK" ] || die "there is no native/sandbox-assets.lock to check against"
  [ -n "$(lock_lines guest-base)" ] || die "native/sandbox-assets.lock has no guest-base lines"
  build_guest_bases
  local differ=0 kind arch file want_sha want_size path got_sha got_size
  while IFS=$'\t' read -r kind arch file want_sha want_size; do
    case "$kind" in
      guest-base) path="$OUT/work/guest/mcbench-sandbox-guest-$arch.tar.zst" ;;
      rootfs)
        path="$OUT/release/$file"
        if [ ! -f "$path" ]; then echo "$file: not given in $OUT/release, not checked"; continue; fi
        ;;
      *) continue ;;
    esac
    got_sha=$(sha "$path")
    got_size=$(size "$path")
    if [ "$got_sha" = "$want_sha" ] && [ "$got_size" = "$want_size" ]; then
      echo "$file: the lock's ($got_sha, $got_size bytes)"
    else
      echo "$file: DIFFERS: the lock has $want_sha, $want_size bytes; $path is $got_sha, $got_size bytes"
      differ=1
    fi
  done < <(grep -v '^#' "$LOCK")
  [ "$differ" = 0 ] || die "the assets above that differ are not the tracked lock's; native/packaging/sandbox-assets.md says when the lock is regenerated"
  [ -z "$CHILD" ] || child_needs
}

# --child: every shared object the build child names is in the x86_64 guest
# base's userland (its library folder, or the loader's own path), read from
# the userland's file list (extracting it needs root for its device nodes).
child_needs() {
  command -v readelf > /dev/null || die "--child needs readelf (binutils)"
  local listing missing="" name
  listing=$(tar -tf "$OUT/work/guest/mcbench-sandbox-guest-x86_64/userland.tar" | sed 's#^\./##')
  for name in $(readelf -d "$CHILD" | sed -n 's/.*(NEEDED).*\[\(.*\)\].*/\1/p'); do
    if printf '%s\n' "$listing" | grep -qx -e "usr/lib/x86_64-linux-gnu/$name" -e "usr/lib64/$name"; then
      echo "$name: in the x86_64 guest base"
    else
      echo "$name: MISSING from the x86_64 guest base"
      missing="$missing $name"
    fi
  done
  [ -z "$missing" ] || die "the build child $CHILD names shared objects the x86_64 guest base lacks:$missing (the WSL distro could not load it)"
}

fetch() {
  [ -f "$LOCK" ] || die "there is no native/sandbox-assets.lock to fetch from"
  local url
  url=$(awk -F'\t' '$1 == "url" { print $2 }' "$LOCK")
  [ "$url" != not-published-yet ] || die "native/sandbox-assets.lock's url is not-published-yet: the sandbox assets have not been published."
  local kind arch file want_sha want_size
  while IFS=$'\t' read -r kind arch file want_sha want_size; do
    case "$kind" in rootfs | guest-base) ;; *) continue ;; esac
    echo "[assets] $url/$file" >&2
    curl -sSfL -o "$OUT/$file.part" "$url/$file"
    [ "$(sha "$OUT/$file.part")" = "$want_sha" ] && [ "$(size "$OUT/$file.part")" = "$want_size" ] ||
      die "$url/$file is not the lock's (sha256 $want_sha, $want_size bytes); it is left as $OUT/$file.part"
    mv "$OUT/$file.part" "$OUT/$file"
    echo "$file $want_sha $want_size"
  done < <(grep -v '^#' "$LOCK")
}

case "$COMMAND" in
  build) build ;;
  check) check ;;
  fetch) fetch ;;
  *) die "$USAGE" ;;
esac
