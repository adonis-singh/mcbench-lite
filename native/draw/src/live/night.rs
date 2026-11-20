// Minecraft's own night for the live view (mcbench_render::live, Day and
// night), read from `night.json`: what Minecraft 1.21.10's client computes
// at the level's time 6000 and 18000 under its default options
// (server/McbenchNight.java, the extractor's night step). Each value below
// names the `sources` entry of night.json it comes from, which names the
// client's class and method.

use serde_json::Value;
use std::io::Read;
use std::path::Path;

// The biome the sky and fog are read for: the app's live worlds are flat
// and carry no biome, and the capture's sky pass is plains' (night.json
// sources.skyDraws: "on the SkyRenderState of minecraft:plains").
const BIOME: &str = "minecraft:plains";

// How a sky draw is shaded (its RenderPipeline's shaders, night.json
// skyDraws[].pipelineState): core/sky (the disc, its colour fogged by
// distance), core/stars (a colour), core/position_tex (a texture).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shader {
    Sky,
    Stars,
    Texture,
}

// One draw of the sky pass at 18000, its vertices as triangles.
pub struct SkyDraw {
    pub label: String,
    pub shader: Shader,
    // Each vertex: position and texture coordinates (0 where none).
    pub vertices: Vec<[f32; 5]>,
    // DynamicTransforms' ModelViewMat (column-major) and ColorModulator.
    pub model_view: [f32; 16],
    pub color: [f32; 4],
    // Blended SRC_ALPHA, ONE (the celestial and stars pipelines' blend).
    pub additive: bool,
    // Sampler0's texture: its path in the client jar, width, height, RGBA8.
    pub texture: Option<(String, u32, u32, Vec<u8>)>,
}

pub struct Night {
    // The lightmaps at 6000 and 18000, by sky * 16 + block, each texel's
    // RGBA8 over 255 (night.json lightmap[t].rgba8: sources
    // "lightmap.texels", LightTexture.updateLightTexture's uniforms run
    // through core/lightmap.fsh).
    pub day: Vec<[f32; 4]>,
    pub night: Vec<[f32; 4]>,
    // The fog colour at 18000 (night.json sky[plains]["18000"].fogColor:
    // sources "sky.fog", FogRenderer.setupFog's computeFogColor) and the Fog
    // block's FogSkyEnd the sky disc fades by.
    pub fog_color: [f32; 4],
    pub sky_end: f32,
    // The sky pass at 18000 (night.json skyDraws["18000"] and buffers:
    // sources "skyDraws", "buffers", "stars", "moon").
    pub draws: Vec<SkyDraw>,
}

fn floats(v: &Value, what: &str) -> Result<Vec<f32>, String> {
    v.as_array()
        .and_then(|a| a.iter().map(|x| x.as_f64().map(|f| f as f32)).collect())
        .ok_or_else(|| format!("night.json: {what} is not a list of numbers"))
}

fn lightmap(j: &Value, time: &str) -> Result<Vec<[f32; 4]>, String> {
    let rows = j["lightmap"][time]["rgba8"]
        .as_array()
        .filter(|r| r.len() == 16)
        .ok_or_else(|| format!("night.json: lightmap {time} has no 16 rows"))?;
    let mut out = Vec::with_capacity(256);
    for row in rows {
        let row = row
            .as_array()
            .filter(|r| r.len() == 16)
            .ok_or_else(|| format!("night.json: a lightmap {time} row has not 16 texels"))?;
        for texel in row {
            let t = floats(texel, "a lightmap texel")?;
            out.push([t[0] / 255.0, t[1] / 255.0, t[2] / 255.0, t[3] / 255.0]);
        }
    }
    Ok(out)
}

// A texture of the client jar, as RGBA8.
fn texture(jar: &Path, path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut bytes = Vec::new();
    zip.by_name(path)
        .map_err(|e| format!("{}: {path}: {e}", jar.display()))?
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{path}: {e}"))?;
    let image = image::load_from_memory(&bytes)
        .map_err(|e| format!("{path}: {e}"))?
        .to_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}

