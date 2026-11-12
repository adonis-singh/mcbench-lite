import { changedLightChunks, computeLight, lightAt } from './light.mjs';
import { mat4 } from '/vendor/gl-matrix/index.js';

// deepslate's bundle (with its own dependencies inlined) is loaded by the
// page as a classic script, which sets this global.
const {
  BlockDefinition,
  BlockModel,
  BlockState,
  Identifier,
  Mesh,
  SpecialRenderers,
  StructureRenderer,
  TextureAtlas,
  upperPowerOfTwo,
} = globalThis.deepslate;

// Deepslate keeps its object-heavy vertex graph after uploading the same data
// to WebGL. Chunk edits rebuild geometry from the world, so only GPU buffers
// and draw counts need to survive between edits.
function retainGpuMesh(mesh) {
  let quads = 0;
  let lines = 0;
  const rebuild = mesh.rebuild;
  const clear = mesh.clear;
  mesh.rebuild = function (...args) {
    const result = rebuild.apply(this, args);
    quads = this.quads.length;
    lines = this.lines.length;
    this.quads = [];
    this.lines = [];
    return result;
  };
  mesh.clear = function (...args) {
    quads = 0;
    lines = 0;
    return clear.apply(this, args);
  };
  // A mesh whose buffers were written directly (the viewer's chunk mesher).
  mesh.uploaded = function (count) {
    quads = count;
    lines = 0;
    this.quads = [];
    this.lines = [];
  };
  mesh.isEmpty = function () { return quads === 0 && lines === 0 && this.quads.length === 0 && this.lines.length === 0; };
  mesh.quadVertices = function () { return (quads + this.quads.length) * 4; };
  mesh.quadIndices = function () { return (quads + this.quads.length) * 6; };
  mesh.lineVertices = function () { return (lines + this.lines.length) * 2; };
  return mesh;
}

const SEE_THROUGH = /(glass|leaves|water|slime|honey|barrier|grate|spawner|roots|ice|chain|bars)/;
// Blocks that hide faces shared with an identical neighbor (a glass wall reads
// as one surface, like in-game) and blocks drawn in the blended pass.
const SELF_CULLING = /^minecraft:(((\w+_)?stained_)?glass|tinted_glass|ice|slime_block|honey_block)$/;
const SEMI_TRANSPARENT = /^minecraft:((\w+_stained_)glass(_pane)?|tinted_glass|ice|frosted_ice|slime_block|honey_block|water)$/;

// Vanilla light emission levels. Blocks whose glow depends on a `lit`/`berries`
// state emit only when that state is true (default states are merged in).
const LUMINANCE = {
  torch: 14, wall_torch: 14, soul_torch: 10, soul_wall_torch: 10,
  redstone_torch: 7, redstone_wall_torch: 7,
  glowstone: 15, sea_lantern: 15, lantern: 15, soul_lantern: 10,
  jack_o_lantern: 15, shroomlight: 15, end_rod: 14, beacon: 15,
  lava: 15, fire: 15, soul_fire: 10,
  campfire: 15, soul_campfire: 10,
  magma_block: 3, glow_lichen: 7, sea_pickle: 6,
  ochre_froglight: 15, verdant_froglight: 15, pearlescent_froglight: 15,
  crying_obsidian: 10, enchanting_table: 7, ender_chest: 7,
  redstone_lamp: 15, furnace: 13, smoker: 13, blast_furnace: 13,
  cave_vines: 14, cave_vines_plant: 14, glow_berries: 14,
  amethyst_cluster: 5, large_amethyst_bud: 4, medium_amethyst_bud: 2, small_amethyst_bud: 1,
};
const LIT_GATED = /^(furnace|smoker|blast_furnace|redstone_lamp|campfire|soul_campfire|redstone_torch|redstone_wall_torch|(\w+_)?candle|cave_vines(_plant)?)$/;

// Vertex darkening per occluding neighbor, vanilla smooth-lighting style.
const AO_CURVE = [1.0, 0.78, 0.58, 0.42];

function luminanceOf(path, props) {
  if (LIT_GATED.test(path) && props.lit !== 'true' && props.berries !== 'true') return 0;
  if (/(^|_)candle$/.test(path)) return 3 * Number(props.candles || 1);
  return LUMINANCE[path] || 0;
}

// Light level 0..15 -> brightness multiplier. Unlit interiors stay readable
// (vanilla "moody" floor) while lit surfaces reach full brightness.
function lightCurve(level) {
  return 0.25 + 0.75 * Math.pow(level / 15, 1.4);
}

// Sun direction shared by the sky and the block lighting (pre-normalized).
const SUN = '0.4467, 0.8437, 0.2978';
// Horizon haze color; fog blends blocks into this so they melt into the sky.
const FOG_COLOR = [0.74, 0.84, 0.94];

// Markers identifying deepslate's chunk shaders, which get swapped for ours.
const DEEPSLATE_VS_MARKER = 'vLighting = normal.y';
const DEEPSLATE_FS_MARKER = 'vTintColor * vLighting';

const MAX_CAMERA_DISTANCE = 12000;
// A standard perspective depth buffer with near 0.1 concentrates almost all
// precision within a few hundred blocks; past ~1500 the error exceeds a block
// and distant builds z-fight into stripes. Remap to logarithmic depth in the
// vertex shader so precision stays sub-block out to the far plane.
const LOG_DEPTH_SCALE = 2 / Math.log2(MAX_CAMERA_DISTANCE + 1);

const CHUNK_VS = `
  attribute vec4 vertPos;
  attribute vec2 texCoord;
  attribute vec4 texLimit;
  attribute vec3 vertColor;
  attribute vec3 normal;
  uniform mat4 mView;
  uniform mat4 mProj;
  uniform float logDepth; // 2/log2(far+1)
  uniform float linearDepth; // 1 for an orthographic lens, whose w is 1
  varying highp vec2 vTexCoord;
  varying highp vec4 vTexLimit;
  varying highp vec3 vTintColor;
  varying highp vec3 vNormal;
  varying highp float vDist;
  void main(void) {
    vec4 viewPos = mView * vertPos;
    gl_Position = mProj * viewPos;
    if (linearDepth < 0.5) gl_Position.z = (log2(max(1.0 + gl_Position.w, 1e-6)) * logDepth - 1.0) * gl_Position.w;
    vTexCoord = texCoord;
    vTexLimit = texLimit;
    vTintColor = vertColor;
    vNormal = normal;
    vDist = length(viewPos.xyz);
  }
`;

