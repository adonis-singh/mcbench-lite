// see_build's shaders (blocks and entities: vs_quad, vs_ordered, fs_quad;
// the sky: vs_sky, fs_sky) in WGSL, drawing six vertices a quad, read
// from a storage buffer: its corners 0, 1, 2, 0, 2, 3 (deepslate's indices). The
// normal is the quad's (v1 - v0) x (v2 - v0) and its texture limit the box
// of its four texture coordinates, as deepslate computes both. The one
// change to the page's equations is the depth range: OpenGL's clip z
// (-w..w) is mapped to wgpu's (0..w) after the page's own z.

struct Uniforms {
  view: mat4x4<f32>,
  proj: mat4x4<f32>,
  sky_unproject: mat4x4<f32>,
  fog_color: vec4<f32>,
  // logDepth, linearDepth (1 for an orthographic lens)
  params: vec4<f32>,
  // fog start, end
  fog: vec4<f32>,
};

// Per atlas: the half-texel step the page clamps sampling by (pixelSize),
// per axis in the atlas square's coordinates.
struct AtlasInfo {
  pixel: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;
@group(1) @binding(2) var<uniform> info: AtlasInfo;

const SUN = vec3<f32>(0.4467, 0.8437, 0.2978);

// A colour as the 8-bit target stores it, rounded here (half up) rather
// than by the hardware's conversion, which differs between adapters.
fn to8(c: vec4<f32>) -> vec4<f32> {
  return floor(clamp(c, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0 + 0.5) / 255.0;
}

// The quads (scene.rs Instance, 108 bytes): corners f32 x12, texture
// coordinates f32 x8, the corners' halved shade unorm16 x4, the tint
// unorm16 x4, the overlay unorm8 x4, and each corner's sky and block light
// u8 x8 (16 a level, as Minecraft packs a lightmap coordinate; corner k's
// at bytes 2k and 2k + 1 of words 25 and 26), which the day's shading does
// not read; 27 words each.
const QUAD_WORDS = 27u;

// A corner's sky and block light levels (0 to 15).
fn corner_light(base: u32, k: u32) -> vec2<f32> {
  let word = quads[base + 25u + k / 2u];
  let pair = (word >> (16u * (k % 2u))) & 0xffffu;
  return vec2<f32>(f32(pair & 0xffu), f32(pair >> 8u)) / 16.0;
}
@group(2) @binding(0) var<storage, read> quads: array<u32>;
// The translucent quads' order for the shot's camera (Minecraft's
// translucency sort), as indices into `quads`.
@group(2) @binding(1) var<storage, read> order: array<u32>;

struct VertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) limit: vec4<f32>,
  @location(2) tint: vec3<f32>,
  @location(3) @interpolate(flat) normal: vec3<f32>,
  @location(4) dist: f32,
  @location(5) @interpolate(flat) overlay: vec4<f32>,
  @location(6) view_z: f32,
};

fn word(at: u32) -> f32 {
  return bitcast<f32>(quads[at]);
}

fn corner_of(base: u32, k: u32) -> vec3<f32> {
  return vec3<f32>(word(base + k * 3u), word(base + k * 3u + 1u), word(base + k * 3u + 2u));
}

fn uv_of(base: u32, k: u32) -> vec2<f32> {
  return vec2<f32>(word(base + 12u + k * 2u), word(base + 13u + k * 2u));
}

@vertex
fn vs_quad(@builtin(vertex_index) v: u32) -> VertexOut {
  return quad_vertex(v / 6u, v % 6u);
}

// The translucent quads, six vertices a quad in `order`'s order.
@vertex
fn vs_ordered(@builtin(vertex_index) v: u32) -> VertexOut {
  return quad_vertex(order[v / 6u], v % 6u);
}

