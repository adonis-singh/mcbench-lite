#!/bin/sh
# On adipc: Mesa's lavapipe (Arch's vulkan-swrast package, Mesa 26.2.x with
# its shared libLLVM, the same Mesa as the host's llvmpipe) inside
# archlinux:latest, driving the glibc-2.28 `shot` through the Khronos loader
# with VK_DRIVER_FILES naming lavapipe's manifest. Packages are cached in
# $R/pacman-cache. Also reports what a lavapipe install consists of (the
# driver library, its shared-library closure and their sizes).
#   scripts/lavapipe.sh <build> [rounds] [cpus]
set -eu
. "$(dirname "$0")/env.sh"
build=$1 rounds=${2:-3} cpus=${3:-}
label=lavapipe${cpus:+-cpus$cpus}
out=$R/results/$label/$build
mkdir -p "$out" "$R/pacman-cache"
cp "$MINI/out/$build/job.json" "$MINI/out/$build/blueprint.bin" "$out/"
before=$(cut -d' ' -f1-3 /proc/loadavg)
docker run --rm ${cpus:+--cpuset-cpus "$cpus"} -v "$R:/r" -v "$DATA:/data:ro" archlinux:latest bash -euc '
  trap "chown -R '"$(id -u):$(id -g)"' /r/pacman-cache /r/results" EXIT
  pacman -Sy --noconfirm --needed --cachedir /r/pacman-cache vulkan-swrast vulkan-icd-loader >/dev/null 2>&1
  if [ ! -f /r/results/lavapipe-closure.txt ]; then
    { pacman -Q vulkan-swrast llvm-libs vulkan-icd-loader; ls -l /usr/lib/libvulkan_lvp.so; ldd /usr/lib/libvulkan_lvp.so | awk "{print \$3}" | grep / | xargs ls -lL | awk "{s+=\$5; print \$5, \$9} END {print s, \"bytes in the closure\"}"; } > /r/results/lavapipe-closure.txt
  fi
  VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json /r/target-228/release/shot /data /r/results/'"$label/$build"' see-build '"$rounds"' > /r/results/'"$label/$build"'/stdout.json
'
after=$(cut -d' ' -f1-3 /proc/loadavg)
python3 -I "$(dirname "$0")/summarize.py" "$out/native-see-build-times.json" "$label" "$before" "$after" >> "$R/results/runs.jsonl"
tail -1 "$R/results/runs.jsonl"
