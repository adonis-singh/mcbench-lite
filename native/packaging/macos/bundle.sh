#!/bin/bash
# Assembles and signs mcbench-lite.app, the macOS payload folder. The one
# macOS route is install.sh (the site's curl | sh command), so install-source
# is always script.
#
#   native/packaging/macos/bundle.sh <mcbench-lite> <mcbench-build-child> <version> <out>
#
# <mcbench-lite> and <mcbench-build-child> are the two executables of one
# architecture (build/macos.sh). <version> is MAJOR.MINOR.PATCH. <out> is a
# folder that does not hold mcbench-lite.app yet; the signed bundle is written
# there, holding these five files and nothing else (the bash tool's guest
# base is downloaded by the app at first start, not shipped):
#
#   mcbench-lite.app/Contents/Info.plist                       Info.plist.in with the version
#   mcbench-lite.app/Contents/MacOS/mcbench-lite
#   mcbench-lite.app/Contents/MacOS/mcbench-build-child
#   mcbench-lite.app/Contents/Resources/install-source         script and a newline
#   mcbench-lite.app/Contents/Resources/version                <version> and a newline (the app compares it with its own at start)
#   mcbench-lite.app/Contents/Resources/mcbench-lite.icns      native/packaging/icon
#
# Everything is in place before sign.sh runs, because a file added to a signed
# bundle breaks its signature. Files are copied without extended attributes
# (cp -X). Info.plist.in's keys beyond the usual ones: LSMinimumSystemVersion
# 12.0 is the release's MACOSX_DEPLOYMENT_TARGET (build/macos.sh); and
# NSSupportsAutomaticGraphicsSwitching lets a dual-GPU Intel Mac stay on its
# integrated GPU, which the window's Metal renderer picks
# (gpui-pre-apple's metal_renderer.rs prefers the low-power device).
set -euo pipefail
die() { echo "bundle.sh: $*" >&2; exit 2; }
[ $# -eq 4 ] || die "usage: bundle.sh <mcbench-lite> <mcbench-build-child> <version> <out>"
lite=$1 child=$2 version=$3 out=$4
here=$(cd "$(dirname "$0")" && pwd)
icon="$here/../icon/mcbench-lite.icns"

[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "the version $version is not MAJOR.MINOR.PATCH"
for f in "$lite" "$child" "$icon"; do
  [ -f "$f" ] || die "$f is missing"
done
case "$(lipo -archs "$lite")" in arm64|x86_64) ;; *) die "$lite is not one arm64 or x86_64 executable" ;; esac
[ "$(lipo -archs "$child")" = "$(lipo -archs "$lite")" ] || die "$child is not of the same architecture as $lite"

app="$out/mcbench-lite.app"
[ ! -e "$app" ] || die "$app already exists"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
sed "s/@VERSION@/$version/g" "$here/Info.plist.in" > "$app/Contents/Info.plist"
plutil -lint -s "$app/Contents/Info.plist"
cp -X "$lite" "$app/Contents/MacOS/mcbench-lite"
cp -X "$child" "$app/Contents/MacOS/mcbench-build-child"
chmod 0755 "$app/Contents/MacOS/mcbench-lite" "$app/Contents/MacOS/mcbench-build-child"
printf 'script\n' > "$app/Contents/Resources/install-source"
printf '%s\n' "$version" > "$app/Contents/Resources/version"
cp -X "$icon" "$app/Contents/Resources/mcbench-lite.icns"
chmod 0644 "$app/Contents/Info.plist" "$app/Contents/Resources/"*

"$here/sign.sh" "$app"
