#!/bin/sh
# On adipc: the container proof. In <image> with nothing graphical installed
# (no Mesa, no X, no Vulkan loader: checked first), with no network and as an
# unprivileged user, the glibc-2.28 `shot` and the files shipped beside it
# draw the build's 4 shots. Three runs per image:
#   none     no driver given: must fail with wgpu's "no adapter" error
#   direct   MCBENCH_VULKAN_DRIVER=<driver library>: the driver opened directly
#   loader   a Khronos loader beside it (LD_LIBRARY_PATH; the libvulkan.so.1
#            from Chrome for Testing 149's headless shell, for this proof only)
#            with VK_DRIVER_FILES naming the driver's manifest
# Pictures go to $R/results/contain-<image>-<run>/<build>/, compared by compare.sh.
#   scripts/contain.sh <image> <build> [driver folder, default $R/drivers/swiftshader]
set -eu
. "$(dirname "$0")/env.sh"
image=$1 build=$2 drivers=${3:-$R/drivers/swiftshader}
tag=$(echo "$image" | tr ':/' '--')
run() {
  name=$1; shift
  out=$R/results/contain-$tag-$name/$build
  mkdir -p "$out"
  cp "$MINI/out/$build/job.json" "$MINI/out/$build/blueprint.bin" "$out/"
  chmod 777 "$out"
  docker run --rm --network none --user 65534:65534 \
    -v "$R/target-228/release/shot:/app/shot:ro" -v "$drivers:/app/drivers:ro" -v "$R/drivers/swiftshader-cft/libvulkan.so.1:/app/loader/libvulkan.so.1:ro" \
    -v "$DATA:/data:ro" -v "$out:/out" "$@" "$image" /app/shot /data /out see-build 1
}
echo "== $image: graphical libraries present (expect none)"
docker run --rm --network none "$image" sh -c 'cat /etc/os-release | grep PRETTY; ldd --version 2>&1 | head -1; for d in /lib /lib64 /usr/lib /usr/lib64 /usr/lib/x86_64-linux-gnu; do ls $d 2>/dev/null | grep -E "^lib(vulkan|EGL|GL|GLX|X11|gbm|drm)\." ; done; ls /usr/share/vulkan /etc/vulkan /usr/share/glvnd 2>&1 | head -3; true'
echo "== $image: no driver"
if run none 2>&1; then echo "UNEXPECTED: drew without a driver"; exit 1; fi
echo "== $image: driver opened directly"
run direct -e MCBENCH_VULKAN_DRIVER=/app/drivers/libvk_swiftshader.so
echo "== $image: a Khronos loader beside it + VK_DRIVER_FILES"
run loader -e LD_LIBRARY_PATH=/app/loader -e VK_DRIVER_FILES=/app/drivers/vk_swiftshader_icd.json
