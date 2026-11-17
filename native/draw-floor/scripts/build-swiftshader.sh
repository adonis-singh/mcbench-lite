#!/bin/sh
# On adipc: SwiftShader's Vulkan driver built from source at the commit
# Chromium 149.0.7827.55 pins (its DEPS: swiftshader_revision), x86_64,
# Subzero reactor (what Chromium uses on x86_64), inside almalinux:8 so the
# library needs nothing newer than glibc 2.28, libstdc++ and libgcc linked in,
# and a non-executable stack (marl's fiber assembly lacks the note; glibc 2.41
# and later refuse to dlopen a library that asks for an executable stack).
# Output: $R/drivers/swiftshader/{libvk_swiftshader.so,vk_swiftshader_icd.json}
#   scripts/build-swiftshader.sh
set -eu
. "$(dirname "$0")/env.sh"
SWIFTSHADER_COMMIT=f9d5d49a3c599a315e3493dc1e9b5309cffb3305
SRC=$R/swiftshader/src
if [ ! -d "$SRC/.git" ]; then
  mkdir -p "$SRC"
  git -C "$SRC" init -q
  git -C "$SRC" remote add origin https://swiftshader.googlesource.com/SwiftShader
  git -C "$SRC" fetch -q --depth 1 origin "$SWIFTSHADER_COMMIT"
  git -C "$SRC" checkout -q FETCH_HEAD
fi
test "$(git -C "$SRC" rev-parse HEAD)" = "$SWIFTSHADER_COMMIT"
mkdir -p "$R/swiftshader/build" "$R/drivers/swiftshader"
docker run --rm --cpus 24 -v "$R/swiftshader:/w" -w /w almalinux:8 bash -euc '
  trap "chown -R '"$(id -u):$(id -g)"' /w" EXIT
  dnf -q -y install gcc-toolset-13-gcc-c++ cmake make python3 git >/dev/null
  source /opt/rh/gcc-toolset-13/enable
  cmake --version | head -1
  cmake -S src -B build -DCMAKE_BUILD_TYPE=Release -DREACTOR_BACKEND=Subzero \
    -DSWIFTSHADER_BUILD_TESTS=OFF -DSWIFTSHADER_BUILD_BENCHMARKS=OFF -DSWIFTSHADER_BUILD_PVR=OFF \
    -DSWIFTSHADER_WARNINGS_AS_ERRORS=OFF -DSWIFTSHADER_ENABLE_ASTC=OFF \
    -DCMAKE_SHARED_LINKER_FLAGS="-static-libstdc++ -static-libgcc -Wl,-z,noexecstack" >/dev/null
  time cmake --build build --target vk_swiftshader -j 24 >build.log 2>&1 || { tail -40 build.log; exit 1; }
'
OUT=$R/swiftshader/build/Linux
cp "$OUT/libvk_swiftshader.so" "$R/drivers/swiftshader/"
strip "$R/drivers/swiftshader/libvk_swiftshader.so"
cp "$OUT/vk_swiftshader_icd.json" "$R/drivers/swiftshader/"
ls -l "$R/drivers/swiftshader"
sha256sum "$R/drivers/swiftshader/"*
