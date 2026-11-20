#!/usr/bin/env bash
# The AppImage:
#
#   native/packaging/linux/appimage.sh <payload folder> <version> <arch> <out folder>
#
# writes <out>/mcbench-lite-<arch>.AppImage: the payload folder in
# usr/lib/mcbench-lite with install-source "appimage" and version, the desktop entry,
# metainfo and icons under usr/share and at the image's root as
# appimagetool wants them, and AppRun, a symbolic link to
# usr/lib/mcbench-lite/mcbench-lite (the app resolves it, so it finds its two
# helpers beside it). Made by appimagetool with the static type-2 runtime
# (both pinned by sha256 in pins.sh, run in DEBIAN_IMAGE with
# --appimage-extract-and-run, so no FUSE is needed to make it); the image
# needs no libfuse2 to run. appimagetool runs natively, so <arch> is this
# machine's.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
packaging_args native/packaging/linux/appimage.sh "$@"
[ "$arch" = "$(uname -m)" ] || { echo "this machine is $(uname -m): the $arch AppImage is made on an $arch machine" >&2; exit 1; }
new_work

tool_url=APPIMAGETOOL_URL_$arch tool_sum=APPIMAGETOOL_SHA256_$arch
runtime_url=APPIMAGE_RUNTIME_URL_$arch runtime_sum=APPIMAGE_RUNTIME_SHA256_$arch
fetch "${!tool_url}" "${!tool_sum}" "$work/appimagetool"
fetch "${!runtime_url}" "${!runtime_sum}" "$work/runtime"
chmod 755 "$work/appimagetool"

dir=$work/mcbench-lite.AppDir
install_payload "$dir/usr/lib/mcbench-lite" appimage
install_share "$dir/usr/share"
install -m 644 "$here/mcbench-lite.desktop" "$dir/mcbench-lite.desktop"
install -m 644 "$icons/mcbench-lite-256.png" "$dir/mcbench-lite.png"
ln -s mcbench-lite.png "$dir/.DirIcon"
ln -s usr/lib/mcbench-lite/mcbench-lite "$dir/AppRun"
name=mcbench-lite-$arch.AppImage
in_image "$DEBIAN_IMAGE" "apt-get -qq update && apt-get -qq install -y --no-install-recommends file >/dev/null
cd /w && ARCH=$arch VERSION=$version ./appimagetool --appimage-extract-and-run --no-appstream --runtime-file runtime mcbench-lite.AppDir $name > tool.log 2>&1 || { cat tool.log; exit 1; }"
install -m 755 "$work/$name" "$out/$name"
echo "$out/$name: $(stat -c %s "$out/$name") bytes"
