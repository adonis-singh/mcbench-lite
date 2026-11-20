// The live view's own shaders, compiled after shader.wgsl in one module:
// the pick (what block and face a pixel shows) and the overlays (boxes'
// fills and outlines, the marked block's cube).

// The pick: the opaque and cutout quads drawn again with a frame's camera
// into one pixel, recording which quad is hit, in integers: its buffer
// (the instance index the draw gives, plus one; 0 where nothing is), its
// index in the buffer, and its normal's axis (1 to 6: +x, -x, +y, -y, +z,
// -z). The block is the quad's own (scene.rs pack_block), looked up on the
// CPU, exact at any coordinate.
struct PickOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) limit: vec4<f32>,
  @location(2) @interpolate(flat) normal: vec3<f32>,
  @location(3) world: vec3<f32>,
  @location(4) @interpolate(flat) quad: vec2<u32>,
};

@vertex
fn vs_pick(@builtin(vertex_index) v: u32, @builtin(instance_index) buffer: u32) -> PickOut {
  let q = v / 6u;
  var corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
  let base = q * QUAD_WORDS;
  let quad = quad_vertex(q, v % 6u);
  var out: PickOut;
  out.position = quad.position;
  out.uv = quad.uv;
  out.limit = quad.limit;
  out.normal = quad.normal;
  out.world = corner_of(base, corners[v % 6u]);
  out.quad = vec2<u32>(buffer, q);
  return out;
}

@fragment
fn fs_pick(in: PickOut) -> @location(0) vec4<u32> {
  let px = info.pixel.xy;
  let tex = textureSample(atlas, atlas_sampler, clamp(in.uv, in.limit.xy + 0.5 * px, in.limit.zw - 0.5 * px));
  if (tex.a < 0.05) {
    discard;
  }
  let n = in.normal;
  let a = abs(n);
  var axis = 1u;
  if (a.x >= a.y && a.x >= a.z) {
    axis = select(2u, 1u, n.x > 0.0);
  } else if (a.y >= a.z) {
    axis = select(4u, 3u, n.y > 0.0);
  } else {
    axis = select(6u, 5u, n.z > 0.0);
  }
  return vec4<u32>(in.quad.x + 1u, in.quad.y, axis, 0u);
}

// BlueMap's terrain height (BlueMap:src/js/map/Map.js:432-455): a ray down
// meets the tile's drawn triangles whatever their texture, front faces
// only (Map.js:371, FrontSide); this records the height it meets them at
// (the page's), with 1 where it meets any.
@fragment
fn fs_height(in: PickOut) -> @location(0) vec2<f32> {
  return vec2<f32>(in.world.y, 1.0);
}

// The overlays: triangles of a colour (a box's fill, the marked cube) and
// lines two pixels wide (a box's edges), in the page's coordinates, depth
// tested against the world with its own depth (shader.wgsl quad_vertex).
// The live view's uniforms carry the drawn size in pixels in params.zw and
// the lines' width in fog.z (the shots leave them 0).

struct OverlayOut {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec4<f32>,
};

fn world_depth(clip_in: vec4<f32>) -> vec4<f32> {
  var clip = clip_in;
  if (u.params.y < 0.5) {
    clip.z = (log2(max(1.0 + clip.w, 1e-6)) * u.params.x - 1.0) * clip.w;
  }
  clip.z = (clip.z + clip.w) * 0.5;
  return clip;
}

