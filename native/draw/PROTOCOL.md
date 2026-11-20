# `mcbench-draw serve`: the protocol

`mcbench-draw serve` is a long-lived helper that draws `see_build`'s
pictures. The server starts it once, speaks to it over its stdin and stdout,
and restarts it if it exits. This file is the contract between the helper
(`native/draw`) and the server's client; it is kept true with every change to
either.

## Starting it

```
mcbench-draw serve
```

The server starts it with its install folder as the working directory:
on Linux the software floor (SwiftShader) reads `SwiftShader.ini` from the
working directory, and none must be found elsewhere. The helper sets no
environment variable of its own (no `VK_ICD_FILENAMES`, `VK_DRIVER_FILES`).

stdout carries frames only. stderr carries log lines only (one line each,
starting `mcbench-draw:`); the server may show or drop them. The helper
exits with status 0 when stdin ends, after the request in hand is answered.

## Frames

Every request and every reply is one frame:

| Bytes | What |
|---|---|
| 4 | the header's length in bytes, unsigned 32-bit little-endian |
| n | the header: one UTF-8 JSON object |
| ... | the payloads the header lists, back to back, in order |

The header lists its payloads' lengths in bytes as `payloads: [n, ...]`; a
header without `payloads` has none. Limits: a header is at most 16 MiB, a
frame has at most 64 payloads, each at most 2 GiB. A frame that breaks them
or is not JSON ends the helper with status 2 and a log line (the stream can
no longer be trusted).

Every request has an `id` (a JSON number or string, unique among the
requests in flight) and a `type`. Every reply repeats the `id` and has
either `ok: true` with its fields, or `error: "<plain sentence>"`. A request
of an unknown type, with a field its type does not take, or without one it
needs, gets an error reply naming it.

Requests are taken in the order they arrive and answered when each is done,
one at a time, so replies come in request order, with one exception: a
`cancel` is answered at once, and the `shots` it cancels then answers
`error: "cancelled"` when it stops (before its next shot).

## Requests

### `hello`

The first request, once. Any other request before it gets
`error: "The helper has not been given its assets (hello) yet"`; a second
`hello` gets `error: "hello was already given"`.

| Field | What |
|---|---|
| `assets` | the files the helper draws with, all absolute paths (below) |
| `adapter` | optional: `"software"` draws on the software floor even where a hardware adapter exists (tests and CI); absent, the rule below |

`assets`:

