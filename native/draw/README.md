# mcbench-draw

`see_build`'s pictures drawn natively: the mesher's quads (`renderer/`, used
as a library) with `see_build`'s shading, camera, sky, fog, log depth, mips,
sizes and supersample, entities from the bake's entity capture, the
labelled grid, JPEG. A library and one program, `mcbench-draw`. The server
runs `mcbench-draw serve` as a long-lived helper; `PROTOCOL.md` is the
contract between them.

## Build

Rust 1.99.1 (the repository's toolchain), from this folder:

```
cargo build --release
```

The program is `target/release/mcbench-draw` (`.exe` on Windows). It needs
no system library beyond the platform's graphics: Metal on macOS; Vulkan or
GL on Linux; Direct3D 12 or Vulkan on Windows. On Linux without a GPU it
draws with SwiftShader's Vulkan driver, shipped beside it and opened
directly (`native/draw-floor/README.md`); on Windows without a GPU with
WARP. It builds with MSVC for `x86_64-pc-windows-msvc` and checks for
`aarch64-pc-windows-msvc`.

## Test

```
cargo test --release
```

Without Minecraft's data the tests draw the synthetic pack
(`tests/synthetic`, made by `src/synthetic.rs`: the project's own blocks
and textures) against the committed reference pictures and quads, run the
helper's protocol, and check edits against fresh loads. Tests that need
Minecraft's data say they are skipped. With a data folder of today's bake:

```
MCBENCH_DRAW_DATA=<data folder> MCBENCH_DRAW_BUILD=<folder with blueprint.bin and job.json> cargo test --release
```

adds every entity kind against `web/entity-draw.mjs` run in Node, the page
tests' drawing cases (`tests/viewer.rs`) and edits of a real build. Other
settings: `MCBENCH_DRAW_SOFTWARE=1` draws on the software floor;
`MCBENCH_DRAW_SWIFTSHADER=<libvk_swiftshader.so>` names the floor's
driver on Linux; `MCBENCH_DRAW_WRITE_REFERENCES=1` writes the synthetic
references instead of comparing (only after a deliberate change to the
pictures, on the Mac's Metal).

## Run by hand

```
mcbench-draw shot <data folder> <build folder> <out folder> [--repeat N] [--software] [--software-vulkan <lib>] [--phases]
```

draws every shot of `<build folder>/job.json` (the renderer spike's
format: `yOffset` and resolved shots) over `blueprint.bin` (and
`entities.json` beside it), and writes `<i>.png` (the read-back picture),
`<i>.jpg` and `times.json`. `--phases` times a first picture's parts one
submission at a time.

```
mcbench-draw compare <a.png|jpg> <b.png|jpg>
```

prints the colour statistics and SSIM of two pictures and whether they are
within the thresholds of "the same picture" (`src/compare.rs`).

```
mcbench-draw serve
```

speaks `PROTOCOL.md` on stdin and stdout; start it with its install
folder as the working directory.

## The same picture on every adapter

`src/compare.rs` holds the thresholds. Before the JPEG (the read-back
picture): mean absolute difference at most 0.5 and p99 of the largest
channel difference at most 4 across adapters. On the path models get (JPEG
at quality 90, decoded): mean at most 1.0, p99 at most 12, pixels over 16
levels at most 0.3%, SSIM of luma at least 0.993. The JPEG thresholds come
from measurement: over the verifier's 112 pictures (five fixed worlds and
twelve community builds, every kind of `see_build` call, replayed from the
requests the real server sent), Metal, an RTX 5090 (Vulkan), SwiftShader
and WARP agree before the JPEG at mean at most 0.17 and p99 at most 3, and
their decoded JPEGs at worst at mean 0.60, p99 10, 0.14% of pixels over 16
levels and SSIM 0.9957. A JPEG turns a difference of a level at a few
pixels into a different rounding of whole 8x8 blocks, and each JPEG is
itself mean 0.39 to 2.51 and p99 2 to 16 from its source. Against those
worst cases the thresholds are 1.7 times the mean, 1.2 times the p99, 2.1
times the share over 16 levels and 1.6 times SSIM's distance from 1; they
still fail a wrong shader, matrix, mip level, blend or draw order (mean 1
to 6, p99 over 25).

Flat colours are the same on every adapter: the shaders round their
output to 8 bits themselves (hardware conversions round differently: the
RTX 5090 rounded halves down), the sky's gradient is written without
`smoothstep`'s reversed edges (which WGSL leaves undefined), and the
downscale rounds the average of the resolved pixels itself. The
multisample resolve is the hardware's: exact where a pixel's samples are
alike, it may round an edge pixel's average a level apart between
adapters. Resolving the samples in the downscale instead made every shot
of a 2 million block world 4 to 9 ms slower on the M3 (the samples written
out of tile memory and read back) for no measurable gain.

## Memory

`hello`'s `memoryLimit` bounds the process's own memory between requests
(PROTOCOL.md, What is kept): 1 GiB by default on a GPU, 768 MiB on a
software adapter. The helper counts what it keeps, by its sizes in memory
(and the GPU's memory where it is the process's, as the device reports
it), over an overhead measured at `hello` and once more after the first
reply that drew a picture, and lets go what it keeps before it allocates (as a world's buffers are
made; before a call's picture targets) and after every reply; it gives
freed memory back once no request waits, and no reply waits for memory.
Until 2026-11-18 it chose what to keep by the process's resident memory
read after the trim, which on Linux held up to about 260 MB of free
memory at the top of the meshing threads' glibc arenas, more or less by
which threads ran when: on adipc at load 21 to 99 a small world's scene
was let go after a large world in 7 of 415 runs, and the scene-count test
of `tests/serve.rs` failed so. A
world's quads go to the GPU a buffer (at most 128 MB) at a time as its
tiles are meshed, on a meshing pool of 8 threads (fewer on a machine with
fewer processors; src/scene.rs MESH_THREADS) and the tiles handed over in
order, so the CPU never holds a whole large world's quads, and the
meshing's own memory (each thread's tile and at most two tiles a thread
ahead of the upload) does not grow with the machine's processor count.

The process's memory in MB of `mcbench-draw serve` (`native/draw` at the
commit that changed this section), each with its peak so far: ready;
pagoda2m (2.26 million blocks) drawn, 4 shots; the same after a one-block
edit; four more worlds drawn (village40k, cyberpunk1m, storage,
twitter506k); a 3.2 million block checkerboard of stone and glass (every
stone's six faces drawn: 9.7 million quads, 1,043 MB of them at 108 bytes
a quad) and the same of stone and light blue stained glass, each world and
2 shots; 5 s idle. Linux: VmRSS (peak VmHWM), 0.1 s after each reply; macOS: the
physical footprint (peak its lifetime maximum), 2 s after each reply
(0.1 s after, it passes the limit by what Metal still caches: up to 1,818
MB after the checkerboard):

