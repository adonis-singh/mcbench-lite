#!/usr/bin/env bash
# vm-build.sh [--no-profile] <tree> <data bundle> <update key file> <linux files folder> <out folder>
#
# On adipc: the release build of the Windows x64 payload with MSVC in a fresh
# overlay of the Windows 11 VM's hold slot (hold.sh), with
# native/packaging/build/windows.ps1, as bench: with the profile (pgo.md),
# or with --no-profile its -NoProfile (for tests only: pgo.md, Windows builds
# without a profile). The instrumented build's target (\target-pgo) stays on
# the cache disk between runs. The payload comes back to
# <out folder>, which must not exist.
#
#   <tree>               a tree brought to adipc by content (git ls-files of native
#                        core renderer server scripts package.json package-lock.json
#                        README.md LICENSE NOTICE.md: the corpus recording's
#                        maker, pgo-workload.js, copies server/, scripts/,
#                        renderer/ and the package files)
#   <data bundle>        the Minecraft data bundle native/minecraft-data.lock names
#   <update key file>    the staging minisign public key (minisign.pub)
#   <linux files folder> mcbench-build-child, the Linux x86_64 build child from the
#                        Linux release build
#
# The build's update feed is the address of the staging feed's latest.json as
# the guest sees the host, $FEED, where vm-test.sh serves release B.
#
# Stage five's own cache disk, ~/mcbench-native-scratch/s5/win/cache.qcow2 (a
# sparse 40 GiB qcow2, an NTFS volume labelled mcbs5cache, made when missing),
# holds only cargo's registry, so each build is cold and the disk stays at
# about the registry's size; deleting it is always safe. It is never the
# check's cache.qcow2.
set -euo pipefail

fail() { echo "vm-build.sh: $*" >&2; exit 1; }
PROFILE=1
if [ "${1:-}" = --no-profile ]; then PROFILE=0; shift; fi
[ "$#" = 5 ] || fail "usage: vm-build.sh [--no-profile] <tree> <data bundle> <update key file> <linux files folder> <out folder>"
TESTDIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TREE="$(cd "$1" && pwd)" BUNDLE="$2" KEY="$3" LINUX="$(cd "$4" && pwd)" DEST="$5"
FEED="http://10.0.2.2:3191/latest.json"
for f in native/packaging/build/windows.ps1 scripts package-lock.json; do [ -e "$TREE/$f" ] || fail "$TREE has no $f."; done
[ -f "$BUNDLE" ] || fail "the data bundle $BUNDLE does not exist."
[ -f "$KEY" ] || fail "the update key file $KEY does not exist."
[ -f "$LINUX/mcbench-build-child" ] || fail "$LINUX has no mcbench-build-child."
[ ! -e "$DEST" ] || fail "$DEST exists."

source "$TESTDIR/hold.sh"
CACHE="$S5WIN/cache.qcow2"
work=""
on_exit() { if [ -n "$work" ]; then rm -rf "$work"; fi; }
hold_take vm-build.sh
work="$(mktemp -d "$S5WIN/vm-build.XXXXXX")"
mkdir -p "$work/in/bundle" "$work/in/linux"
cp "$BUNDLE" "$work/in/bundle/"
cp "$KEY" "$work/in/minisign.pub"
cp "$LINUX/mcbench-build-child" "$work/in/linux/"
echo "$FEED" >"$work/in/feed"
# The staging sandbox assets' folder as the guest sees it, when the caller names one.
if [ -n "${MCBENCH_SANDBOX_ASSETS_URL:-}" ]; then echo "$MCBENCH_SANDBOX_ASSETS_URL" >"$work/in/assets-url"; fi
if [ "$PROFILE" = 0 ]; then touch "$work/in/no-profile"; fi
tar -cf "$work/tree.tar" -C "$TREE" .
[ -f "$CACHE" ] || { qemu-img create -q -f qcow2 "$CACHE" 40G; echo "vm-build: made the cache disk $CACHE"; }

hold_boot -drive file="$CACHE",if=none,id=cache,format=qcow2,cache=writeback,discard=unmap,detect-zeroes=unmap \
  -device virtio-blk-pci,drive=cache
bg_wait as bench 'New-Item -ItemType Directory -Force C:\mcb | Out-Null'
bg_wait to_guest "$TESTDIR/guest/build.ps1" "$work/tree.tar" "$work/in" C:/mcb/
bg_wait as bench 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\build.ps1 cache'
t0=$SECONDS
bg_wait as bench 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\build.ps1 build'
echo "vm-build: the release build took $((SECONDS - t0)) s in the guest"
mkdir -p "$(dirname "$DEST")"
bg_wait from_guest C:/mcb/out "$DEST"
(cd "$DEST" && sha256sum -- *)
if [ "$PROFILE" = 1 ]; then bg_wait from_guest C:/mcb/out.profdata "$DEST.profdata"; bg_wait from_guest C:/mcb/out.pgo.log "$DEST.pgo.log"; fi
echo "vm-build: the payload is in $DEST; the cache disk takes $(du -h "$CACHE" | cut -f1) on adipc"
