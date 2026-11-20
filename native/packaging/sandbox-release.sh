#!/bin/bash
# native/packaging/sandbox-release.sh: assembles the folder of the bash
# sandbox's own release, sandbox-assets-<hash> (native/packaging/README.md,
# native/packaging/sandbox-assets.md). Nothing is uploaded.
#
#   native/packaging/sandbox-release.sh <assets> <sources> <out>
#
# <assets> is the <out> of `sandbox-assets.sh build`: release/ with the four
# assets, pasta's source passt-<version>.tar.gz and sandbox-assets.lock, and
# lists/. After a --only build, the other kind's two files are fetched into
# <assets>/release/ first (sandbox-assets.md, The release and its name).
# <sources> is the <OUT> of `sources.sh`. <out> must not exist. It becomes
#
#   <out>/rootfs-x86_64.tar.zst, rootfs-aarch64.tar.zst,
#         guest-base-x86_64.tar.zst, guest-base-aarch64.tar.zst
#                               each with the lock's sha256 and size
#   <out>/passt-<version>.tar.gz
#   <out>/sandbox-assets.lock
#   <out>/sandbox-sources.tar   <sources> as one folder sandbox-sources/: an
#                               uncompressed GNU tar, entries sorted by name,
#                               every time 0, owner and group 0 with no names,
#                               so the same folder gives the same bytes
#   <out>/SHA256SUMS            over every other file
#
# It first runs `sources.sh --check` on <sources> against <assets>/lists and
# the lock, so the tar holds a complete bundle. Needs GNU tar and python3.
set -euo pipefail
die() { echo "sandbox-release.sh: $*" >&2; exit 1; }
[ $# -eq 3 ] || { echo "usage: native/packaging/sandbox-release.sh <assets> <sources> <out>" >&2; exit 2; }
assets=$1 sources=$2 out=$3
packaging=$(cd "$(dirname "$0")" && pwd)
[ -f "$assets/release/sandbox-assets.lock" ] || die "$assets/release has no sandbox-assets.lock: give the <out> of sandbox-assets.sh build"
[ -d "$assets/lists" ] || die "$assets has no lists/: give the <out> of sandbox-assets.sh build"
[ -d "$sources" ] || die "the sources folder $sources does not exist"
[ ! -e "$out" ] || die "$out exists; the release is assembled into a new folder"
tar --version 2> /dev/null | grep -q 'GNU tar' || die "tar is not GNU tar, which the deterministic sandbox-sources.tar needs"
sha() { sha256sum "$1" | cut -c1-64; }
lock=$assets/release/sandbox-assets.lock

echo "[sandbox-release] the source bundle's completeness check"
"$packaging/sources.sh" --check "$assets/lists" "$lock" "$sources"

missing=""
while IFS=$'\t' read -r kind arch file want_sha want_size; do
  case "$kind" in rootfs | guest-base) ;; *) continue ;; esac
  path=$assets/release/$file
  if [ ! -f "$path" ]; then missing="$missing $file"; continue; fi
  [ "$(sha "$path")" = "$want_sha" ] && [ "$(wc -c < "$path" | tr -d ' ')" = "$want_size" ] ||
    die "$path is not the lock's $arch $kind (sha256 $want_sha, $want_size bytes)"
done < <(grep -v '^#' "$lock")
[ -z "$missing" ] || die "$assets/release lacks$missing; fetch them from the previous release with sandbox-assets.sh fetch"
passt=("$assets"/release/passt-*.tar.gz)
[ "${#passt[@]}" = 1 ] && [ -f "${passt[0]}" ] || die "$assets/release holds ${#passt[@]} passt-<version>.tar.gz files, not one"

mkdir -p "$out"
out=$(cd "$out" && pwd)
while IFS=$'\t' read -r kind _ file _; do
  case "$kind" in rootfs | guest-base) cp "$assets/release/$file" "$out/" ;; esac
done < <(grep -v '^#' "$lock")
cp "${passt[0]}" "$lock" "$out/"
echo "[sandbox-release] sandbox-sources.tar"
tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu \
  --transform 's,^\.,sandbox-sources,' -cf "$out/sandbox-sources.tar" -C "$sources" .
(cd "$out" && for file in *; do echo "$(sha "$file")  $file"; done) > "$out/.SHA256SUMS"
mv "$out/.SHA256SUMS" "$out/SHA256SUMS"
name="sandbox-assets-$(grep -v '^url' "$lock" | sha256sum | cut -c1-16)"
echo "[sandbox-release] the release $name in $out; its lock's url is $(awk -F'\t' '$1 == "url" { print $2 }' "$lock")"
(cd "$out" && for file in *; do printf '%12s  %s\n' "$(wc -c < "$file" | tr -d ' ')" "$file"; done)
