#!/bin/sh
# On adipc: whether a self-contained lavapipe is practical. Mesa 26.2.7 (the
# release tarball, checked against the sha256 in its release notes) built inside
# debian:13 with lavapipe only (no GL, no window systems) and LLVM 19 linked
# statically (-Dshared-llvm=disabled), so the driver needs no libLLVM.
# Output: $R/drivers/lavapipe-static/{libvulkan_lvp.so,lvp_icd.x86_64.json},
# its size and its shared-library needs. (debian:13 has glibc 2.41: a
# shipped build would be made on an older base, as build-swiftshader.sh does.)
set -eu
. "$(dirname "$0")/env.sh"
MESA=mesa-26.2.7
# From the release notes, https://docs.mesa3d.org/relnotes/26.2.7.html
MESA_SHA256=04294f75d22e9f61cd1e050206bc48b3d39366e788f1803a3368e3d5a872ce18
mkdir -p "$R/lavapipe" "$R/drivers/lavapipe-static"
cd "$R/lavapipe"
[ -f "$MESA.tar.xz" ] || curl -sfLO "https://archive.mesa3d.org/$MESA.tar.xz"
echo "$MESA_SHA256  $MESA.tar.xz" | sha256sum -c -
[ -d "$MESA" ] || tar -xf "$MESA.tar.xz"
docker run --rm --cpus 24 -v "$R/lavapipe:/w" -w /w debian:13 bash -euc '
  trap "chown -R '"$(id -u):$(id -g)"' /w" EXIT
  apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends \
    gcc g++ meson ninja-build pkg-config python3-mako python3-yaml python3-packaging bison flex \
    llvm-19-dev libclang-common-19-dev zlib1g-dev libzstd-dev libelf-dev glslang-tools libexpat1-dev libdrm-dev ca-certificates libpolly-19-dev libxml2-dev libz3-dev >/dev/null
  cd '"$MESA"'
  meson setup build --wipe -Dbuildtype=release -Db_ndebug=true \
    -Dgallium-drivers=llvmpipe -Dvulkan-drivers=swrast -Dplatforms= \
    -Dglx=disabled -Degl=disabled -Dgbm=disabled -Dopengl=false -Dgles1=disabled -Dgles2=disabled \
    -Dllvm=enabled -Dshared-llvm=disabled -Dvalgrind=disabled -Dlibunwind=disabled -Dxmlconfig=disabled \
    -Dzstd=disabled -Dvideo-codecs= -Dbuild-tests=false \
    -Dc_link_args="-static-libgcc -static-libstdc++" -Dcpp_link_args="-static-libgcc -static-libstdc++" \
    --native-file <(printf "[binaries]\nllvm-config = '"'"'/usr/bin/llvm-config-19'"'"'\n") > /w/meson.log 2>&1 || { tail -30 /w/meson.log; exit 1; }
  time ninja -C build -j 24 src/gallium/targets/lavapipe/libvulkan_lvp.so > /w/ninja.log 2>&1 || { tail -30 /w/ninja.log; exit 1; }
'
cp "$MESA/build/src/gallium/targets/lavapipe/libvulkan_lvp.so" "$R/drivers/lavapipe-static/"
strip "$R/drivers/lavapipe-static/libvulkan_lvp.so"
ls -l "$R/drivers/lavapipe-static/libvulkan_lvp.so"
ldd "$R/drivers/lavapipe-static/libvulkan_lvp.so"
objdump -T "$R/drivers/lavapipe-static/libvulkan_lvp.so" | grep -E ' (vkGetInstanceProcAddr|vk_icdGetInstanceProcAddr)$' || true
printf '{"file_format_version":"1.0.1","ICD":{"library_path":"./libvulkan_lvp.so","api_version":"1.4.0"}}\n' > "$R/drivers/lavapipe-static/lvp_icd.x86_64.json"