| Adapter, quads | ready | pagoda2m | its edit | four worlds | stone and glass | stained glass | idle |
|---|---|---|---|---|---|---|---|
| RTX 5090, Vulkan (1 GiB), 108 bytes | 159 (183) | 614 (614) | 636 (636) | 824 (856) | 621 (1,498) | 662 (1,498) | 662 |
| the same, 100 bytes (62075aa3) | 159 (181) | 599 (599) | 622 (622) | 772 (830) | 430 (1,404) | 587 (1,404) | 587 |
| SwiftShader (768 MiB), 108 bytes | 64 (76) | 388 (928) | 392 (928) | 331 (1,349) | 189 (1,909) | 255 (1,909) | 255 |
| the same, 100 bytes | 63 (75) | 405 (946) | 404 (946) | 387 (1,139) | 264 (1,731) | 250 (1,731) | 250 |
| Mac M3, Metal (1 GiB), 108 bytes | 111 (111) | 721 (1,045) | 505 (1,045) | 926 (1,604) | 188 (2,747) | 216 (2,837) | 216 |
| the same, 100 bytes | 111 (111) | 671 (995) | 457 (995) | 900 (1,569) | 122 (2,478) | 156 (2,547) | 156 |

Measured 2026-11-17 with each quad corner's sky and block light added to
the quad (100 to 108 bytes, for the live view's night; the 100-byte rows
are the same steps with the helper of 62075aa3, alternated): adipc with
the helper on 4 of its cores (load 12.7 to 15.8, other sessions' builds
running), SwiftShader built from the commit the release pins; the Mac on
AC (load 2.4 to 3.8). Between runs the peaks and what is held after a
reply move by tens of MB and more (the trim runs once no request waits,
and the 0.1 s and 2 s waits catch it at different points), so a row's
difference from the row under it is the quads' 8% only in the peaks of
the checkerboards (1,498 against 1,404 MB on the 5090, 1,909 against
1,731 on SwiftShader, 2,837 against 2,547 on the M3). The table before
these quads (the same steps, other four worlds; adipc at load 6 to 9, the
Mac at 4.6 to 6) had 213 (232), 657 (657), 680 (680), 863 (901), 854
(1,222), 905 (1,226), 905 on the 5090; 65 (78), 388 (930), 387 (930), 339
(1,279), 278 (1,753), 241 (1,753), 241 on SwiftShader; 111 (111), 722
(1,013), 508 (1,013), 835 (1,510), 117 (2,024), 227 (2,056), 189 on the
M3. The re-verification of
draw-a21d8e98d4b9c15a measured, for the same steps through the server
(its `memoryLimit` 768 MiB), 1,864 to 1,961 MB after the checkerboards on
the RTX 5090 (peak 3.9 GB), 2,278 to 2,713 MB on SwiftShader (peak 5.4 GB)
and 2.2 to 2.5 GB on the M3 (with 100-byte quads). Where the memory goes for the stone and glass
checkerboard: its blocks and light 35 to 40 MB; its tiles' quads on the
CPU while meshed, a tile a thread of the meshing pool's 8 (about 86 MB a
thread at that density)
and at most a buffer waiting (128 MB); the quads on the GPU 1,043 MB (the
process's own on SwiftShader and the M3, not on the 5090); the picture
targets 519 MB on SwiftShader and the 5090, 82 MB on the M3; the pictures
being encoded about 25 MB a shot; freed memory the allocators keep (300 MB
with glibc, 400 MB with mimalloc after such a world, given back before
the process is counted over the limit); on the M3, what Metal caches for
a draw (228 MB of purgeable memory and 100 MB of graphics memory after
pagoda2m's shots), which it gives back by itself about 1.5 s later.

A 3.2 million block world of fully exposed blocks cannot be drawn on
SwiftShader under 1.9 GB (the target was 2 GB): its 1,043 MB of quads, the
519 MB of picture targets of a large shot and the meshing in flight are
all the process's own memory during the call. On Linux the system
allocator returned freed memory better than mimalloc (583 against 888 MB
idle on the 5090), so mimalloc is used on macOS only; since 2026-11-18
glibc runs in one arena with fixed trim and mmap thresholds (src/main.rs). The server stops a
helper that is idle, so idle memory is none: a new helper with pagoda2m
sent again is ready in 0.61 to 0.66 s on the RTX 5090 with 4 cores, 0.58
s on SwiftShader.

## Translucent blocks

Blocks Minecraft draws in its translucent layer (`render-layers.tsv`:
stained glass, tinted glass, ice, slime and honey blocks, water) are drawn
blended at their textures' own alpha (stained glass 102 of 255, its rim
155 and 163; tinted glass 110 and 200; ice 190; slime 180; honey 189 and
191), in the order Minecraft 1.21.10 draws them, read from its client
(LevelRenderer, SectionOcclusionGraph, Octree, ChunkSectionsToRender,
SectionRenderDispatcher.RenderSection, VertexSorting and MeshData, through
Mojang's mappings):

| What | Minecraft's order |
|---|---|
| Sections | the reverse of the order its octree visits them: a cube of sections around the camera's (render distance 12, Options' default, from the overworld's floor, `dimension_type/overworld.json`), each branch visiting first the child on the camera's side of each axis, the axis the camera is farthest along deciding first (Octree$AxisSorting); a section is drawn whole |
| Quads of a section | farthest first by the f32 squared distance from the camera (relative to the section's origin) to each quad's centroid, the midpoint of its corners 0 and 2; a stable sort, ties in the order the section was meshed (its blocks x fastest, then y, then z) |

The helper keeps each quad's section and centroid and orders them for each
shot's camera (`store::octree_visits`, `store::sort_quads`;
`tests/translucent.rs` holds the section order to an octree built and
walked as Minecraft's code builds and walks it). Where a world reaches past
12 chunks from the camera (Minecraft draws no section beyond its render
distance) the octree is made larger to hold it. A scene's translucent
quads go to the GPU once, section by section; a shot writes only their
order (4 bytes a quad). Glass (the cutout layer) is drawn opaque where its
texture is.

## Files

| Path | What |
|---|---|
| `src/assets.rs` | the bake's files: pack, texture names, block atlas with its mips, block facts, entity capture and its atlas from the client jar |
| `src/scene.rs` | the mesher's tiles meshed in parallel and shaded with `see_build`'s equations, as 108-byte quads (each corner's sky and block light kept for the live view's night) |
| `src/entities.rs` | `web/entity-draw.mjs` ported, and the placing and light of each entity's quads |
| `src/blueprint.rs` | the engine's blueprint: clips, and an edit as the mesher's patch |
| `src/store.rs` | worlds and scenes kept, per tile; edits applied to a kept world |
| `src/gpu.rs`, `src/shader.wgsl`, `src/down.wgsl` | adapter choice, pipelines, offscreen pictures |
| `src/floor.rs`, `src/warp.rs` | SwiftShader opened directly (Linux); WARP's buffers in cached memory (Windows) |
| `src/picture.rs` | the 2D canvas step (alpha, grid), JPEG, montage |
| `src/compare.rs` | the checks of the same picture |
| `src/serve.rs` | the helper |
| `fonts/` | Liberation Sans Bold 2.1.5 (SIL Open Font License 1.1, `fonts/LICENSE`), the grid's labels |

`unsafe` code is only in `src/floor.rs` and `src/warp.rs`: a Vulkan driver
loaded and handed to wgpu-hal, and a Direct3D 12 buffer made and handed to
wgpu.