pub fn read(night: &Path, jar: &Path) -> Result<Night, String> {
    let text = std::fs::read(night).map_err(|e| format!("{}: {e}", night.display()))?;
    let j: Value =
        serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", night.display()))?;
    let sky = &j["sky"][BIOME]["18000"];
    let fog_color = floats(&sky["fogColor"], "the fog colour")?;
    let sky_end = sky["fog"]["FogSkyEnd"]
        .as_f64()
        .ok_or("night.json: no FogSkyEnd")? as f32;
    let mut draws = Vec::new();
    for d in j["skyDraws"]["18000"]
        .as_array()
        .ok_or("night.json: no sky pass at 18000")?
    {
        let label = d["label"].as_str().unwrap_or("").to_string();
        let state = &d["pipelineState"];
        let shader = match state["vertexShader"].as_str() {
            Some("minecraft:core/sky") => Shader::Sky,
            Some("minecraft:core/stars") => Shader::Stars,
            Some("minecraft:core/position_tex") => Shader::Texture,
            other => return Err(format!("night.json: the sky draw {label} has the shader {other:?}, which the live view does not draw")),
        };
        let blend = &state["blend"];
        let additive = match blend.as_object() {
            None => false,
            Some(_) if blend["sourceColor"] == "SRC_ALPHA" && blend["destColor"] == "ONE" => true,
            Some(_) => {
                return Err(format!(
                    "night.json: the sky draw {label} blends as {blend}"
                ))
            }
        };
        let buffer = &j["buffers"][d["vertexBuffer"]
            .as_str()
            .ok_or("night.json: a sky draw names no buffer")?];
        let verts: Vec<Vec<f32>> = buffer["vertices"]
            .as_array()
            .ok_or("night.json: a buffer has no vertices")?
            .iter()
            .map(|v| floats(v, "a vertex"))
            .collect::<Result<_, _>>()?;
        let vertex = |i: usize| -> Result<[f32; 5], String> {
            let v = verts
                .get(i)
                .ok_or("night.json: a draw reads past its buffer")?;
            Ok([
                v[0],
                v[1],
                v[2],
                *v.get(3).unwrap_or(&0.0),
                *v.get(4).unwrap_or(&0.0),
            ])
        };
        let mut vertices = Vec::new();
        for draw in d["draws"]
            .as_array()
            .ok_or("night.json: a sky draw has no draws")?
        {
            match state["vertexFormatMode"].as_str() {
                // VertexFormat.Mode.QUADS: each four vertices drawn 0 1 2, 2 3 0.
                Some("QUADS") => {
                    let (base, first, count) = (
                        draw["baseVertex"].as_u64().unwrap_or(0) as usize,
                        draw["firstIndex"].as_u64().unwrap_or(0) as usize,
                        draw["indexCount"].as_u64().unwrap_or(0) as usize,
                    );
                    for q in first / 6..(first + count) / 6 {
                        for k in [0, 1, 2, 2, 3, 0] {
                            vertices.push(vertex(base + q * 4 + k)?);
                        }
                    }
                }
                // TRIANGLE_FAN: the first vertex and each next two.
                Some("TRIANGLE_FAN") => {
                    let (first, count) = (
                        draw["firstVertex"].as_u64().unwrap_or(0) as usize,
                        draw["vertexCount"].as_u64().unwrap_or(0) as usize,
                    );
                    for i in 1..count.saturating_sub(1) {
                        for k in [first, first + i, first + i + 1] {
                            vertices.push(vertex(k)?);
                        }
                    }
                }
                other => {
                    return Err(format!(
                        "night.json: the sky draw {label} is drawn as {other:?}"
                    ))
                }
            }
        }
        let t = &d["uniforms"]["DynamicTransforms"];
        let model_view: [f32; 16] = floats(&t["ModelViewMat"], "ModelViewMat")?
            .try_into()
            .map_err(|_| "night.json: a ModelViewMat is not 16 numbers")?;
        let color: [f32; 4] = floats(&t["ColorModulator"], "ColorModulator")?
            .try_into()
            .map_err(|_| "night.json: a ColorModulator is not 4 numbers")?;
        let texture = match d["samplers"]["Sampler0"].as_str() {
            Some(path) => {
                let (w, h, rgba) = texture(jar, path)?;
                Some((path.to_string(), w, h, rgba))
            }
            None => None,
        };
        draws.push(SkyDraw {
            label,
            shader,
            vertices,
            model_view,
            color,
            additive,
            texture,
        });
    }
    Ok(Night {
        day: lightmap(&j, "6000")?,
        night: lightmap(&j, "18000")?,
        fog_color: fog_color
            .try_into()
            .map_err(|_| "night.json: the fog colour is not 4 numbers")?,
        sky_end,
        draws,
    })
}
