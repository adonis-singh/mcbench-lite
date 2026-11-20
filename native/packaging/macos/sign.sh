#!/bin/bash
# Signs an assembled mcbench-lite.app ad hoc, as every macOS asset ships it.
#
#   native/packaging/macos/sign.sh <mcbench-lite.app>
#
# The build child is signed first, with no entitlements and a fixed
# identifier (codesign would otherwise make one from its name and a hash). Then the bundle: its
# main executable gets the one entitlement the app has,
# com.apple.security.virtualization (native/crates/sandbox/macos/
# vm.entitlements), without which the bash tool's VM cannot start, and the
# signature seals Info.plist, Contents/Resources (install-source, version, the icon)
# and the nested build child. No hardened runtime: without a
# Developer ID nothing is notarised, so the runtime would protect nothing a
# user relies on, and V8's JIT in the build child and sandbox-exec would then
# need entitlements of their own (docs/native-app.md, Packaging tools).
# --timestamp=none keeps the signature free of the time, so two signings of
# the same files give the same bytes.
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: sign.sh <mcbench-lite.app>" >&2; exit 2; }
app=$1
entitlements="$(cd "$(dirname "$0")/../../crates/sandbox/macos" && pwd)/vm.entitlements"
[ -f "$app/Contents/MacOS/mcbench-build-child" ] || { echo "sign.sh: $app has no Contents/MacOS/mcbench-build-child" >&2; exit 2; }

codesign --force --sign - --timestamp=none --identifier io.github.adonis-singh.mcbench-lite.build-child "$app/Contents/MacOS/mcbench-build-child"
codesign --force --sign - --timestamp=none --entitlements "$entitlements" "$app"
codesign --verify --deep --strict "$app"
