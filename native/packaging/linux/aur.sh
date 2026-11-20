#!/usr/bin/env bash
# The AUR package mcbench-lite-bin for one release:
#
#   native/packaging/linux/aur.sh <release folder> <version> <out folder> <arch>...
#
# writes <out>/PKGBUILD (from PKGBUILD.in) and <out>/.SRCINFO (makepkg
# --printsrcinfo in ARCH_IMAGE). Unlike the other package scripts it takes
# the release folder rather than a payload: the package installs from the
# release's Linux archives, so for each <arch> named,
# mcbench-lite-linux-<arch>.tar.gz must be in the release folder, and its
# sha256 is the PKGBUILD's. The archives are read from the release
# v<version> under MCBENCH_LITE_RELEASES (pins.sh).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
[ $# -ge 4 ] || { echo "usage: native/packaging/linux/aur.sh <release folder> <version> <out folder> <arch>..." >&2; exit 2; }
release=$(cd "$1" && pwd)
version=$2
mkdir -p "$3"
out=$(cd "$3" && pwd)
shift 3
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version $version is not MAJOR.MINOR.PATCH" >&2; exit 1; }
download=$(release_download "$version")

arches= sources=
for a in "$@"; do
  deb_arch "$a" >/dev/null
  f=$release/mcbench-lite-linux-$a.tar.gz
  [ -f "$f" ] || { echo "$f is missing: make it with native/packaging/linux/archive.sh" >&2; exit 1; }
  arches="$arches '$a'"
  sources+="source_$a=(\"mcbench-lite-linux-$a-\$pkgver.tar.gz::$download""mcbench-lite-linux-$a.tar.gz\")"$'\n'
  sources+="sha256sums_$a=('$(sha256sum "$f" | cut -d' ' -f1)')"$'\n'
done
# The libraries the programs link and load (README.md, The programs'
# libraries), as Arch names their packages, and bubblewrap; fontconfig, which
# the app loads only when present, is in PKGBUILD.in's optdepends.
depends="'glibc' 'gcc-libs' 'libxcb' 'libxkbcommon' 'libxkbcommon-x11' 'libglvnd' 'wayland' 'vulkan-icd-loader' 'bubblewrap' 'hicolor-icon-theme'"

new_work
awk -v version="$version" -v arches="${arches# }" -v depends="$depends" -v sources="$sources" '
  { gsub(/@VERSION@/, version); gsub(/@ARCHES@/, arches); gsub(/@DEPENDS@/, depends) }
  /^@SOURCES@$/ { printf "%s", sources; next }
  { print }' "$here/PKGBUILD.in" > "$work/PKGBUILD"
in_image "$ARCH_IMAGE" "useradd -m b && chown -R b /w && su b -c 'cd /w && makepkg --printsrcinfo > .SRCINFO'"
mv "$work/PKGBUILD" "$work/.SRCINFO" "$out/"
echo "$out/PKGBUILD, $out/.SRCINFO: mcbench-lite-bin $version for$arches"
