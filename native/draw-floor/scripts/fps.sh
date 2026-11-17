#!/bin/sh
# On adipc: the interactive view's frame time on one adapter. The spike's
# `window --mode offscreen` orbits the camera every frame (4x MSAA, no
# culling or LOD, the whole build uploaded) into a 1280x800 target for
# SECONDS seconds, first drawing only, then also reading each frame back.
#   scripts/fps.sh <adapter> <build> [seconds] [cpus]
#   adapter: gpu | llvmpipe-gl | swiftshader | lavapipe (lavapipe runs in archlinux:latest)
# Appends the spike's stat lines, labelled, to $R/results/fps.jsonl.
set -eu
. "$(dirname "$0")/env.sh"
adapter=$1 build=$2 seconds=${3:-10} cpus=${4:-}
label=$adapter${cpus:+-cpus$cpus}
before=$(cut -d' ' -f1-3 /proc/loadavg)
args="$DATA $MINI/out/$build --mode offscreen --size 1280,800 --seconds $seconds"
case $adapter in
  gpu) out=$(${cpus:+taskset -c $cpus} "$R/target/release/window" $args) ;;
  llvmpipe-gl) out=$(${cpus:+taskset -c $cpus} env WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json "$R/target/release/window" $args 2>/dev/null) ;;
  swiftshader) out=$(${cpus:+taskset -c $cpus} env MCBENCH_VULKAN_DRIVER=$R/drivers/swiftshader/libvk_swiftshader.so "$R/target/release/window" $args) ;;
  lavapipe) out=$(docker run --rm ${cpus:+--cpuset-cpus "$cpus"} -v "$R:/r" -v "$DATA:$DATA:ro" -v "$MINI/out:$MINI/out:ro" archlinux:latest bash -euc '
      trap "chown -R '"$(id -u):$(id -g)"' /r/pacman-cache" EXIT
      pacman -Sy --noconfirm --needed --cachedir /r/pacman-cache vulkan-swrast vulkan-icd-loader >/dev/null 2>&1
      VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json /r/target-228/release/window '"$args") ;;
  *) echo "unknown adapter $adapter" >&2; exit 2 ;;
esac
after=$(cut -d' ' -f1-3 /proc/loadavg)
echo "$out" | while read -r line; do
  echo "{\"run\":\"$label\",\"build\":\"$build\",\"load_before\":\"$before\",\"load_after\":\"$after\",${line#\{}"
done | tee -a "$R/results/fps.jsonl"