// Directional sun + hemisphere ambient. Faces get distinct brightness by
// orientation (like vanilla) plus a warm sun side / cool shadow side, and
// distant geometry fades into the horizon haze.
const CHUNK_FS = `
  precision highp float;
  varying highp vec2 vTexCoord;
  varying highp vec4 vTexLimit;
  varying highp vec3 vTintColor;
  varying highp vec3 vNormal;
  varying highp float vDist;
  uniform sampler2D sampler;
  uniform highp float pixelSize;
  uniform vec3 fogColor;
  uniform vec2 fogRange;
  void main(void) {
    vec4 texColor = texture2D(sampler, clamp(vTexCoord,
      vTexLimit.xy + vec2(0.5, 0.5) * pixelSize,
      vTexLimit.zw - vec2(0.5, 0.5) * pixelSize
    ));
    if (texColor.a < 0.05) discard;
    vec3 n = normalize(vNormal);
    float hemi = n.y * 0.5 + 0.5;
    vec3 ambient = mix(vec3(0.40, 0.42, 0.47), vec3(0.60, 0.63, 0.68), hemi);
    float sunDiff = max(dot(n, vec3(${SUN})), 0.0);
    vec3 light = ambient + vec3(1.0, 0.96, 0.88) * sunDiff * 0.5;
    vec3 color = texColor.rgb * vTintColor * min(light, vec3(1.15));
    float fog = smoothstep(fogRange.x, fogRange.y, vDist);
    gl_FragColor = vec4(mix(color, fogColor, fog), texColor.a);
  }
`;

const SKY_VS = `
  attribute vec2 pos;
  varying vec2 vNdc;
  void main(void) {
    vNdc = pos;
    gl_Position = vec4(pos, 0.0, 1.0);
  }
`;

const SKY_FS = `
  precision highp float;
  varying vec2 vNdc;
  uniform mat4 unproject;
  void main(void) {
    vec4 p = unproject * vec4(vNdc, 1.0, 1.0);
    vec3 dir = normalize(p.xyz / p.w);
    vec3 zenith = vec3(0.30, 0.49, 0.79);
    vec3 horizon = vec3(0.74, 0.84, 0.94);
    vec3 ground = vec3(0.50, 0.57, 0.66);
    vec3 sky = mix(horizon, zenith, pow(clamp(dir.y, 0.0, 1.0), 0.55));
    sky = mix(sky, ground, smoothstep(0.0, -0.35, dir.y));
    float sunAmt = max(dot(dir, vec3(${SUN})), 0.0);
    sky += vec3(1.0, 0.92, 0.75) * pow(sunAmt, 1200.0) * 1.2;
    sky += vec3(1.0, 0.88, 0.62) * pow(sunAmt, 8.0) * 0.12;
    gl_FragColor = vec4(sky, 1.0);
  }
`;

async function loadImage(src) {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error(`Failed to load ${src}`));
    image.src = src;
  });
}

async function loadResources() {
  const [blocks, blockstates, models, uvMap, atlasImage] = await Promise.all([
    fetch('/api/assets/blocks.json').then((r) => r.json()),
    fetch('/api/assets/block_definitions.json').then((r) => r.json()),
    fetch('/api/assets/models.json').then((r) => r.json()),
    fetch('/api/assets/atlas_map.json').then((r) => r.json()),
    loadImage('/api/assets/atlas.png'),
  ]);

  const blockDefinitions = {};
  for (const id of Object.keys(blockstates)) {
    blockDefinitions[`minecraft:${id}`] = BlockDefinition.fromJson(blockstates[id]);
  }

  const blockModels = {};
  for (const id of Object.keys(models)) {
    blockModels[`minecraft:${id}`] = BlockModel.fromJson(models[id]);
  }
  Object.values(blockModels).forEach((m) => m.flatten({ getBlockModel: (id) => blockModels[id] }));

  // A block is opaque (neighbors' hidden faces can be culled) when its default
  // model is a full cube and its id doesn't suggest transparency.
  const opaque = new Set();
  for (const name of Object.keys(blocks)) {
    if (SEE_THROUGH.test(name)) continue;
    const model = blockModels[`minecraft:block/${name}`];
    if (!model || !Array.isArray(model.elements) || model.elements.length === 0) continue;
    const fullCube = model.elements.some((el) => Array.isArray(el.from) && Array.isArray(el.to)
      && el.from.every((v) => v === 0) && el.to.every((v) => v === 16));
    if (fullCube) opaque.add(`minecraft:${name}`);
  }

  const atlasSize = upperPowerOfTwo(Math.max(atlasImage.width, atlasImage.height));
  const atlasCanvas = document.createElement('canvas');
  atlasCanvas.width = atlasSize;
  atlasCanvas.height = atlasSize;
  const ctx = atlasCanvas.getContext('2d');
  ctx.drawImage(atlasImage, 0, 0);
  const atlasData = ctx.getImageData(0, 0, atlasSize, atlasSize);
  const idMap = {};
  for (const id of Object.keys(uvMap)) {
    const [u, v, du, dv] = uvMap[id];
    // Animated textures are vertical frame strips; UVs point at the first square frame.
    const dv2 = (du !== dv && id.startsWith('block/')) ? du : dv;
    idMap[Identifier.create(id).toString()] = [u / atlasSize, v / atlasSize, (u + du) / atlasSize, (v + dv2) / atlasSize];
  }
  const textureAtlas = new TextureAtlas(atlasData, idMap);
  const flags = new Map(); // block id -> flags (a function of the id)

  return {
    resources: {
      getBlockDefinition(id) { return blockDefinitions[id.toString()]; },
      getBlockModel(id) { return blockModels[id.toString()]; },
      getTextureUV(id) { return textureAtlas.getTextureUV(id); },
      getTextureAtlas() { return textureAtlas.getTextureAtlas(); },
      getPixelSize() { return textureAtlas.getPixelSize(); },
      getBlockFlags(id) {
        const s = id.toString();
        let entry = flags.get(s);
        if (!entry) {
          entry = {
            opaque: opaque.has(s),
            self_culling: SELF_CULLING.test(s),
            semi_transparent: SEMI_TRANSPARENT.test(s),
          };
          flags.set(s, entry);
        }
        return entry;
      },
      getBlockProperties(id) {
        const entry = blocks[id.path];
        return entry ? entry[0] : null;
      },
      getDefaultBlockProperties(id) {
        const entry = blocks[id.path];
        return entry ? entry[1] : null;
      },
    },
  };
}