fn quad_vertex(q: u32, k: u32) -> VertexOut {
  let base = q * QUAD_WORDS;
  var corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
  let corner = corners[k];
  let c0 = corner_of(base, 0u);
  let c1 = corner_of(base, 1u);
  let c2 = corner_of(base, 2u);
  let c3 = corner_of(base, 3u);
  let t0 = uv_of(base, 0u);
  let t1 = uv_of(base, 1u);
  let t2 = uv_of(base, 2u);
  let t3 = uv_of(base, 3u);
  let shades = vec4<f32>(unpack2x16unorm(quads[base + 20u]), unpack2x16unorm(quads[base + 21u]));
  let tint = vec4<f32>(unpack2x16unorm(quads[base + 22u]), unpack2x16unorm(quads[base + 23u]));
  let overlay = unpack4x8unorm(quads[base + 24u]);
  var pos = c0;
  var uv = t0;
  var shade = shades.x;
  if (corner == 1u) { pos = c1; uv = t1; shade = shades.y; }
  else if (corner == 2u) { pos = c2; uv = t2; shade = shades.z; }
  else if (corner == 3u) { pos = c3; uv = t3; shade = shades.w; }
  var out: VertexOut;
  let view_pos = u.view * vec4<f32>(pos, 1.0);
  var clip = u.proj * view_pos;
  if (u.params.y < 0.5) {
    clip.z = (log2(max(1.0 + clip.w, 1e-6)) * u.params.x - 1.0) * clip.w;
  }
  clip.z = (clip.z + clip.w) * 0.5;
  out.position = clip;
  out.uv = uv;
  out.limit = vec4<f32>(min(min(t0, t1), min(t2, t3)), max(max(t0, t1), max(t2, t3)));
  // The shade is stored halved.
  out.tint = tint.rgb * (shade * 2.0);
  let n = cross(c1 - c0, c2 - c0);
  out.normal = select(n, normalize(n), dot(n, n) > 0.0);
  out.dist = length(view_pos.xyz);
  out.overlay = overlay;
  out.view_z = -view_pos.z;
  return out;
}

@fragment
fn fs_quad(in: VertexOut) -> @location(0) vec4<f32> {
  let px = info.pixel.xy;
  let tex = textureSample(atlas, atlas_sampler, clamp(in.uv, in.limit.xy + 0.5 * px, in.limit.zw - 0.5 * px));
  if (tex.a < 0.05) {
    discard;
  }
  let n = normalize(in.normal);
  let hemi = n.y * 0.5 + 0.5;
  let ambient = mix(vec3<f32>(0.40, 0.42, 0.47), vec3<f32>(0.60, 0.63, 0.68), hemi);
  let sun_diff = max(dot(n, SUN), 0.0);
  let light = ambient + vec3<f32>(1.0, 0.96, 0.88) * sun_diff * 0.5;
  let color = mix(in.overlay.rgb, tex.rgb * in.tint, in.overlay.a) * min(light, vec3<f32>(1.15));
  let fog = smoothstep(u.fog.x, u.fog.y, in.dist);
  return to8(vec4<f32>(mix(color, u.fog_color.rgb, fog), tex.a));
}

// The surface each pixel shows, for coverage masks: its distance along the
// view axis, its normal's axis code, and alpha 1 where a surface is.
@fragment
fn fs_surface(in: VertexOut) -> @location(0) vec4<f32> {
  let px = info.pixel.xy;
  let tex = textureSample(atlas, atlas_sampler, clamp(in.uv, in.limit.xy + 0.5 * px, in.limit.zw - 0.5 * px));
  if (tex.a < 0.05) {
    discard;
  }
  let n = normalize(in.normal);
  var axis = 7.0;
  if (n.x > 0.99) { axis = 1.0; } else if (n.x < -0.99) { axis = 2.0; }
  else if (n.y > 0.99) { axis = 3.0; } else if (n.y < -0.99) { axis = 4.0; }
  else if (n.z > 0.99) { axis = 5.0; } else if (n.z < -0.99) { axis = 6.0; }
  return vec4<f32>(in.view_z, axis, tex.a, 1.0);
}

struct SkyOut {
  @builtin(position) position: vec4<f32>,
  @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
  var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  var out: SkyOut;
  out.ndc = corners[i];
  out.position = vec4<f32>(corners[i], 0.0, 1.0);
  return out;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
  let p = u.sky_unproject * vec4<f32>(in.ndc, 1.0, 1.0);
  let dir = normalize(p.xyz / p.w);
  let zenith = vec3<f32>(0.30, 0.49, 0.79);
  let horizon = vec3<f32>(0.74, 0.84, 0.94);
  let ground = vec3<f32>(0.50, 0.57, 0.66);
  var sky = mix(horizon, zenith, pow(clamp(dir.y, 0.0, 1.0), 0.55));
  // smoothstep(0.0, -0.35, dir.y) of the page, written out: SPIR-V's
  // SmoothStep is undefined for a first edge above the second.
  let below = clamp(dir.y / -0.35, 0.0, 1.0);
  sky = mix(sky, ground, below * below * (3.0 - 2.0 * below));
  let sun_amt = max(dot(dir, SUN), 0.0);
  sky += vec3<f32>(1.0, 0.92, 0.75) * pow(sun_amt, 1200.0) * 1.2;
  sky += vec3<f32>(1.0, 0.88, 0.62) * pow(sun_amt, 8.0) * 0.12;
  return to8(vec4<f32>(sky, 1.0));
}