| Field | File (in a data folder of today's bake) | Read for |
|---|---|---|
| `pack` | `minecraft-live/bluemap/<bake>/web/mcbench-pack.bin` | the mesher's pack (`renderer/`): states, models, light facts, tints |
| `textures` | `minecraft-live/bluemap/<bake>/web/maps/live/textures.json` | the pack's texture names by material index |
| `atlas` | `assets/1.21.10/atlas.png` | `see_build`'s block texture atlas (misode/mcmeta's) |
| `atlasMap` | `assets/1.21.10/atlas_map.json` | each block texture's box in it |
| `blockTable` | `minecraft-live/paper/block-table-<paper>.tsv` | full faces per state (corner shading) |
| `renderLayers` | `minecraft-live/bluemap/<bake>/render-layers.tsv` | which states' models and fluids are drawn blended |
| `entities` | `minecraft-live/bluemap/<bake>/web/mcbench-entities.json` | the entity capture (what Minecraft's entity renderers draw) |
| `softwareVulkan` | Linux only, optional: the shipped SwiftShader driver `libvk_swiftshader.so` beside the helper (native/draw-floor/README.md), given where the build ships one (Linux x86_64 in this stage, not arm64); given on another platform it is an error |
| `clientJar` | `minecraft-live/bluemap/data/minecraft-client-1.21.10.jar` | the entity textures, read at the boxes the capture's `atlas.rects` gives |

Tints are not listed: the mesher's quads carry their tint from the pack.
The bake's `mcbench-entities.png` is not read: it is written through Java's
ImageIO, which brightens grey textures with transparency (a stored 104
becomes 171); the helper reads each texture from the client jar as stored,
as Minecraft does, and places it where the capture's atlas placed it.

The adapter (native/draw-floor/README.md): the helper takes the first
hardware adapter (any device type but `Cpu`, a discrete GPU before an
integrated one) of its platform's backends in turn: Metal on macOS; Vulkan,
then GL on Linux; Direct3D 12, then Vulkan on Windows. With none, or with
`adapter: "software"`, it takes the software floor: on Linux SwiftShader
opened directly from `softwareVulkan` (without `softwareVulkan`: `error:
"This machine has no graphics adapter, and this build for Linux <arch>
ships no software renderer"`, then the hardware reasons; nothing else is
searched for) (no Vulkan loader; system software
drivers such as llvmpipe and lavapipe are passed over, so every machine
without a GPU draws with the same tested driver); on Windows WARP (Direct3D
12's software adapter); on macOS there is none. If no adapter is found:
`error: "No graphics adapter: <the floor's reason> (hardware: <each backend
and wgpu's reason>)"` (on macOS `"No Metal device ..."`), and the helper
stays up answering only that.

Reply:

| Field | What |
|---|---|
| `version` | the helper's version (`mcbench-draw <crate version>`) |
| `adapter` | `{ name, backend, deviceType, driver, driverInfo, software }`; `software` is true when `wgpu` reports the adapter as a CPU rasteriser (`deviceType: "Cpu"`) |
| `limits` | `{ maxTextureSide, maxBufferBytes, maxPictureSide, worldsKept, scenesKept, memoryLimit }` (What is kept) |
| `timings` | milliseconds: `device`, `assets`, `pipelines`, `total` |

The adapter is also logged to stderr once.

### `world`

Gives the helper a world to draw, meshes it and uploads it. A world of a
name already kept is replaced.

| Field | What |
|---|---|
| `world` | a string naming the world (the server's: conversation, revision, tick) |
| `yOffset` | the page's height offset (`minecraftLive.yOffset`): the blueprint is at Minecraft heights, cameras and entities at Minecraft's less this |
| payload 0 | the engine's blueprint bytes (`McbenchEngine.write`'s format) |
| payload 1 | optional: the entities of the tick drawn, UTF-8 JSON, the array `server/shot-world.js` keeps (`result.entities`: the engine's list with positions at the page's heights) |

An edit: a `world` request may name a `base`, a world the helper still
holds that the new one replaces (the server's previous world of the same
conversation):

| Field | What |
|---|---|
| `base` | optional: the name of the world the new one is an edit of |
| `keepBase` | optional, default false: true keeps `base` as its own world beside the new one; false drops it (its name is free) |

The new world is still sent whole (payload 0 is the whole blueprint,
payload 1 the entities). The helper finds the cells that differ from the
base's, applies them through the mesher's own patch (`World::patch`: it
relights only where the edit can reach and names the tiles that can draw
differently), meshes and uploads only those tiles, and keeps the rest of
the base's geometry. The result is the same as a world sent without
`base`, quad for quad and pixel for pixel (the crate's tests hold it to
that). With `keepBase: true` the helper reads the new world fresh, as if
no `base` were given (the base's lit world is not copied). A `base` the
helper does not hold answers `error: "No world named <base> is kept"` with
`unknownWorld: true`: the server then sends the world again without
`base`. The reply says `patched: true` and `tilesMeshed` (with `tiles`, the
world's count) when the edit was applied to the base.

Light is the mesher's (Minecraft's rule over the blueprint's own volume,
with no cell cap); there is no light floor to pass (today's page's
`FLOOR`).

Reply: `quads` (block quads), `entityQuads`, `tiles`, `blocks`, `bounds`
(`{ min, max }`, page heights, of the blocks), `bytes` (GPU geometry),
`missingTextures` (names of textures the block atlas lacks: their quads are
not drawn; empty for today's bake), and
`timings` in milliseconds: `read` (blueprint and light), `mesh` (tiles in
parallel, with shading), `entities`, `upload`, `total`.

### `shots`

Draws resolved shots of a kept world.

| Field | What |
|---|---|
| `world` | a world's name |
| `shots` | an array of shots exactly as `server/shot.js` `resolveShot` returns them (`width`, `height`, `camera`, `clip`, `grid`, `described`; a `tick` beside them is allowed); `described` and `tick` are not read |
| `supersample` | the factor shots are drawn at before they are scaled down (today's `SUPERSAMPLE`, 2) |
| `format` | `{ "type": "jpeg", "quality": <1-100> }` (today: 90), or `{ "type": "png" }`: the same picture before the JPEG, for comparing adapters |

A shot with a `clip` draws only the blocks inside it (and the entities whose
position is inside it): the helper meshes and lights that part of the world
as its own scene once and keeps it with the world.

Reply: one payload per shot, in order, each the encoded picture, and
`shots: [{ width, height, bytes, timings: { scene, draw, readback, grid,
encode, total } }]` (milliseconds; `scene` is meshing a clip's scene, 0 when
kept; `grid` is the 2D canvas step: colour clamped to alpha and the grid
drawn when the shot has one). For a world that is not kept: `error: "No world named <name> is
kept"` with `unknownWorld: true`; the server sends the world again and
retries.

### `montage`

One picture of several JPEGs placed side by side (a bench share's card,
`server/shot.js` `composeMontage`).

| Field | What |
|---|---|
| `size` | `[width, height]` of the card |
| `cells` | `[{ x, y, width, height }, ...]` (`montageCells`), one per payload |
| `quality` | JPEG quality, 1-100 (today: 90) |
| payloads | the JPEG of each cell, in order; each is drawn at its cell's size |

Reply: one payload, the card's JPEG, and `timings: { total }`.
Needs no `hello`.

### `drop`

`world`: a name. Frees the world and its scenes. Reply: `dropped: true`, or
`dropped: false` when it was not kept (the helper evicts worlds by itself).

### `cancel`

`job`: the `id` of a `shots` request in flight. Reply at once:
`found: true` (that request will answer `error: "cancelled"`), or
`found: false` (not in flight: answered already or never sent).

## What is kept

`hello` takes an optional `memoryLimit`: the bytes of memory the helper's
process may hold between requests. The default is 1,073,741,824 (1 GiB) on
a hardware adapter and 805,306,368 (768 MiB) on a software one, whose
"GPU" memory is the process's own. The helper keeps at most 4 worlds and 6
scenes on a hardware adapter, 2 and 3 on a software one. `hello`'s
`limits` give `worldsKept`, `scenesKept` and `memoryLimit`.

What the bound counts (src/store.rs, `Store::counted`): the bytes of what
the helper keeps, by their sizes in memory, the GPU's memory where it is
the process's, and an overhead. Kept on the CPU are each world's blueprint
and entities and each scene's tiles, translucent order, entities and lit
world. On a discrete GPU, whose memory is its own, only what is read back
into the process is counted of the GPU's (the picture targets'
readbacks). On every other adapter (a software adapter's memory is the
process's heap, an integrated GPU's the machine's, Apple's footprint
holds it) the GPU's memory is counted as the device reports it: the blocks
Vulkan's and Direct3D 12's allocator has reserved (memory freed inside a
block stays the device's until the whole block is) or Metal's allocated
size; on GL, whose driver allocates each resource on its own, as the
resources' sizes. The overhead is the process's memory less what the count
holds (src/memory.rs, `held`: Linux VmRSS; Windows the working set; macOS
the physical footprint less what Metal gives back by itself): measured
when `hello` has made the device and the pipelines, and then once, for the
helper's life, after the first reply that drew a picture, when the driver
has made what it keeps for drawing (a pool of host memory on an RTX 5090,
SwiftShader's own memory); until that first draw the count uses the
`hello` figure. Otherwise the process's memory is read only to give freed
memory back (glibc's
malloc_trim, mimalloc's collect on every thread, HeapCompact) when the
process holds more than the limit, never to choose what is kept: the
same requests keep the same worlds and scenes on any load. On Linux
glibc's malloc runs in one arena (src/main.rs): with an arena a thread,
malloc_trim cannot give back the free memory at the top of the other
threads' arenas, and how much of it there is depends on which threads
ran when.

What it keeps: each world's blueprint and entities, each scene's quads on
the GPU (108 bytes a quad), what a scene keeps of its tiles on the CPU, the
lit world of the newest whole-world scene (an edit's base), and the picture
targets of the two picture sizes drawn last (making them again costs every
request tens of milliseconds: a 2184 x 1640 shot at supersample 2 keeps
519 MB, or 82 MB where the adapter keeps multisampled targets in tile
memory, as Apple's GPUs do).

The helper lets go, in this order: the scenes the request has not drawn,
least recently used first; the picture targets, the size drawn longest ago
first; and, after a `shots` request, the scenes it drew (a world whose
scene alone passes the limit is drawn, then let go: the next `shots`
meshes it again from its kept blueprint). Worlds (their blueprints and
entities, small) are bounded by their count. It does so before it
allocates: as a `world` request's scene grows (after each buffer of its
quads, counting the scene so far), and when a `shots` request starts,
counting the picture targets the shots will make; and after every reply
(counts only: 0.1 to 3 µs on adipc). Freed memory is given back once no
request waits: no reply waits for memory to be given back, and no request
for the giving back after the one before (16 to 80 µs on adipc, a
malloc_trim of about 10 ms where the process is over the limit). A `world` request's own scene
is kept for the shots that follow even when it alone passes the limit.

So after a `shots` request, the process holds at most the limit by the
bound's count, or, where what the request needs alone passes it, its
overhead and the kept blueprints; `tests/serve.rs` holds VmRSS to the
limit plus 32 MiB once freed memory is given back. On the M3 the
footprint passes the limit by what Metal still caches until it gives that
back, within 2 s of the reply. During a request the process
holds at most the larger of the limit and what the request itself needs
(its scene's quads where the GPU's memory is the process's, its world's
blocks and light, its picture targets), plus the meshing in flight (a tile
a thread: up to about 80 MB a thread for the densest tiles), a buffer of
quads waiting for the GPU (at most 128 MB) and the pictures being encoded
(about 25 MB a shot). Measured for a 3.2 million block world whose every
stone shows its six faces (9.7 million quads, 966 MB): a peak of 1.75 GB
on SwiftShader (4 threads), 1.23 GB on an RTX 5090 (its quads in the GPU's
memory) and 2.0 to 3.3 GB on the M3 (8 threads; Metal's own memory for the
draw included); `README.md` has the table. The server also stops a helper
that is idle, so idle memory is none, and a new helper and the world sent
again take 0.4 to 0.7 s for a 2 million block world.

## Errors

Every failure is an error reply with a plain sentence. A lost or failed
graphics device (`wgpu` reports the device lost, or a validation or
out-of-memory error on it) answers the request in hand with
`error: "The graphics device was lost: <wgpu's reason>"` and
`deviceLost: true`, and the helper then exits with status 3: the server
restarts it once and retries, as today's code retries a lost WebGL context.
A panic is logged and ends the helper with status 101.