// Alpha-aware mip chain for the atlas. A plain box filter averages cutout
// textures (leaves, plants) with their transparent texels, washing colors out
// and turning foliage translucent at distance. Instead: a mip texel is solid
// when at least half its source texels are, its color is the alpha-weighted
// average of the solid texels, and mixed-alpha edges collapse to fully
// transparent. Tiles are 16px-aligned in the atlas, so levels 1..4 never blend
// across tile boundaries.
function halveRGBA(src) {
  const w = src.width >> 1;
  const h = src.height >> 1;
  const out = new Uint8ClampedArray(w * h * 4);
  const sd = src.data;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i00 = ((y * 2) * src.width + x * 2) * 4;
      const i01 = i00 + 4;
      const i10 = i00 + src.width * 4;
      const i11 = i10 + 4;
      let r = 0, g = 0, b = 0, aSum = 0, solid = 0;
      let fr = 0, fg = 0, fb = 0, fn = 0;
      for (const i of [i00, i01, i10, i11]) {
        const a = sd[i + 3];
        if (a >= 128) {
          solid++;
          r += sd[i] * a; g += sd[i + 1] * a; b += sd[i + 2] * a;
          aSum += a;
        }
        if (a > 0) {
          fr += sd[i]; fg += sd[i + 1]; fb += sd[i + 2]; fn++;
        }
      }
      const o = (y * w + x) * 4;
      if (solid >= 2) {
        out[o] = r / aSum; out[o + 1] = g / aSum; out[o + 2] = b / aSum;
        out[o + 3] = aSum / solid;
      } else if (fn > 0) {
        // Keep a plausible color under alpha 0 so inter-level blending
        // doesn't pull edges toward black.
        out[o] = fr / fn; out[o + 1] = fg / fn; out[o + 2] = fb / fn;
        out[o + 3] = 0;
      }
    }
  }
  return { data: out, width: w, height: h };
}

function buildMipLevels(imageData, count) {
  const levels = [];
  let src = { data: imageData.data, width: imageData.width, height: imageData.height };
  for (let level = 0; level < count; level++) {
    src = halveRGBA(src);
    levels.push(src);
  }
  return levels;
}

function compileProgram(gl, vsSource, fsSource) {
  const compile = (type, source) => {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(`Shader compile failed: ${gl.getShaderInfoLog(shader)}`);
    }
    return shader;
  };
  const program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER, vsSource));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, fsSource));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`Program link failed: ${gl.getProgramInfoLog(program)}`);
  }
  return program;
}

const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));

// A 16^3 chunk's coordinates packed in one number, and a block's index in
// its chunk.
function cellKey(cx, cy, cz) {
  if (Math.abs(cx) >= 0x8000 || Math.abs(cy) >= 0x8000 || Math.abs(cz) >= 0x8000) throw new Error(`Chunk ${cx},${cy},${cz} is out of the viewer's range`);
  return ((cx + 0x8000) * 0x10000 + (cy + 0x8000)) * 0x10000 + (cz + 0x8000);
}
const localIndex = (x, y, z) => ((y & 15) << 8) | ((z & 15) << 4) | (x & 15);