@vertex
fn vs_fill(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> OverlayOut {
  var out: OverlayOut;
  out.position = world_depth(u.proj * (u.view * vec4<f32>(pos, 1.0)));
  out.color = color;
  return out;
}

// A line's corner: its two ends, which end and side this vertex is.
@vertex
fn vs_line(@location(0) a: vec3<f32>, @location(1) b: vec3<f32>, @location(2) color: vec4<f32>, @location(3) corner: vec2<f32>) -> OverlayOut {
  let ca = u.proj * (u.view * vec4<f32>(a, 1.0));
  let cb = u.proj * (u.view * vec4<f32>(b, 1.0));
  let half = u.params.zw * 0.5;
  let sa = ca.xy / ca.w * half;
  let sb = cb.xy / cb.w * half;
  var dir = sb - sa;
  if (dot(dir, dir) == 0.0) {
    dir = vec2<f32>(1.0, 0.0);
  }
  let side = normalize(vec2<f32>(-dir.y, dir.x)) * (u.fog.z * 0.5) * corner.y;
  var clip = select(ca, cb, corner.x > 0.5);
  clip = vec4<f32>(clip.xy + side / half * clip.w, clip.z, clip.w);
  var out: OverlayOut;
  out.position = world_depth(clip);
  out.color = color;
  return out;
}

@fragment
fn fs_overlay(in: OverlayOut) -> @location(0) vec4<f32> {
  return in.color;
}

// Night (mcbench_render::live, Day and night; live/night.rs reads the
// values from night.json). The live view's uniforms say night in fog.w
// (the shots and the live view's day leave it 0, and these entry points
// then draw exactly what vs_quad and vs_ordered draw).

// Minecraft's lightmaps at the level's time 6000 and 18000, by sky * 16 +
// block (LightTexture's 16 x 16 RGBA8 texels over 255).
struct Lightmaps {
  day: array<vec4<f32>, 256>,
  night: array<vec4<f32>, 256>,
};
@group(3) @binding(0) var<uniform> lightmaps: Lightmaps;

fn lightmap_texel(night: bool, sky: u32, block: u32) -> vec3<f32> {
  if (night) {
    return lightmaps.night[sky * 16u + block].rgb;
  }
  return lightmaps.day[sky * 16u + block].rgb;
}

// minecraft_sample_lightmap (core/terrain.vsh): the lightmap sampled
// bilinearly at clamp(light / 16 + 0.5 / 16, 0.5 / 16, 15.5 / 16), the
// sky light along y and the block light along x; in texels, at the levels
// themselves, clamped to 0 to 15. `l` is (sky, block).
fn sample_lightmap(night: bool, l: vec2<f32>) -> vec3<f32> {
  let t = clamp(l, vec2<f32>(0.0), vec2<f32>(15.0));
  let f = t - floor(t);
  let i0 = vec2<u32>(floor(t));
  let i1 = min(i0 + vec2<u32>(1u), vec2<u32>(15u));
  let low = mix(lightmap_texel(night, i0.x, i0.y), lightmap_texel(night, i0.x, i1.y), f.y);
  let high = mix(lightmap_texel(night, i1.x, i0.y), lightmap_texel(night, i1.x, i1.y), f.y);
  return mix(low, high, f.x);
}

// At night, a vertex's colour times Minecraft's lightmap at 18000 over its
// lightmap at 6000 at the corner's sky and block light.
fn at_night(vertex: VertexOut, q: u32, k: u32) -> VertexOut {
  var out = vertex;
  if (u.fog.w > 0.5) {
    var corners = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
    let l = corner_light(q * QUAD_WORDS, corners[k]);
    out.tint = out.tint * (sample_lightmap(true, l) / sample_lightmap(false, l));
  }
  return out;
}

@vertex
fn vs_quad_live(@builtin(vertex_index) v: u32) -> VertexOut {
  return at_night(quad_vertex(v / 6u, v % 6u), v / 6u, v % 6u);
}

@vertex
fn vs_ordered_live(@builtin(vertex_index) v: u32) -> VertexOut {
  let q = order[v / 6u];
  return at_night(quad_vertex(q, v % 6u), q, v % 6u);
}

// The night sky: the sky pass Minecraft's client draws at 18000 (night.json
// skyDraws), each draw with its own uniforms.
struct SkyUniforms {
  // The client's projection times the camera's rotation times the draw's
  // ModelViewMat.
  mvp: mat4x4<f32>,
  // ColorModulator.
  color: vec4<f32>,
  // The Fog block's FogColor, and FogSkyEnd in x.
  fog_color: vec4<f32>,
  fog: vec4<f32>,
};
@group(0) @binding(1) var<uniform> sky: SkyUniforms;
@group(1) @binding(3) var sky_texture: texture_2d<f32>;
@group(1) @binding(4) var sky_sampler: sampler;

struct SkyVertexOut {
  @builtin(position) position: vec4<f32>,
  @location(0) uv: vec2<f32>,
  // fog_spherical_distance and fog_cylindrical_distance (include/fog.glsl).
  @location(1) spherical: f32,
  @location(2) cylindrical: f32,
};

@vertex
fn vs_night_sky(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>) -> SkyVertexOut {
  var out: SkyVertexOut;
  let clip = sky.mvp * vec4<f32>(pos, 1.0);
  // Drawn first, with no depth: any depth within the clip volume.
  out.position = vec4<f32>(clip.xy, clip.w * 0.5, clip.w);
  out.uv = uv;
  out.spherical = length(pos);
  out.cylindrical = max(length(pos.xz), abs(pos.y));
  return out;
}

// include/fog.glsl linear_fog_value.
fn linear_fog_value(d: f32, start: f32, end: f32) -> f32 {
  if (d <= start) {
    return 0.0;
  } else if (d >= end) {
    return 1.0;
  }
  return (d - start) / (end - start);
}

// core/sky.fsh: apply_fog(ColorModulator, spherical, cylindrical, 0,
// FogSkyEnd, FogSkyEnd, FogSkyEnd, FogColor).
@fragment
fn fs_night_sky(in: SkyVertexOut) -> @location(0) vec4<f32> {
  let end = sky.fog.x;
  let fog = max(linear_fog_value(in.spherical, 0.0, end), linear_fog_value(in.cylindrical, end, end));
  return to8(vec4<f32>(mix(sky.color.rgb, sky.fog_color.rgb, fog * sky.fog_color.a), sky.color.a));
}

// core/stars.fsh.
@fragment
fn fs_night_stars(in: SkyVertexOut) -> @location(0) vec4<f32> {
  return sky.color;
}

// core/position_tex.fsh.
@fragment
fn fs_night_texture(in: SkyVertexOut) -> @location(0) vec4<f32> {
  let color = textureSample(sky_texture, sky_sampler, in.uv);
  if (color.a == 0.0) {
    discard;
  }
  return color * sky.color;
}
