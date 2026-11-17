struct SkyOut {
  @builtin(position) position: vec4<f32>,
  @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> SkyOut {
  var corners = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
  var out: SkyOut;
  out.ndc = corners[i];
  out.position = vec4<f32>(corners[i], 0.0, 1.0);
  return out;
}

// The supersampled picture scaled down by `factor`: the average of the
// factor x factor resolved pixels of each output pixel, in f32, rounded to
// 8 bits here (half up) rather than by the hardware's conversion, which
// rounds differently between adapters.
@group(0) @binding(0) var big: texture_2d<f32>;
@group(0) @binding(1) var<uniform> factor: vec4<u32>;

@fragment
fn fs_down(in: SkyOut) -> @location(0) vec4<f32> {
  let at = vec2<u32>(in.position.xy) * factor.x;
  var sum = vec4<f32>(0.0);
  for (var y = 0u; y < factor.x; y++) {
    for (var x = 0u; x < factor.x; x++) {
      sum += floor(textureLoad(big, at + vec2<u32>(x, y), 0) * 255.0 + 0.5);
    }
  }
  return floor(sum / f32(factor.x * factor.x) + 0.5) / 255.0;
}
