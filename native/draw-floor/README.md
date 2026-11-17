# The software floor for `mcbench-draw`

This folder is a spike. It establishes what `see_build` draws with on a machine whose graphics give `wgpu` no hardware adapter. Today the headless Chromium carries SwiftShader and always draws; the native helper must carry its own floor on Linux and use the operating system's on Windows and macOS. Everything here was measured on 2026-11-16 with the renderer spike's `shot` and `window` programs (branch `claude/sharp-mendeleev-e55a37`, commit 31f64e6, `native/spikes/renderer/`, wgpu 30.0.2) plus `direct-vulkan.patch`. Nothing in `native/draw/` was changed.

## Decision

| Platform | Floor | Shipped beside the helper | Selected when |
|---|---|---|---|
| Linux x86_64 | SwiftShader's Vulkan driver, built from source, opened directly by the helper (no Vulkan loader) | `libvk_swiftshader.so` (4,613,672 bytes) and its licence texts | wgpu enumerates no adapter whose device type is not `Cpu` |
| Linux aarch64 | the same, built with SwiftShader's LLVM reactor (Subzero has no arm64 back end) | `libvk_swiftshader.so` (size not measured) | same |
| Windows 10/11 x64 and arm64 | WARP ("Microsoft Basic Render Driver"), part of Windows, through wgpu's DX12 backend | nothing | same; wgpu already enumerates WARP as a `Cpu` DX12 adapter |
| macOS | none: Metal or nothing | nothing | no Metal device: the helper replies with an error naming it |

The helper reports the floor in its `hello` reply as `adapter.software: true` (device type `Cpu`), and the server logs it once. The floor is never chosen silently and never chosen while a hardware adapter exists, except when the server asks for it explicitly (below), which the tests and CI need.

## What the helper and the server do (for `s2-draw` and `s2-node`)

The server passes the shipped driver's absolute path in `hello.assets` as `software_vulkan` on Linux, always (it is part of the install, not a probe result). The helper:

1. creates its `wgpu::Instance` as today (all of the platform's backends) and enumerates adapters;
2. takes a hardware adapter if there is one (device type other than `Cpu`), by the existing rule;
3. otherwise, on Linux, opens `software_vulkan` directly: `ash::Entry::load_from(path)`, `vkCreateInstance` with `wgpu::hal::vulkan::Instance::desired_extensions(..)`, `wgpu::hal::vulkan::Instance::from_raw(..)`, `wgpu::Instance::from_hal::<wgpu::hal::api::Vulkan>(..)`, and its single adapter; this is the 20 lines in `direct-vulkan.patch` (`direct_vulkan`), driven there by `MCBENCH_VULKAN_DRIVER`;
4. on Windows, step 2 failing means wgpu's own list holds only WARP (`Cpu`, DX12), which it takes; on macOS, an empty list is an error reply ("No Metal device: ...").

System software adapters (Mesa's llvmpipe through GL, lavapipe through Vulkan) also have device type `Cpu`; the rule passes over them to the shipped SwiftShader, so every Linux machine without a GPU draws with the same tested driver and the same bytes (the container runs below are byte-identical to the host's). A `hello` field `adapter: "software"` selects the floor on a machine that has a GPU, for the parity tests and CI; it runs the same step 3.

Two things the helper must not do: set `VK_DRIVER_FILES`/`VK_ICD_FILENAMES` in its own environment (the direct load needs neither, and they would hide the machine's GPU drivers from step 1), or run with a working directory that may hold a `SwiftShader.ini` (SwiftShader reads `SwiftShader.ini` from the current directory, `src/System/SwiftConfig.cpp:90`, and it can set its thread count and affinity). Start the helper with its own install folder as its working directory.

Why direct loading and not the loader: a minimal server or container has no `libvulkan.so.1` (checked: `debian:12`, `ubuntu:24.04`, `almalinux:9` have none, nor any Mesa, EGL, GL or X library). Shipping the Khronos loader as well works (proved below with `LD_LIBRARY_PATH` plus `VK_DRIVER_FILES=<dir>/vk_swiftshader_icd.json`, which needs a loader built with Vulkan headers 1.3.207 or later; older loaders read only `VK_ICD_FILENAMES`), but it adds a 656 KB library, an environment the server must set only in the floor case, and a second way the hardware path could pick up a stray driver. SwiftShader exports `vkGetInstanceProcAddr` itself (checked with `objdump -T`), which is all `ash` needs.

## Measurements

Machine: adipc, AMD Ryzen 9 9950X3D (16 cores, 32 threads, two CCDs), 62 GB, RTX 5090 (driver 615.78.08), Arch Linux, glibc 2.44, Mesa 26.2.6 on the host and 26.2.7 in the `archlinux:latest` container. Mains power. The owner's desktop session was running but idle; the one-minute load before each run was 5.8 to 12.2, almost all of it this work's own previous runs (software rasterisers keep all their threads busy). Each run is one cold process: device, assets, mesh, upload, the build's 4 shots at 2184x1640 with supersample 2 (4x MSAA), then 3 warm rounds of the same 4 shots. "Warm per shot" is draw + readback + JPEG at quality 90 (JPEG is 29-35 ms of it on every adapter), median (min-max) of 12. "4c/8t" is `taskset -c 0-3,16-19` (lavapipe: `docker --cpuset-cpus`), a stand-in for an ordinary 4-core laptop CPU, though these cores are faster than a laptop's. Raw lines: `results/runs.jsonl` (kept on adipc, see the end).

`see_build` pictures, village40k (42,862 blocks, 38,273 quads):

| Adapter | CPUs | First shot draw | Warm per shot | Cold process, 4 shots | Peak RSS |
|---|---|---|---|---|---|
| RTX 5090, Vulkan | all | 9.5 ms | 40.8 ms (39.8-46.5) | 452 ms | 336 MiB |
| SwiftShader (built here), direct | all | 151 ms | 170 ms (149-192) | 837 ms | 733 MiB |
| SwiftShader (Chrome for Testing 149), direct | all | 162 ms | 172 ms (153-198) | 845 ms | 736 MiB |
| llvmpipe through wgpu GL (host Mesa) | all | 100 ms | 120 ms (113-126) | 598 ms | 774 MiB |
| lavapipe (Arch package, loader) | all | 294 ms | 279 ms (269-290) | 1,315 ms | 747 MiB |
| SwiftShader (built here) | 4c/8t | 175 ms | 191 ms (181-216) | 904 ms | 736 MiB |
| llvmpipe GL | 4c/8t | 301 ms | 180 ms (163-190) | 1,032 ms | 787 MiB |
| lavapipe | 4c/8t | 336 ms | 240 ms (228-261) | 1,246 ms | 762 MiB |

pagoda2m (2,259,658 blocks, 1,302,104 quads):

| Adapter | CPUs | First shot draw | Warm per shot | Cold process, 4 shots | Peak RSS |
|---|---|---|---|---|---|
| RTX 5090, Vulkan | all | 15.8 ms | 41.0 ms (39.1-46.3) | 882 ms | 1,146 MiB |
| SwiftShader (built here) | all | 834 ms | 847 ms (822-921) | 3,947 ms | 1,657 MiB |
| SwiftShader (Chrome for Testing 149) | all | 838 ms | 883 ms (827-912) | 4,060 ms | 1,690 MiB |
| llvmpipe GL | all | 307 ms | 267 ms (188-288) | 1,676 ms | 1,739 MiB |
| lavapipe | all | 627 ms | 360 ms (303-384) | 2,373 ms | 1,928 MiB |
| SwiftShader (built here) | 4c/8t | 541 ms | 511 ms (501-559) | 2,760 ms | 1,551 MiB |
| llvmpipe GL | 4c/8t | 494 ms | 446 ms (258-476) | 2,333 ms | 2,933 MiB |
| lavapipe | 4c/8t | 653 ms | 422 ms (279-459) | 2,451 ms | 1,858 MiB |

SwiftShader runs at most 16 worker threads (`min(logical CPUs, 16)`, `SwiftConfig.cpp:128`) and on this two-CCD CPU it was slower with more CPUs: pagoda2m warm per shot 887 ms on all 32, 895 ms on 8 cores/16 threads across both CCDs (0-7,16-23), 504 ms on 4 cores/8 threads of one CCD (0-3,16-19), 862 ms on 4 threads (0-3); with all CPUs and `ThreadCount` forced through `SwiftShader.ini`: 690 ms at 8, 720 ms at 4, 814 ms at 12. The one-CCD 8-thread case is the representative one for the floor's target machines (one CPU complex, 4-8 cores); a large desktop with no GPU is not the floor's case. Tuning the thread count is not proposed: it would be a measured shortcut for one CPU layout.

First-picture cost (the Windows VM's 42.4 s first pagoda2m picture on WARP against 1-2.7 s later): not reproduced by any Linux software driver. SwiftShader's first pagoda2m draw is the same as its warm draws (834 ms against 847 ms); lavapipe's first draw costs 267 ms more than its warm draws (627 against 360: its shaders are compiled at the first draw); llvmpipe's 40 ms more. Upload of the 288 MiB of geometry took 100 ms on lavapipe and is inside "cold" for the others. So there is nothing on Linux to measure for `s2-draw`'s WARP work; the cost is WARP's own.

Colour against the GPU (the renderer report's cross-backend check, `scripts/pngdiff.py`: mean absolute difference <= 0.5 and p99 of the per-pixel max-channel difference <= 4, all 4 shots of both builds):

| Against the RTX 5090 | village40k mean / p99 | pagoda2m mean / p99 | Passes |
|---|---|---|---|
| SwiftShader (built here) | 0.15-0.31 / 1 | 0.16-0.33 / 1 | all 8 |
| SwiftShader (Chrome for Testing 149) | 0.22-0.41 / 1-5 | 0.20-0.47 / 1-7 | 6 of 8: the top orthographic shot fails p99 (5 and 7) |
| llvmpipe GL | 0.15-0.32 / 1 | 0.21-0.32 / 1-3 | all 8 |
| lavapipe | 0.15-0.32 / 1 | 0.21-0.32 / 1-3 | all 8 |
| SwiftShader (built here) against llvmpipe GL | 0.002-0.09 / 0-3 | | all 8 |

Chrome's SwiftShader differs from the one built here at the same SwiftShader commit (f9d5d49a): along texel and block edges of the top orthographic view, where texel boundaries fall on pixel boundaries, and in a uniform one-level offset of the sky. Chromium builds it with its own clang and GN flags; the cause was not looked for further. The source build is the one recommended, for this and for its licence.

Interactive frame time (the later stage's live view, for the stated floor): `window --mode offscreen --size 1280,800`, camera orbiting every frame, 4x MSAA, the whole build drawn (no culling, no level of detail), 10 s each, mean frame time and frames per second, draw only (with readback within 3%):

| Adapter | CPUs | village40k | pagoda2m |
|---|---|---|---|
| RTX 5090 | all | 0.10 ms | 0.38 ms |
| SwiftShader | all | 25.4 ms, 39 fps (p99 28.0) | 742 ms, 1.3 fps |
| SwiftShader | 4c/8t | 14.5 ms, 69 fps (p99 16.0) | 367 ms, 2.7 fps |
| llvmpipe GL | all | 10.6 ms, 94 fps | 235 ms, 4.3 fps |
| llvmpipe GL | 4c/8t | 14.5 ms, 69 fps | 289 ms, 3.5 fps |
| lavapipe | all | 10.2 ms, 98 fps | 205 ms, 4.9 fps |
| lavapipe | 4c/8t | 12.5 ms, 80 fps | 227 ms, 4.4 fps |

Peak RSS of the window process: 227-261 MiB at 40k, 1,032-1,432 MiB at 2M. On the floor, a 40k build orbits at 39-69 fps and a 2M build does not orbit (1.3-2.7 fps on SwiftShader, at most 4.9 on any software driver). The live view's stated floor is therefore: on a software adapter, builds up to about 100 thousand blocks interactive at 1280x800; larger builds need the frustum culling and level of detail the renderer report already lists, and until then the view says it is on a software renderer and draws at reduced resolution.

## The container proofs

`scripts/contain.sh` runs the glibc-2.28 `shot` in `debian:12` (glibc 2.36), `ubuntu:24.04` (2.39) and `almalinux:9` (2.34), each as it is pulled: no Mesa, no X, no EGL/GL, no `libvulkan.so.1`, no `/usr/share/vulkan` (listed by the script before the runs), with `--network none` and as user 65534. Mounted read only: the program, the driver folder, the data. Three runs each:

| Image | No driver | Driver opened directly | Loader + `VK_DRIVER_FILES` |
|---|---|---|---|
| debian:12 | fails: "No graphics adapter: ... vulkan drivers/libraries could not be loaded, ... gl drivers/libraries could not be loaded" | village40k and pagoda2m drawn, all 8 shots pass against the GPU (mean 0.15-0.33, p99 1) | same, same pictures |
| ubuntu:24.04 | same failure | village40k drawn, 4 shots pass (same numbers) | same |
| almalinux:9 | same failure | village40k drawn, 4 shots pass (same numbers) | same |

The container pictures are byte-identical to the host's SwiftShader pictures. The "no driver" run also shows that wgpu 30 with no loader and no EGL reports an error and does not crash. Image digests: debian:12 `sha256:268fb24c39bc…`, ubuntu:24.04 `sha256:73b999393d07…`, almalinux:9 `sha256:245f8aab832f…` (full digests in `results/images.txt` on adipc).

## What ships, and how the release obtains it

Linux x86_64: `libvk_swiftshader.so`, 4,613,672 bytes stripped, sha256 `1637fb0f1a642dd99b9c857e4355144239c8a3e7d7742c08da6498e7f71dbe8b`. Needs only `libc.so.6`, `libm.so.6`, `libdl.so.2`, `libpthread.so.0`, `librt.so.1`; highest symbol version GLIBC_2.27 (under the engine's 2.28); libstdc++ and libgcc linked in; non-executable stack. No manifest is needed for the direct load.

`scripts/build-swiftshader.sh` is the reproducible recipe: SwiftShader at `f9d5d49a3c599a315e3493dc1e9b5309cffb3305` (the revision Chromium 149.0.7827.55's DEPS pins; dated 2026-05-04), fetched by commit hash from `https://swiftshader.googlesource.com/SwiftShader`, built in `almalinux:8` (digest `sha256:2d0f2eb2a83c…`) with gcc-toolset-13 and CMake 3.26.5 from AlmaLinux's repositories, Subzero reactor, tests, benchmarks, PowerVR examples and ASTC off, `-static-libstdc++ -static-libgcc -Wl,-z,noexecstack`. Building the library takes 28 s of compile on 24 jobs (55 s with the container's package install). Two builds from scratch gave the same sha256. The release script (`scripts/release-draw.js`, owned by `s2-node`) should run this recipe in the release workflow, refuse a result whose sha256 differs from the pinned one, and put the library in the Linux archive next to `mcbench-draw`, as `server/tools.js` pins the engine's downloads by sha256 and size. The `-z noexecstack` flag is required: marl's fiber assembly lacks the GNU-stack note, and glibc 2.41 and later refuse to `dlopen` a library that asks for an executable stack (the host here has 2.44).

Linux aarch64: the same recipe with `-DREACTOR_BACKEND=LLVM` (SwiftShader's `reactor.gni`: "Subzero doesn't support ARM64"; its bundled `third_party/llvm-16.0` is compiled in), built natively in `almalinux:8` arm64 on a GitHub `ubuntu-24.04-arm` runner. Not built or measured here; expect a much larger library and a longer build because of LLVM.

Licences and notices to ship with it (Linux): SwiftShader's `LICENSE.txt` (Apache-2.0; the repository has no NOTICE file) and the licences of the third-party code compiled into the library, from the SwiftShader tree at that commit: `third_party/marl/LICENSE` (Apache-2.0), `third_party/SPIRV-Tools/LICENSE` (Apache-2.0), `third_party/SPIRV-Headers/LICENSE` (Khronos MIT-style), `third_party/subzero/LICENSE.TXT` and `third_party/llvm-subzero/LICENSE.TXT` (the LLVM release licence, University of Illinois/NCSA). On aarch64 add `third_party/llvm-16.0`'s licence (Apache-2.0 with LLVM exception). The release should copy these files into a `licenses/swiftshader/` folder of the archive.

Taking the library out of the Chrome for Testing zip that `server/tools.js` pins today was considered and rejected. It is the same size (4,663,480 bytes, glibc 2.17) and the zip also holds a Khronos loader, but Google states no licence for Chrome for Testing binaries and Chrome's terms forbid redistribution; its pictures fail the p99 threshold on top views (above); and stage two removes the Chromium download.

Windows: nothing ships. macOS: nothing ships.

## Candidates compared

| | SwiftShader (built) | lavapipe (Mesa) | llvmpipe through GL |
|---|---|---|---|
| Licence | Apache-2.0 (+ NCSA, MIT-style parts) | MIT (Mesa), LLVM: Apache-2.0 with exception | same as lavapipe |
| What it takes to ship | one 4.6 MB library | the driver plus LLVM: Arch's package closure is 221 MB (`libvulkan_lvp.so` 14.9 MB, `libLLVM.so.23.1` 173 MB, ICU 35 MB, SPIRV-Tools, X/xcb/wayland/drm libraries); a static-LLVM build: see below | the same Mesa plus EGL/GLVND, and wgpu's GL backend with its limits (no Rgba32Float target, Fifo only) |
| Loader-free load | yes (`vkGetInstanceProcAddr` exported) | only through `vk_icdGetInstanceProcAddr` (`ash::Entry::from_static_fn`) | no (EGL) |
| Build | 28 s, CMake, no LLVM on x86_64 | Meson, LLVM static libraries, Python mako/yaml, bison, flex | n/a |
| Pictures against GPU | pass, p99 1 | pass, p99 1-3 | pass, p99 1-3 |
| 2M shot, 4c/8t | 511 ms | 422 ms | 446 ms |
| 40k shot, 4c/8t | 191 ms | 240 ms | 180 ms |

lavapipe draws a 2M build about 20% faster than SwiftShader on 8 threads and up to 2.4 times faster on 32, and is slower on small builds. It is the faster rasteriser for big scenes, at 10 to 40 times the shipped size and a much heavier build. SwiftShader is recommended: `see_build`'s pictures on the floor cost 0.2 s (40k) to 0.5 s (2M) per shot on an ordinary CPU, against 1.6-1.8 s per shot for today's Chromium path on the Mac.

A self-contained lavapipe is practical but large. `scripts/lavapipe-static.sh` built Mesa 26.2.7 (tarball checked against the sha256 in its release notes) in `debian:13` with lavapipe only and LLVM 19 linked statically: `libvulkan_lvp.so` is 69,917,096 bytes stripped, still needs `libz.so.1`, `libzstd.so.1` and `libdrm.so.2` (the last is absent from minimal images; Mesa's Meson build requires libdrm even with no window system), exports only `vk_icdGetInstanceProcAddr`, and has symbol versions up to GLIBC_2.38 because of the build image (an older base would be needed, with LLVM's static libraries, Polly and SPIRV-Tools for it). It draws correctly (pagoda2m against the GPU: mean 0.21-0.32, p99 1-3) at 405 ms (270-459) per warm shot on 4c/8t, peak RSS 1,630 MiB. Getting it to build took four rounds of missing build dependencies (libdrm, expat, Polly, libxml2/z3); a release recipe would have to carry all of that, against SwiftShader's one CMake call.

## Windows: WARP

Microsoft's WARP page (https://learn.microsoft.com/en-us/windows/win32/direct3darticles/directx-warp) states WARP is "present in DXGI adapter enumeration" from Windows 8, supports Direct3D 12 feature levels 12_0 and 12_1 from Windows 10 1709, "does not require graphics hardware to execute", and covers services and server environments; it applies to "Windows 8, Windows 10, Windows Server 2012 & above". The Basic Display driver page (https://learn.microsoft.com/en-us/windows-hardware/drivers/display/microsoft-basic-display-driver) says Basic Display "is always used with BasicRender" (WARP) when no vendor driver is present. Windows 11 inherits it (the Windows VM ran Direct3D 12 on WARP, feature level 12_1, with only the Basic Display Adapter). Not stated anywhere I found: Server Core, Remote Desktop sessions, session 0, and Windows container images. Microsoft's container GPU page (https://learn.microsoft.com/en-us/virtualization/windowscontainers/deploy-containers/gpu-acceleration) only says Server Core and Nano Server images are not supported for GPU acceleration; Nano Server has no Win32 graphics stack, so expect no Direct3D there. Microsoft's WARP DLLs from NuGet may not be redistributed, so there is nothing to ship.

wgpu 30.0.2 needs nothing special: its DX12 backend enumerates adapters with `EnumAdapters1` and keeps the software WARP adapter, reported as `DeviceType::Cpu` (`wgpu-hal/src/auxil/dxgi/factory.rs`, `wgpu-hal/src/dx12/adapter.rs` at v30.0.2); `force_fallback_adapter: true` restricts the choice to `Cpu` adapters (`wgpu-core/src/instance.rs`), which is how the floor is forced for tests. The selection rule above therefore needs no Windows-specific code.

Checks that need Windows: (a) GitHub's free runners: build the helper for x64 and arm64, draw both builds on WARP and compare with the reference pictures at the thresholds; the runners are Windows Server Datacenter, which settles "Server" but not Server Core, so add a job running the helper in `mcr.microsoft.com/windows/servercore:ltsc2022` and in `mcr.microsoft.com/windows/nanoserver:ltsc2022` with process isolation, expecting a picture in the first and the helper's no-adapter error in the second. (b) A real machine: nothing for the floor itself; a Windows machine whose GPU driver is broken or disabled (Device Manager) should show the helper on WARP with `software: true`.

## macOS: Metal only

wgpu's Metal backend targets macOS 10.13 and later (`wgpu-hal/src/metal/mod.rs` at v30.0.2) and enumerates devices with `MTLCopyAllDevices`; every Mac that runs a macOS version the app supports has a Metal GPU (Apple's Metal support list, https://support.apple.com/en-us/102894). In a macOS guest of Apple's Virtualization framework on Apple silicon, Metal runs on the paravirtual GPU ("Apple Paravirtual device"; the rig's guest ran wgpu on it, and GitHub's arm64 `macos-14` and later runners expose it: WebKit bug 278277 reports WebGL working there). Parallels guests on Intel or Apple silicon reach Metal through the host (https://kb.parallels.com/124095). Without a Metal device (old Intel-based CI runners: actions/runner-images issue 1779, "MTLCreateSystemDefaultDevice returned nil", 2020; QEMU or VMware guests without GPU support) wgpu finds no adapter. Apple ships no software Metal renderer. SwiftShader builds for macOS (Chrome ships `libvk_swiftshader.dylib`), and the direct load would work there through wgpu's Vulkan backend, but no supported Mac lacks Metal, so the floor on macOS is "no Metal device: refuse with the error", stated in the README's requirements.

Checks: (a) GitHub `macos-14`/`macos-15` arm64 runners: build, draw both builds on the paravirtual Metal device, compare at the thresholds. `macos-13` (Intel) runners, while they exist, show the no-adapter error path.

## One driver for all three platforms?

SwiftShader builds for Windows (`vk_swiftshader.dll`, Visual Studio 2019 or later) and macOS (`libvk_swiftshader.dylib`), and the direct load is the same code on all three, so the floor could be identical everywhere. Against each platform's own: on Windows it would add about 5 MB per architecture and a Visual Studio build of SwiftShader (arm64 with LLVM) to the release, replace a driver Microsoft maintains and that is always present with one the app maintains, and it would be chosen over WARP, which already passes the pictures (mean 0.14-0.28 against Vulkan, p99 1). On macOS it would add a library for a case that does not occur on supported Macs. What it would buy is identical pictures across platforms on the floor, which the thresholds already make unnecessary. Recommended: SwiftShader on Linux only, WARP on Windows, nothing on macOS.

## Files here

| File | What it does |
|---|---|
| `direct-vulkan.patch` | the spike's `Gpu::new` opens `MCBENCH_VULKAN_DRIVER` directly; adds `ash = "0.38"` |
| `scripts/env.sh` | adipc paths, toolchain, cargo home and target in the scratch folder |
| `scripts/build-spike.sh` | Mac: spike at 31f64e6 + patch to adipc, built there |
| `scripts/build-spike-228.sh` | the same programs built in `almalinux:8` (glibc 2.28) for the containers |
| `scripts/build-swiftshader.sh` | SwiftShader from source, pinned, x86_64 |
| `scripts/measure.sh` | one `shot` run on gpu, llvmpipe-gl, swiftshader or swiftshader-cft, optional CPU set |
| `scripts/lavapipe.sh` | one `shot` run on Arch's lavapipe in `archlinux:latest`, and its package closure |
| `scripts/lavapipe-static.sh` | Mesa 26.2.7 lavapipe with static LLVM, for its size |
| `scripts/compare.sh`, `scripts/pngdiff.py` | the cross-backend colour check between two runs |
| `scripts/contain.sh` | the container proof |
| `scripts/fps.sh` | the interactive frame time at 1280x800 |
| `scripts/summarize.py` | one line per run for `runs.jsonl` |

Order to reproduce: `build-spike.sh` (Mac), then on adipc `build-swiftshader.sh`, `build-spike-228.sh`; Chrome's copy for `swiftshader-cft` was copied from the renderer spike's Chromium 149 headless shell into `$R/drivers/swiftshader-cft/`; then `measure.sh <adapter> <build> 3 [cpus]`, `lavapipe.sh <build> 3 [cpus]`, `compare.sh gpu <adapter> <build>`, `contain.sh <image> <build>`, `fps.sh <adapter> <build> 10 [cpus]`.
