#!/bin/sh
# On adipc: one `shot` run (the spike's test program: cold process, the
# build's 4 shots at 2184x1640 with supersample 2, then ROUNDS warm rounds of
# the same 4 shots) on one adapter, optionally pinned to some CPUs.
#   scripts/measure.sh <adapter> <build> [rounds] [cpus]
#   adapter: gpu            the machine's own (RTX 5090, Vulkan)
#            llvmpipe-gl    Mesa llvmpipe through wgpu's GL backend (host Mesa)
#            swiftshader    the driver built by build-swiftshader.sh, loaded directly
#            swiftshader-cft  the one in Chrome for Testing 149's headless shell, loaded directly
#   cpus: a taskset list, e.g. 0-3,16-19 (4 cores, 8 threads)
# Writes $R/results/<adapter>[-cpus]/<build>/native-see-build-*.png and
# native-see-build-times.json, and appends one line to $R/results/runs.jsonl.
set -eu
. "$(dirname "$0")/env.sh"
adapter=$1 build=$2 rounds=${3:-3} cpus=${4:-}
label=$adapter${cpus:+-cpus$cpus}
out=$R/results/$label/$build
mkdir -p "$out"
cp "$MINI/out/$build/job.json" "$MINI/out/$build/blueprint.bin" "$out/"
case $adapter in
  gpu) set -- ;;
  llvmpipe-gl) set -- env WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json ;;
  swiftshader) set -- env MCBENCH_VULKAN_DRIVER=$R/drivers/swiftshader/libvk_swiftshader.so ;;
  swiftshader-cft) set -- env MCBENCH_VULKAN_DRIVER=$R/drivers/swiftshader-cft/libvk_swiftshader.so ;;
  *) echo "unknown adapter $adapter" >&2; exit 2 ;;
esac
[ -n "$cpus" ] && set -- taskset -c "$cpus" "$@"
before=$(cut -d' ' -f1-3 /proc/loadavg)
"$@" "$R/target/release/shot" "$DATA" "$out" see-build "$rounds" > "$out/stdout.json"
after=$(cut -d' ' -f1-3 /proc/loadavg)
python3 -I "$(dirname "$0")/summarize.py" "$out/native-see-build-times.json" "$label" "$before" "$after" >> "$R/results/runs.jsonl"
tail -1 "$R/results/runs.jsonl"
