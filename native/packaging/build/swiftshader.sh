#!/usr/bin/env bash
# The software Vulkan driver of the Linux x86_64 payload: SwiftShader's
# libvk_swiftshader.so, which the app's renderer opens itself (no Vulkan
# loader) on a machine with no GPU adapter.
#
#   native/packaging/build/swiftshader.sh <cache> <out>
#
# writes <out>/libvk_swiftshader.so and <out>/licenses/swiftshader/, the
# licences of everything compiled into it. <out> must exist and hold neither.
# The driver is built from SwiftShader's sources at COMMIT (the commit
# Chromium 149.0.7827.55 pins) in IMAGE (AlmaLinux 8, glibc 2.28, by digest)
# with gcc-toolset-13 and the Subzero reactor, libstdc++ and libgcc linked in
# and a non-executable stack, then stripped, and refused unless its sha256 and
# size are SHA256 and BYTES, the bytes two builds from scratch gave (the same
# recipe and pin as scripts/release-draw.js FLOOR and buildFloor, which build
# the helper's release; native/draw-floor/README.md). It runs on x86_64 with
# Docker, in a container named mcbn-s5-swiftshader-<pid> limited to
# CARGO_BUILD_JOBS CPUs (default 2) and MCBENCH_BUILD_MEMORY (default 8g).
#
# <cache> keeps the fetched source (<cache>/src) and the built, checked driver
# with its licences (<cache>/floor); a later run copies <cache>/floor when its
# driver's sha256 is SHA256 and builds nothing. Delete <cache> for a build from
# scratch.
#
# Linux aarch64 has no driver: it needs SwiftShader's LLVM reactor built on an
# arm64 machine, which has never been built.
set -euo pipefail

COMMIT=f9d5d49a3c599a315e3493dc1e9b5309cffb3305
SOURCE=https://swiftshader.googlesource.com/SwiftShader
IMAGE=almalinux:8@sha256:2d0f2eb2a83cc09dbc2eb92fade4dbe8f709628149e81aa00aec70630406603b
SHA256=1637fb0f1a642dd99b9c857e4355144239c8a3e7d7742c08da6498e7f71dbe8b
BYTES=4613672
LICENSES='LICENSE.txt third_party/marl/LICENSE third_party/SPIRV-Tools/LICENSE third_party/SPIRV-Headers/LICENSE third_party/subzero/LICENSE.TXT third_party/llvm-subzero/LICENSE.TXT'
FILE=libvk_swiftshader.so

[ $# -eq 2 ] || { echo "usage: native/packaging/build/swiftshader.sh <cache> <out>" >&2; exit 2; }
[ "$(uname -m)" = x86_64 ] || { echo "this machine is $(uname -m): SwiftShader's driver is built on an x86_64 machine" >&2; exit 1; }
[ -d "$2" ] || { echo "$2 is not a folder" >&2; exit 1; }
[ ! -e "$2/$FILE" ] && [ ! -e "$2/licenses/swiftshader" ] || { echo "$2 already holds $FILE or licenses/swiftshader" >&2; exit 1; }
mkdir -p "$1"
cache=$(cd "$1" && pwd)
out=$(cd "$2" && pwd)
floor=$cache/floor

# check <file>: fails unless <file> is the pinned driver.
check() {
  local sum bytes
  sum=$(sha256sum "$1" | cut -c1-64)
  bytes=$(stat -c %s "$1")
  [ "$sum" = "$SHA256" ] && [ "$bytes" = "$BYTES" ] || { echo "$1 has sha256 $sum and $bytes bytes; the pin is $SHA256 and $BYTES" >&2; exit 1; }
}

if [ -d "$floor" ]; then
  check "$floor/$FILE"
  echo "swiftshader: $floor/$FILE from the cache"
else
  src=$cache/src
  if [ ! -d "$src/.git" ]; then
    /bin/rm -rf "$src"
    mkdir -p "$src"
    git -C "$src" init -q
    git -C "$src" remote add origin "$SOURCE"
    git -C "$src" fetch -q --depth 1 origin "$COMMIT"
    git -C "$src" checkout -q FETCH_HEAD
  fi
  [ "$(git -C "$src" rev-parse HEAD)" = "$COMMIT" ] || { echo "$src is not SwiftShader $COMMIT" >&2; exit 1; }
  jobs=${CARGO_BUILD_JOBS:-2}
  /bin/rm -rf "$cache/build" "$cache/floor.part"
  mkdir -p "$cache/build"
  SECONDS=0
  docker run --rm --name "mcbn-s5-swiftshader-$$" --cpus "$jobs" --memory "${MCBENCH_BUILD_MEMORY:-8g}" \
    -v "$cache:/w" -w /w "$IMAGE" bash -c "
set -eu
trap 'chown -R $(id -u):$(id -g) /w' EXIT
dnf -q -y install gcc-toolset-13-gcc-c++ cmake make python3 git >/dev/null
source /opt/rh/gcc-toolset-13/enable
cmake -S src -B build -DCMAKE_BUILD_TYPE=Release -DREACTOR_BACKEND=Subzero -DSWIFTSHADER_BUILD_TESTS=OFF -DSWIFTSHADER_BUILD_BENCHMARKS=OFF -DSWIFTSHADER_BUILD_PVR=OFF -DSWIFTSHADER_WARNINGS_AS_ERRORS=OFF -DSWIFTSHADER_ENABLE_ASTC=OFF '-DCMAKE_SHARED_LINKER_FLAGS=-static-libstdc++ -static-libgcc -Wl,-z,noexecstack' >/dev/null
cmake --build build --target vk_swiftshader -j $jobs >build/build.log 2>&1 || { tail -40 build/build.log; exit 1; }
strip -o build/$FILE build/Linux/$FILE"
  echo "swiftshader: built in $SECONDS s at $jobs CPUs"
  mkdir -p "$cache/floor.part/licenses/swiftshader"
  cp "$cache/build/$FILE" "$cache/floor.part/$FILE"
  chmod 644 "$cache/floor.part/$FILE"
  check "$cache/floor.part/$FILE"
  for license in $LICENSES; do
    install -m 644 "$src/$license" "$cache/floor.part/licenses/swiftshader/${license//\//-}"
  done
  /bin/rm -rf "$cache/build"
  mv "$cache/floor.part" "$floor"
fi
install -m 644 "$floor/$FILE" "$out/$FILE"
install -d -m 755 "$out/licenses/swiftshader"
install -m 644 "$floor/licenses/swiftshader/"* "$out/licenses/swiftshader/"
echo "swiftshader: $out/$FILE, sha256 $SHA256, $BYTES bytes; $(ls "$out/licenses/swiftshader" | wc -l) licences in $out/licenses/swiftshader"