// Headless renderer behind see_build: server/shot.js drives it through
// web/shot.html (reset -> applyDelta -> flush -> setCamera -> renderNow).
export async function createViewer(canvas) {
  const { resources } = await loadResources();
  const gl = canvas.getContext('webgl2', { antialias: true, powerPreference: 'high-performance' });
  if (!gl) throw new Error('WebGL2 is not available');

  // Swap deepslate's chunk shaders for ours (sun + ambient lighting, fog).
  let replacedVS = false;
  let replacedFS = false;
  const shaderSource = gl.shaderSource.bind(gl);
  gl.shaderSource = (shader, source) => {
    if (source.includes(DEEPSLATE_VS_MARKER)) { replacedVS = true; source = CHUNK_VS; }
    else if (source.includes(DEEPSLATE_FS_MARKER)) { replacedFS = true; source = CHUNK_FS; }
    shaderSource(shader, source);
  };

  // Deepslate mipmaps the atlas with the GPU's box filter, which butchers
  // cutout textures. Upload our alpha-aware mip chain instead, capped at
  // level 4 (one texel per 16px tile) so filtering never crosses tiles.
  // NEAREST within a level keeps the pixel-art look; LINEAR between levels
  // removes shimmer at distance.
  gl.generateMipmap = (target) => {
    const mips = buildMipLevels(resources.getTextureAtlas(), 4);
    mips.forEach((img, i) => {
      gl.texImage2D(target, i + 1, gl.RGBA, img.width, img.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, img.data);
    });
    gl.texParameteri(target, gl.TEXTURE_MAX_LEVEL, 4);
    gl.texParameteri(target, gl.TEXTURE_MIN_FILTER, gl.NEAREST_MIPMAP_LINEAR);
  };

  const CHUNK = 16;

  // Sparse StructureProvider over world coordinates (no origin shift, so
  // chunks stay stable as the build grows). The viewer meshes chunks itself
  // (meshChunk), reading neighbours from `cells`.
  class SparseWorld {
    constructor() {
      this.cells = new Map(); // cellKey -> Array(4096) of placed blocks by local index
      this.chunks = new Map(); // 'cx,cy,cz' -> Map(local index -> placed block), in the order set
      this.emitters = new Map(); // posKey -> light level 1..15, in the order set
      this.size = 0;
      // The chunk put() and remove() looked at last, and the one at() did
      // (neither arrays nor maps are replaced until clear()).
      this.slotX = NaN;
      this.slotY = NaN;
      this.slotZ = NaN;
      this.slotBlocks = null;
      this.slotChunk = null;
      this.lastX = NaN;
      this.lastY = NaN;
      this.lastZ = NaN;
      this.lastBlocks = null;
    }

    static chunkKey(x, y, z) {
      return `${Math.floor(x / CHUNK)},${Math.floor(y / CHUNK)},${Math.floor(z / CHUNK)}`;
    }

    // Lets every block go, with the chunks' arrays and maps.
    clear() {
      this.cells.clear();
      this.chunks.clear();
      this.emitters.clear();
      this.size = 0;
      this.slotX = this.slotY = this.slotZ = NaN;
      this.slotBlocks = this.slotChunk = null;
      this.lastX = this.lastY = this.lastZ = NaN;
      this.lastBlocks = null;
    }

    // Points the slot at a block's chunk, made when `make` is set; false
    // when it has none.
    slot(x, y, z, make) {
      const cx = x >> 4, cy = y >> 4, cz = z >> 4;
      if (cx === this.slotX && cy === this.slotY && cz === this.slotZ) return true;
      const cell = cellKey(cx, cy, cz);
      let blocks = this.cells.get(cell);
      if (!blocks) {
        if (!make) return false;
        this.cells.set(cell, blocks = new Array(4096));
      }
      const key = `${cx},${cy},${cz}`;
      let chunk = this.chunks.get(key);
      if (!chunk) this.chunks.set(key, chunk = new Map());
      this.slotX = cx; this.slotY = cy; this.slotZ = cz;
      this.slotBlocks = blocks;
      this.slotChunk = chunk;
      return true;
    }

    set(x, y, z, state, opaque, lum, scale) {
      const placed = { pos: [x, y, z], state, nbt: undefined, opaque, lum, scale };
      this.put(placed);
      return placed;
    }

    put(placed) {
      const [x, y, z] = placed.pos;
      this.slot(x, y, z, true);
      const index = localIndex(x, y, z);
      const old = this.slotBlocks[index];
      if (!old) this.size++;
      this.slotBlocks[index] = placed;
      this.slotChunk.set(index, placed);
      if (placed.lum > 0) this.emitters.set(`${x},${y},${z}`, placed.lum);
      else if (old && old.lum > 0) this.emitters.delete(`${x},${y},${z}`);
    }

    remove(x, y, z) {
      if (!this.slot(x, y, z, false)) return;
      const index = localIndex(x, y, z);
      const old = this.slotBlocks[index];
      if (!old) return;
      this.slotBlocks[index] = undefined;
      this.size--;
      this.slotChunk.delete(index);
      if (old.lum > 0) this.emitters.delete(`${x},${y},${z}`);
    }

    forEachBlock(fn) {
      for (const chunk of this.chunks.values()) for (const placed of chunk.values()) fn(placed);
    }

    getBlocks() {
      const blocks = [];
      this.forEachBlock((placed) => blocks.push(placed));
      return blocks;
    }

    // The block at a position. Consecutive lookups mostly fall in one
    // chunk, whose array is remembered.
    at(x, y, z) {
      const cx = x >> 4, cy = y >> 4, cz = z >> 4;
      if (cx !== this.lastX || cy !== this.lastY || cz !== this.lastZ) {
        const blocks = this.cells.get(cellKey(cx, cy, cz));
        if (!blocks) return undefined;
        this.lastX = cx; this.lastY = cy; this.lastZ = cz;
        this.lastBlocks = blocks;
      }
      return this.lastBlocks[localIndex(x, y, z)];
    }

    getSize() {
      return [0, 0, 0]; // only used for the grid mesh, which we never draw
    }
  }

  const world = new SparseWorld();
  const renderer = new StructureRenderer(gl, world, resources, { chunkSize: CHUNK, useInvisibleBlockBuffer: false });
  if (!replacedVS || !replacedFS) throw new Error('Failed to patch deepslate chunk shaders');

  // ---- light engine ----
  // Voxel light field over the build's bounding box (+1 air margin), BlueMap /
  // vanilla style: skylight pours down open columns at level 15 and spreads
  // with -1 per step; block light BFS-spreads from emitters. Both live in one
  // grid as max(sky, block).
  let light = null; // { minX, minY, minZ, sx, sy, sz, lvl: Uint8Array }
  let lightPending = false;

  const cellLight = (x, y, z) => lightAt(light, x, y, z);

  // Light changes ripple up to 15 blocks from an edit. Rebuild once per
  // flush: each chunk whose light changed takes a new light stamp (part of
  // what a mesh is made from, meshKey), and the chunks around it, whose
  // blocks read light across their faces, edges and corners, are meshed again.
  let lightStamp = 0;
  const lightStamps = new Map(); // viewer chunk -> the stamp of its last light change
  function rebuildLight() {
    lightPending = false;
    const before = light;
    light = computeLight(world);
    const changed = changedLightChunks(before, light);
    if (changed.size === 0) return;
    lightStamp += 1;
    for (const cell of changed) {
      lightStamps.set(cell, lightStamp);
      for (const near of around(cell)) if (world.chunks.has(near)) meshDirty.add(near);
    }
  }

  // ---- chunk meshing, with AO + light baked in (BlueMap-style) ----
  // The chunk mesher makes the buffers deepslate's ChunkBuilder made (its
  // updateStructureBuffers, finishChunkMesh and Mesh.rebuild, with the
  // viewer's proxy scale and shading on each block's quads), value for
  // value and in the same order, writing typed arrays instead of quad
  // objects. A block's quads before they are placed depend only on its
  // state and which of its faces are culled, so deepslate makes them once
  // for each (a template).
  function isOccluder(x, y, z) {
    return world.at(x, y, z)?.opaque === true;
  }

  const chunkBuilder = renderer.chunkBuilder;
  const preparedChunks = new WeakSet();
  const getChunk = chunkBuilder.getChunk.bind(chunkBuilder);
  chunkBuilder.getChunk = (position) => {
    const chunk = getChunk(position);
    if (!preparedChunks.has(chunk)) {
      retainGpuMesh(chunk.mesh);
      retainGpuMesh(chunk.transparentMesh);
      preparedChunks.add(chunk);
    }
    return chunk;
  };

  // By block id: its flags and whether deepslate counts it waterlogged
  // whatever its properties (fluids, kelp, seagrass). Each state object
  // keeps its id's (a state's name never changes).
  const idInfo = new Map();
  const INFO = Symbol('mesher info');
  function infoOf(state) {
    let info = state[INFO];
    if (info) return info;
    const name = state.getName();
    const id = name.toString();
    info = idInfo.get(id);
    if (!info) {
      info = { id, name, flags: resources.getBlockFlags(name), fluid: new BlockState(name, {}).isWaterlogged() };
      idInfo.set(id, info);
    }
    state[INFO] = info;
    return info;
  }
  // BlockState.isWaterlogged, reading the properties as they are now.
  const waterlogged = (state, info) => info.fluid || state.getProperties().waterlogged === 'true';

  // By block state object: its templates, by the bits of its culled faces.
  // deepslate merges a block's default properties into its state's own
  // properties when it meshes the block; so does the mesher, the first time.
  const stateTemplates = new WeakMap();
  const templatesByKey = new Map();
  function templatesOf(state, info) {
    let templates = stateTemplates.get(state);
    if (!templates) {
      const props = state.getProperties();
      const defaults = resources.getDefaultBlockProperties(info.name) ?? {};
      for (const [k, v] of Object.entries(defaults)) if (!props[k]) props[k] = v;
      const key = `${info.id}${JSON.stringify(props)}`;
      templates = templatesByKey.get(key);
      if (!templates) templatesByKey.set(key, templates = new Array(64));
      stateTemplates.set(state, templates);
    }
    return templates;
  }

  const FAILED = { quads: 0 };
  // A block's quads as deepslate's BlockDefinition and SpecialRenderers make
  // them: per vertex its position, color, texture coordinates and limits.
  function makeTemplate(state, info, cull) {
    let mesh;
    try {
      mesh = new Mesh();
      const definition = resources.getBlockDefinition(info.name);
      if (definition) mesh.merge(definition.getMesh(info.name, state.getProperties(), resources, resources, cull));
      const special = SpecialRenderers.getBlockMesh(state, undefined, resources, cull);
      if (!special.isEmpty()) mesh.merge(special);
    } catch (e) {
      console.error(`Error rendering block ${info.name}`, e);
      return FAILED;
    }
    if (mesh.lines.length > 0) throw new Error(`Block ${info.id} has lines, which the chunk mesher does not draw`);
    const count = mesh.quads.length;
    const pos = new Float64Array(count * 12);
    const color = new Float64Array(count * 12);
    const texture = new Float64Array(count * 8);
    const limit = new Float64Array(count * 16);
    mesh.quads.forEach((quad, q) => {
      quad.vertices().forEach((v, i) => {
        const at = q * 4 + i;
        if (!v.color || !v.texture || !v.textureLimit) throw new Error('Missing vertex component');
        pos[at * 3] = v.pos.x; pos[at * 3 + 1] = v.pos.y; pos[at * 3 + 2] = v.pos.z;
        color[at * 3] = v.color[0]; color[at * 3 + 1] = v.color[1]; color[at * 3 + 2] = v.color[2];
        texture[at * 2] = v.texture[0]; texture[at * 2 + 1] = v.texture[1];
        for (let k = 0; k < 4; k++) limit[at * 4 + k] = v.textureLimit[k];
      });
    });
    return { quads: count, pos, color, texture, limit, transparent: Boolean(resources.getBlockFlags(info.name)?.semi_transparent) };
  }

  // The vertex arrays of one chunk mesh being made, grown as needed.
  function makeSink() {
    return { count: 0, capacity: 0, pos: null, color: null, texture: null, limit: null, normal: null };
  }
  function reserve(sink, quads) {
    const need = sink.count + quads;
    if (need <= sink.capacity) return;
    const capacity = Math.max(need, sink.capacity * 2, 1024);
    for (const [field, per] of [['pos', 12], ['color', 12], ['texture', 8], ['limit', 16], ['normal', 12]]) {
      const next = new Float32Array(capacity * per);
      if (sink[field]) next.set(sink[field].subarray(0, sink.count * per));
      sink[field] = next;
    }
    sink.capacity = capacity;
  }
  const opaqueSink = makeSink();
  const transparentSink = makeSink();
  let indices = new Uint16Array(0);

  // Vertex.transform's scratch vector and the translation of
  // finishChunkMesh (gl-matrix stores both as 32-bit floats).
  const VEC = new Float32Array(3);
  const translation = mat4.create();
  const vx = new Float64Array(4);
  const vy = new Float64Array(4);
  const vz = new Float64Array(4);
  const vertexPos = [0, 0, 0];
  const base = [0, 0, 0];
  const c1 = [0, 0, 0];
  const c2 = [0, 0, 0];
  const cc = [0, 0, 0];

  // Appends one block's quads, placed (finishChunkMesh), scaled (a render-LOD
  // proxy) and shaded (AO and light) as the viewer's finishChunkMesh hook did.
  function emitBlock(placed, template) {
    const [px, py, pz] = placed.pos;
    const sink = template.transparent ? transparentSink : opaqueSink;
    reserve(sink, template.quads);
    mat4.identity(translation);
    mat4.translate(translation, translation, placed.pos);
    const m = translation;
    const scale = placed.scale;
    const emissive = placed.lum > 0;
    const tp = template.pos;
    const tc = template.color;
    for (let q = 0; q < template.quads; q++) {
      for (let i = 0; i < 4; i++) {
        const t = (q * 4 + i) * 3;
        VEC[0] = tp[t]; VEC[1] = tp[t + 1]; VEC[2] = tp[t + 2];
        const x = VEC[0], y = VEC[1], z = VEC[2];
        let w = m[3] * x + m[7] * y + m[11] * z + m[15];
        w = w || 1.0;
        VEC[0] = (m[0] * x + m[4] * y + m[8] * z + m[12]) / w;
        VEC[1] = (m[1] * x + m[5] * y + m[9] * z + m[13]) / w;
        VEC[2] = (m[2] * x + m[6] * y + m[10] * z + m[14]) / w;
        vx[i] = VEC[0]; vy[i] = VEC[1]; vz[i] = VEC[2];
      }
      // Quad.normal: (v2 - v1) x (v3 - v1), normalized.
      const e1x = vx[1] - vx[0], e1y = vy[1] - vy[0], e1z = vz[1] - vz[0];
      const e2x = vx[2] - vx[0], e2y = vy[2] - vy[0], e2z = vz[2] - vz[0];
      let nx = e1y * e2z - e1z * e2y;
      let ny = e1z * e2x - e1x * e2z;
      let nz = e1x * e2y - e1y * e2x;
      if (!(nx == 0 && ny == 0 && nz == 0)) {
        const r = 1 / Math.sqrt(nx * nx + ny * ny + nz * nz);
        nx *= r; ny *= r; nz *= r;
      }
      if (scale) {
        for (let i = 0; i < 4; i++) {
          vx[i] = px + (vx[i] - px) * scale[0];
          vy[i] = py + (vy[i] - py) * scale[1];
          vz[i] = pz + (vz[i] - pz) * scale[2];
        }
      }
      const out = sink.count++;
      const p12 = out * 12;
      for (let i = 0; i < 4; i++) {
        sink.pos[p12 + i * 3] = vx[i]; sink.pos[p12 + i * 3 + 1] = vy[i]; sink.pos[p12 + i * 3 + 2] = vz[i];
        sink.normal[p12 + i * 3] = nx; sink.normal[p12 + i * 3 + 1] = ny; sink.normal[p12 + i * 3 + 2] = nz;
      }
      sink.texture.set(template.texture.subarray(q * 8, q * 8 + 8), out * 8);
      sink.limit.set(template.limit.subarray(q * 16, q * 16 + 16), out * 16);

      // Shading: the face's light from the cell it radiates into (its own
      // cell for interior quads like flames), with vertex AO and smooth light
      // for axis-aligned faces on a cell boundary.
      const cx = (vx[0] + vx[1] + vx[2] + vx[3]) / 4;
      const cy = (vy[0] + vy[1] + vy[2] + vy[3]) / 4;
      const cz = (vz[0] + vz[1] + vz[2] + vz[3]) / 4;
      const fx = Math.floor(cx + nx * 0.5);
      const fy = Math.floor(cy + ny * 0.5);
      const fz = Math.floor(cz + nz * 0.5);
      const faceLevel = cellLight(fx, fy, fz);
      const faceLight = emissive ? 1.15 : lightCurve(faceLevel);
      let axis = -1;
      if (Math.abs(nx) > 0.99) axis = 0;
      else if (Math.abs(ny) > 0.99) axis = 1;
      else if (Math.abs(nz) > 0.99) axis = 2;
      const centerAxis = axis === 0 ? cx : axis === 1 ? cy : cz;
      const onBoundary = axis >= 0 && Math.abs(centerAxis - Math.round(centerAxis)) < 1e-3;
      if (!onBoundary) {
        for (let i = 0; i < 4; i++) {
          const t = (q * 4 + i) * 3;
          sink.color[p12 + i * 3] = tc[t] * faceLight;
          sink.color[p12 + i * 3 + 1] = tc[t + 1] * faceLight;
          sink.color[p12 + i * 3 + 2] = tc[t + 2] * faceLight;
        }
        continue;
      }
      const a1 = axis === 0 ? 1 : 0;
      const a2 = axis === 2 ? 1 : 2;
      base[0] = fx; base[1] = fy; base[2] = fz;
      for (let i = 0; i < 4; i++) {
        vertexPos[0] = vx[i]; vertexPos[1] = vy[i]; vertexPos[2] = vz[i];
        const s1 = vertexPos[a1] > base[a1] + 0.5 ? 1 : -1;
        const s2 = vertexPos[a2] > base[a2] + 0.5 ? 1 : -1;
        c1[0] = fx; c1[1] = fy; c1[2] = fz; c1[a1] += s1;
        c2[0] = fx; c2[1] = fy; c2[2] = fz; c2[a2] += s2;
        cc[0] = fx; cc[1] = fy; cc[2] = fz; cc[a1] += s1; cc[a2] += s2;
        const side1 = isOccluder(c1[0], c1[1], c1[2]);
        const side2 = isOccluder(c2[0], c2[1], c2[2]);
        const corner = isOccluder(cc[0], cc[1], cc[2]);
        const occ = side1 && side2 ? 3 : (side1 ? 1 : 0) + (side2 ? 1 : 0) + (corner ? 1 : 0);
        const ao = AO_CURVE[occ];
        let vLight;
        if (emissive) {
          vLight = 1.15;
        } else {
          let sum = faceLevel;
          let cnt = 1;
          if (!side1) { sum += cellLight(c1[0], c1[1], c1[2]); cnt++; }
          if (!side2) { sum += cellLight(c2[0], c2[1], c2[2]); cnt++; }
          if (!corner) { sum += cellLight(cc[0], cc[1], cc[2]); cnt++; }
          vLight = lightCurve(sum / cnt);
        }
        const s = ao * vLight;
        const t = (q * 4 + i) * 3;
        sink.color[p12 + i * 3] = tc[t] * s;
        sink.color[p12 + i * 3 + 1] = tc[t + 1] * s;
        sink.color[p12 + i * 3 + 2] = tc[t + 2] * s;
      }
    }
  }

  // Cull flags of ChunkBuilder.needsCull, as bits: up, down, west, east,
  // north, south.
  const SIDES = [[0, 1, 0], [0, -1, 0], [-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]];
  function cullBits(placed, info) {
    const [x, y, z] = placed.pos;
    let bits = 0;
    for (let side = 0; side < 6; side++) {
      const [dx, dy, dz] = SIDES[side];
      const neighbor = world.at(x + dx, y + dy, z + dz);
      if (!neighbor) continue;
      const other = infoOf(neighbor.state);
      let cull;
      if (info.id === other.id && other.flags.self_culling) cull = true;
      else if (other.flags.opaque) cull = !(side === 0 && waterlogged(placed.state, info));
      else cull = waterlogged(placed.state, info) && waterlogged(neighbor.state, other);
      if (cull) bits |= 1 << side;
    }
    return bits;
  }
  const cullOf = (bits) => ({
    up: (bits & 1) !== 0,
    down: (bits & 2) !== 0,
    west: (bits & 4) !== 0,
    east: (bits & 8) !== 0,
    north: (bits & 16) !== 0,
    south: (bits & 32) !== 0,
  });

  // Mesh.rebuild of the chunk mesh from a sink (no lines). Block positions
  // are left out: only deepslate's drawColoredStructure, which the viewer
  // never calls, reads them.
  function upload(mesh, sink) {
    const count = sink.count;
    const write = (field, target, data) => {
      if (count === 0) {
        if (mesh[field]) gl.deleteBuffer(mesh[field]);
        mesh[field] = undefined;
        return;
      }
      if (!mesh[field]) mesh[field] = gl.createBuffer() ?? undefined;
      if (!mesh[field]) throw new Error('Cannot create new buffer');
      gl.bindBuffer(target, mesh[field]);
      gl.bufferData(target, data, gl.DYNAMIC_DRAW);
    };
    write('posBuffer', gl.ARRAY_BUFFER, sink.pos?.subarray(0, count * 12));
    for (const field of ['linePosBuffer', 'lineColorBuffer']) {
      if (mesh[field]) gl.deleteBuffer(mesh[field]);
      mesh[field] = undefined;
    }
    write('colorBuffer', gl.ARRAY_BUFFER, sink.color?.subarray(0, count * 12));
    write('textureBuffer', gl.ARRAY_BUFFER, sink.texture?.subarray(0, count * 8));
    write('textureLimitBuffer', gl.ARRAY_BUFFER, sink.limit?.subarray(0, count * 16));
    write('normalBuffer', gl.ARRAY_BUFFER, sink.normal?.subarray(0, count * 12));
    if (indices.length < count * 6) {
      // Mesh.rebuild's indices, 16-bit as deepslate draws them.
      indices = new Uint16Array(Math.max(count * 6, indices.length * 2));
      for (let i = 0; i < indices.length / 6; i++) {
        indices[i * 6] = 4 * i;
        indices[i * 6 + 1] = 4 * i + 1;
        indices[i * 6 + 2] = 4 * i + 2;
        indices[i * 6 + 3] = i * 4;
        indices[i * 6 + 4] = 4 * i + 2;
        indices[i * 6 + 5] = 4 * i + 3;
      }
    }
    write('indexBuffer', gl.ELEMENT_ARRAY_BUFFER, indices.subarray(0, count * 6));
    mesh.uploaded(count);
  }

  // ChunkBuilder.updateStructureBuffers for one chunk, its blocks in the
  // order they were set.
  function meshChunk(position, blocks) {
    const chunk = chunkBuilder.getChunk(position);
    chunk.mesh.clear();
    chunk.transparentMesh.clear();
    opaqueSink.count = 0;
    transparentSink.count = 0;
    if (blocks) {
      for (const placed of blocks.values()) {
        const info = infoOf(placed.state);
        const templates = templatesOf(placed.state, info);
        const bits = cullBits(placed, info);
        const template = templates[bits] ?? (templates[bits] = makeTemplate(placed.state, info, cullOf(bits)));
        if (template.quads > 0) emitBlock(placed, template);
      }
    }
    upload(chunk.mesh, opaqueSink);
    upload(chunk.transparentMesh, transparentSink);
  }

  // The lens of the shot (server/shot.js): a perspective field of view, or
  // an orthographic box in view space. Far plane pushed out (deepslate's
  // default 500 clips big builds); aspect follows the canvas.
  let lens = { fov: 70 };
  const perspective = (fov) => {
    const proj = mat4.create();
    mat4.perspective(proj, fov * Math.PI / 180, canvas.width / canvas.height, 0.1, MAX_CAMERA_DISTANCE);
    return proj;
  };
  renderer.getPerspective = () => {
    if (!lens.ortho) return perspective(lens.fov);
    const { halfWidth, halfHeight, near, far } = lens.ortho;
    const proj = mat4.create();
    mat4.ortho(proj, -halfWidth, halfWidth, -halfHeight, halfHeight, near, far);
    return proj;
  };

  // Fog uniforms on deepslate's chunk program (our fragment shader).
  const fogColorLoc = gl.getUniformLocation(renderer.shaderProgram, 'fogColor');
  const fogRangeLoc = gl.getUniformLocation(renderer.shaderProgram, 'fogRange');
  const logDepthLoc = gl.getUniformLocation(renderer.shaderProgram, 'logDepth');
  const linearDepthLoc = gl.getUniformLocation(renderer.shaderProgram, 'linearDepth');
  function syncFog(size) {
    const start = clamp(size * 3, 90, MAX_CAMERA_DISTANCE * 0.6);
    const end = Math.min(start * 1.9, MAX_CAMERA_DISTANCE * 0.95);
    gl.useProgram(renderer.shaderProgram);
    gl.uniform3f(fogColorLoc, FOG_COLOR[0], FOG_COLOR[1], FOG_COLOR[2]);
    gl.uniform2f(fogRangeLoc, start, end);
  }
  syncFog(32);

  // GPU sky (gradient + sun) on its own VAO so deepslate's attribute state on
  // the default VAO is never disturbed.
  const skyProgram = compileProgram(gl, SKY_VS, SKY_FS);
  const skyUnprojectLoc = gl.getUniformLocation(skyProgram, 'unproject');
  const skyVao = gl.createVertexArray();
  gl.bindVertexArray(skyVao);
  const skyBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, skyBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const skyPosLoc = gl.getAttribLocation(skyProgram, 'pos');
  gl.enableVertexAttribArray(skyPosLoc);
  gl.vertexAttribPointer(skyPosLoc, 2, gl.FLOAT, false, 0, 0);
  gl.bindVertexArray(null);

  // ---- camera: orbit around `focus` at `focusDist` ----
  let yaw = 2.4;
  let pitch = 0.5;
  let focus = [0.5, 0.5, 0.5];
  let focusDist = 24;

  function forwardVec() {
    const cp = Math.cos(pitch);
    return [cp * Math.sin(yaw), -Math.sin(pitch), -cp * Math.cos(yaw)];
  }

  let syncChunks = new Map(); // server 16^3 chunk key -> the blocks it set
  const meshDirty = new Set(); // chunk keys awaiting remesh

  function drawSky() {
    const rot = mat4.create();
    mat4.rotateX(rot, rot, pitch);
    mat4.rotateY(rot, rot, yaw);
    // The sky is a direction lookup, so an orthographic shot takes it from
    // a perspective lens.
    const unproject = mat4.create();
    mat4.multiply(unproject, lens.ortho ? perspective(70) : renderer.projMatrix, rot);
    mat4.invert(unproject, unproject);
    gl.useProgram(skyProgram);
    gl.uniformMatrix4fv(skyUnprojectLoc, false, unproject);
    gl.disable(gl.DEPTH_TEST);
    gl.depthMask(false);
    gl.bindVertexArray(skyVao);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.bindVertexArray(null);
    gl.depthMask(true);
    gl.enable(gl.DEPTH_TEST);
  }

  function draw() {
    renderer.setViewport(0, 0, canvas.width, canvas.height);
    renderer.projMatrix = renderer.getPerspective();
    gl.useProgram(renderer.shaderProgram);
    gl.uniform1f(logDepthLoc, LOG_DEPTH_SCALE);
    gl.uniform1f(linearDepthLoc, lens.ortho ? 1 : 0);
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
    drawSky();
    const f = forwardVec();
    const eye = [focus[0] - f[0] * focusDist, focus[1] - f[1] * focusDist, focus[2] - f[2] * focusDist];
    const view = mat4.create();
    mat4.rotateX(view, view, pitch);
    mat4.rotateY(view, view, yaw);
    mat4.translate(view, view, [-eye[0], -eye[1], -eye[2]]);
    renderer.drawStructure(view);
  }

  // Marks a changed block's chunk dirty, plus the chunks across the faces,
  // edges and corners of it the block touches (their culling and ambient
  // occlusion read it). The chunk marked last and the ones marked around it
  // (bits of `dirtyNear`, by AROUND's index) are remembered until flush()
  // takes chunks out.
  let dirtyX = NaN, dirtyY = NaN, dirtyZ = NaN, dirtyNear = 0;
  function markDirty(x, y, z) {
    const cx = x >> 4, cy = y >> 4, cz = z >> 4;
    if (cx !== dirtyX || cy !== dirtyY || cz !== dirtyZ) {
      meshDirty.add(`${cx},${cy},${cz}`);
      dirtyX = cx; dirtyY = cy; dirtyZ = cz;
      dirtyNear = 1 << 13;
    }
    const lx = x & 15, ly = y & 15, lz = z & 15;
    const x0 = lx === 0 ? -1 : 0, x1 = lx === 15 ? 1 : 0;
    const y0 = ly === 0 ? -1 : 0, y1 = ly === 15 ? 1 : 0;
    const z0 = lz === 0 ? -1 : 0, z1 = lz === 15 ? 1 : 0;
    if (x0 === x1 && y0 === y1 && z0 === z1) return;
    for (let dx = x0; dx <= x1; dx++) for (let dy = y0; dy <= y1; dy++) for (let dz = z0; dz <= z1; dz++) {
      const bit = 1 << ((dx + 1) * 9 + (dy + 1) * 3 + (dz + 1));
      if (dirtyNear & bit) continue;
      dirtyNear |= bit;
      meshDirty.add(`${cx + dx},${cy + dy},${cz + dz}`);
    }
  }

  function preparePalette(entries) {
    return entries.map((entry) => {
      const path = entry.id.replace(/^minecraft:/, '');
      const states = entry.states || {};
      const props = { ...(resources.getDefaultBlockProperties({ path }) || {}), ...states };
      return {
        state: new BlockState(path, states),
        meta: {
          opaque: resources.getBlockFlags({ toString: () => `minecraft:${path}` }).opaque,
          lum: luminanceOf(path, props),
        },
      };
    });
  }

  // Lets the whole world go: its blocks, every chunk's meshes with their GPU
  // buffers, the light, and all that was kept by chunk. What comes next is
  // built on nothing, as on a page just loaded.
  function reset() {
    for (const versions of stash.values()) for (const entry of versions.values()) freeEntry(entry);
    // deepslate's chunks, by position. A stashed mesh a chunk still drew
    // (restored) was freed above: freeing it again finds nothing.
    chunkBuilder.chunks.forEach((plane) => plane.forEach((row) => row.forEach((chunk) => freeEntry(chunk))));
    chunkBuilder.chunks = [];
    world.clear();
    syncChunks = new Map();
    keyCells = new Map();
    cellVersions = new Map();
    built = new Map();
    stash = new Map();
    meshDirty.clear();
    dirtyX = NaN;
    lightStamps.clear();
    // An empty world's light, with nothing to rebuild.
    light = null;
    lightPending = false;
  }

  // A chunk's mesh depends on its blocks and, through culling, ambient
  // occlusion and light, on the chunks around it: `built` holds, for each
  // chunk meshed, its meshKey when it was, and a dirty chunk whose key is the
  // same is not meshed again. The contents and meshes of chunks removed or
  // replaced are kept by version, so a see_build clip box (which removes the
  // chunks outside it) followed by a shot of the whole world brings the same
  // versions back at no meshing cost.
  let keyCells = new Map(); // server chunk key -> viewer chunk
  let cellVersions = new Map(); // viewer chunk 'cx,cy,cz' -> server version
  let built = new Map(); // viewer chunk -> the meshKey it was meshed with
  let stash = new Map(); // viewer chunk -> Map(version -> { blocks, mesh, transparentMesh, built })
  const STASHED_VERSIONS = 2;

  // The chunk and the 26 around it, by (dx + 1) * 9 + (dy + 1) * 3 + (dz + 1).
  const AROUND = [];
  for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) for (let dz = -1; dz <= 1; dz++) AROUND.push([dx, dy, dz]);
  function around(cell) {
    const [x, y, z] = cell.split(',').map(Number);
    return AROUND.map(([dx, dy, dz]) => `${x + dx},${y + dy},${z + dz}`);
  }

  // What a chunk's mesh is made from: the versions and light stamps of the
  // chunks around it.
  function meshKey(cell) {
    return around(cell).map((near) => `${cellVersions.get(near) ?? '-'}/${lightStamps.get(near) ?? 0}`).join(' ');
  }

  // deepslate's Mesh buffers: deleted with a mesh no chunk draws any more.
  const MESH_BUFFERS = ['posBuffer', 'colorBuffer', 'textureBuffer', 'textureLimitBuffer', 'normalBuffer', 'blockPosBuffer', 'indexBuffer', 'linePosBuffer', 'lineColorBuffer'];
  function freeMesh(mesh) {
    for (const field of MESH_BUFFERS) {
      if (mesh[field]) gl.deleteBuffer(mesh[field]);
      mesh[field] = undefined;
    }
    mesh.clear();
  }
  function freeEntry(entry) {
    freeMesh(entry.mesh);
    freeMesh(entry.transparentMesh);
  }

  // `placed`: the blocks a server chunk set, in the order it set them.
  function stashChunk(cell, placed) {
    const blocks = placed.map(({ pos: [x, y, z] }) => world.at(x, y, z));
    const chunk = chunkBuilder.getChunk(cell.split(',').map(Number));
    const Mesh = chunk.mesh.constructor;
    if (!stash.has(cell)) stash.set(cell, new Map());
    const versions = stash.get(cell);
    const version = cellVersions.get(cell);
    // Re-stashed, a version is the newest again.
    versions.delete(version);
    versions.set(version, { blocks, mesh: chunk.mesh, transparentMesh: chunk.transparentMesh, built: built.get(cell) });
    if (versions.size > STASHED_VERSIONS) {
      const [oldest, entry] = versions.entries().next().value;
      versions.delete(oldest);
      freeEntry(entry);
    }
    chunk.mesh = retainGpuMesh(new Mesh());
    chunk.transparentMesh = retainGpuMesh(new Mesh());
    built.delete(cell);
  }

  function restoreChunk(cell, entry) {
    for (const placed of entry.blocks) {
      world.put(placed);
      markDirty(placed.pos[0], placed.pos[1], placed.pos[2]);
    }
    const chunk = chunkBuilder.getChunk(cell.split(',').map(Number));
    chunk.mesh = entry.mesh;
    chunk.transparentMesh = entry.transparentMesh;
    built.set(cell, entry.built);
    return entry.blocks;
  }

  // Applies complete replacements for changed server chunks. Work is
  // proportional to the changed chunks, never the full world.
  function applyDelta(delta) {
    const replaceKeys = [...(delta.removed || []), ...(delta.chunks || []).map((chunk) => chunk.key)];
    for (const key of replaceKeys) {
      const placed = syncChunks.get(key);
      if (!placed) continue;
      const [first] = placed;
      const cell = first && SparseWorld.chunkKey(...first.pos);
      if (cell) stashChunk(cell, placed);
      for (const { pos: [x, y, z] } of placed) {
        world.remove(x, y, z);
        markDirty(x, y, z);
      }
      syncChunks.delete(key);
    }

    for (const key of replaceKeys) {
      const cell = keyCells.get(key);
      if (cell) cellVersions.delete(cell);
    }
    for (const chunk of delta.chunks || []) {
      const cell = SparseWorld.chunkKey(...chunk.origin);
      keyCells.set(chunk.key, cell);
      cellVersions.set(cell, chunk.version);
      const entry = stash.get(cell)?.get(chunk.version);
      if (entry) {
        syncChunks.set(chunk.key, restoreChunk(cell, entry));
        continue;
      }
      const palette = preparePalette(chunk.palette || []);
      const placed = [];
      for (const [lx, ly, lz, index, sx = 1, sy = 1, sz = 1] of chunk.blocks || []) {
        const x = chunk.origin[0] + lx;
        const y = chunk.origin[1] + ly;
        const z = chunk.origin[2] + lz;
        const { state, meta } = palette[index];
        const scale = sx > 1 || sy > 1 || sz > 1 ? [sx, sy, sz] : undefined;
        placed.push(world.set(x, y, z, state, meta.opaque, meta.lum, scale));
        markDirty(x, y, z);
      }
      syncChunks.set(chunk.key, placed);
    }

    if (delta.bounds) {
      const { min, max } = delta.bounds;
      syncFog(Math.max(max[0] - min[0], max[1] - min[1], max[2] - min[2]) + 1);
    }
    if (replaceKeys.length > 0) lightPending = true;
  }

  // Screenshots need a COMPLETE world: rebuild light and mesh every dirty
  // chunk synchronously before any frame is captured.
  function flush() {
    if (lightPending) rebuildLight();
    dirtyX = NaN;
    for (const ck of [...meshDirty]) {
      meshDirty.delete(ck);
      const key = meshKey(ck);
      if (built.get(ck) === key) continue;
      built.set(ck, key);
      meshChunk(ck.split(',').map(Number), world.chunks.get(ck));
    }
  }

  function setCamera(opts) {
    yaw = opts.yaw;
    pitch = opts.pitch;
    focus = opts.focus;
    focusDist = opts.focusDist;
    lens = opts.ortho ? { ortho: opts.ortho } : { fov: opts.fov };
  }

  return {
    reset,
    applyDelta,
    flush,
    setCamera,
    // WebGL buffers are only valid for a screenshot within the same task.
    renderNow: draw,
    pending: () => meshDirty.size + (lightPending ? 1 : 0),
  };
}
