// What the helper draws with besides a blueprint, all of it the app's own
// bake of Minecraft's data (PROTOCOL.md, hello): the mesher's pack
// (McbenchBake.java), its texture names (BlueMap's textures.json), see_build's
// block atlas (misode/mcmeta's atlas.png and atlas_map.json), the block table
// (McbenchBlockTable.java), the render layers (McbenchClient.java), and the
// entity capture (McbenchEntities.java) whose atlas is made here from the
// client jar's textures, read raw.

use mcbench_renderer::pack::Pack;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Paths {
    pub pack: PathBuf,
    pub textures: PathBuf,
    pub atlas: PathBuf,
    pub atlas_map: PathBuf,
    pub block_table: PathBuf,
    pub render_layers: PathBuf,
    pub entities: PathBuf,
    pub client_jar: PathBuf,
    // Linux: the software floor's driver library shipped beside the helper.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub software_vulkan: Option<PathBuf>,
}

impl Paths {
    // The files of a data folder of today's bake: its one bake, its one
    // block table, its assets. For tools and tests; the server names the
    // files itself.
    pub fn of_data(data: &Path) -> Result<Paths, String> {
        let bluemap = data.join("minecraft-live/bluemap");
        let bake = only(&bluemap, |name| name.len() == 16 && name.chars().all(|c| c.is_ascii_hexdigit()))?;
        let paper = data.join("minecraft-live/paper");
        let table = only(&paper, |name| name.starts_with("block-table-") && name.ends_with(".tsv"))?;
        let jar = only(&bluemap.join("data"), |name| name.starts_with("minecraft-client-") && name.ends_with(".jar"))?;
        Ok(Paths {
            pack: bake.join("web/mcbench-pack.bin"),
            textures: bake.join("web/maps/live/textures.json"),
            atlas: data.join("assets/1.21.10/atlas.png"),
            atlas_map: data.join("assets/1.21.10/atlas_map.json"),
            block_table: table,
            render_layers: bake.join("render-layers.tsv"),
            entities: bake.join("web/mcbench-entities.json"),
            client_jar: jar,
            software_vulkan: None,
        })
    }
}

fn only(folder: &Path, wanted: impl Fn(&str) -> bool) -> Result<PathBuf, String> {
    let found: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| wanted(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(format!("{} holds {} matching entries, not one", folder.display(), found.len())),
    }
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

// A texture atlas on the GPU's terms: a square of a power of two (so a
// texel's edge is exact in 16-bit texture coordinates), its mip levels,
// each texture's box in texels, and the half-texel the shader clamps
// sampling inside a box by, per axis, in the square's coordinates (the
// page's `pixelSize`, kept as the page computes it over its own image).
pub struct Atlas {
    pub side: u32,
    pub levels: Vec<Vec<u8>>,
    pub boxes: HashMap<String, [u32; 4]>,
    pub pixel: [f32; 2],
    // Mip levels are blended linearly (the block atlas) or not used.
    pub mipmapped: bool,
}

// see_build's block atlas as the GPU samples it: the image on a square of
// the next power of two, four alpha-aware mip levels (`halve`), and each
// texture's box.
pub fn block_atlas(png: &[u8], map: &[u8]) -> Result<Atlas, String> {
    let image = image::load_from_memory(png).map_err(|e| format!("atlas.png: {e}"))?.to_rgba8();
    let side = image.width().max(image.height()).next_power_of_two();
    let mut base = vec![0u8; (side * side * 4) as usize];
    blit(&mut base, side, image.as_raw(), image.width(), image.height(), 0, 0);
    let mut levels = vec![base];
    let mut width = side;
    for _ in 0..4 {
        let next = halve(levels.last().unwrap(), width);
        width /= 2;
        levels.push(next);
    }
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(map).map_err(|e| format!("atlas_map.json: {e}"))?;
    let mut boxes = HashMap::new();
    for (id, value) in map {
        let v: Vec<u32> = value
            .as_array()
            .ok_or("atlas_map.json has an entry that is not a list")?
            .iter()
            .map(|n| n.as_u64().map(|n| n as u32).ok_or("atlas_map.json has a box that is not whole numbers"))
            .collect::<Result<_, _>>()?;
        let [u, v0, du, dv] = v[..] else { return Err(format!("atlas_map.json's box for {id} is not four numbers")) };
        // Animated textures are vertical strips: the first square frame.
        let dv = if du != dv && id.starts_with("block/") { du } else { dv };
        let key = if id.contains(':') { id } else { format!("minecraft:{id}") };
        boxes.insert(key, [u, v0, u + du, v0 + dv]);
    }
    Ok(Atlas { side, levels, boxes, pixel: [1.0 / side as f32; 2], mipmapped: true })
}

fn blit(dst: &mut [u8], side: u32, src: &[u8], width: u32, height: u32, x: u32, y: u32) {
    for row in 0..height {
        let from = (row * width * 4) as usize;
        let to = (((y + row) * side + x) * 4) as usize;
        dst[to..to + (width * 4) as usize].copy_from_slice(&src[from..from + (width * 4) as usize]);
    }
}

// A mip level from the one above, each pixel from its four: where two or
// more are mostly opaque (alpha at least 128), their colour weighted by
// alpha and their mean alpha; otherwise the plain mean colour of those not
// clear, alpha 0. Rounded as a Uint8ClampedArray rounds (to nearest, ties
// to even).
fn halve(src: &[u8], width: u32) -> Vec<u8> {
    let w = (width / 2) as usize;
    let sw = width as usize;
    let mut out = vec![0u8; w * w * 4];
    let clamp = |v: f64| -> u8 { v.clamp(0.0, 255.0).round_ties_even() as u8 };
    for y in 0..w {
        for x in 0..w {
            let i00 = ((y * 2) * sw + x * 2) * 4;
            let (mut r, mut g, mut b, mut a_sum, mut solid) = (0f64, 0f64, 0f64, 0f64, 0u32);
            let (mut fr, mut fg, mut fb, mut seen) = (0f64, 0f64, 0f64, 0u32);
            for i in [i00, i00 + 4, i00 + sw * 4, i00 + sw * 4 + 4] {
                let a = src[i + 3] as f64;
                if a >= 128.0 {
                    solid += 1;
                    r += src[i] as f64 * a;
                    g += src[i + 1] as f64 * a;
                    b += src[i + 2] as f64 * a;
                    a_sum += a;
                }
                if a > 0.0 {
                    fr += src[i] as f64;
                    fg += src[i + 1] as f64;
                    fb += src[i + 2] as f64;
                    seen += 1;
                }
            }
            let o = (y * w + x) * 4;
            if solid >= 2 {
                out[o] = clamp(r / a_sum);
                out[o + 1] = clamp(g / a_sum);
                out[o + 2] = clamp(b / a_sum);
                out[o + 3] = clamp(a_sum / solid as f64);
            } else if seen > 0 {
                out[o] = clamp(fr / seen as f64);
                out[o + 1] = clamp(fg / seen as f64);
                out[o + 2] = clamp(fb / seen as f64);
            }
        }
    }
    out
}

// The entity capture's atlas: each texture the capture names at its box
// (McbenchEntityAssets.atlas packed them, `rects`), its pixels read from the
// client jar as stored. Today's bake writes the atlas through Java's ImageIO,
// which reads a grey PNG with transparency as linear light and writes it
// brightened (a stored 104 becomes 171); Minecraft reads the stored values,
// and so does this. An animated texture is its first square frame, as the
// bake takes it.
pub fn entity_atlas(json: &serde_json::Value, jar: &Path) -> Result<Atlas, String> {
    let atlas = &json["atlas"];
    let width = atlas["width"].as_u64().ok_or("The entity capture's atlas has no width")? as u32;
    let height = atlas["height"].as_u64().ok_or("The entity capture's atlas has no height")? as u32;
    let rects = atlas["rects"].as_object().ok_or("The entity capture's atlas has no rects")?;
    let side = width.max(height).next_power_of_two();
    let mut pixels = vec![0u8; (side * side * 4) as usize];
    // The jar read whole (one read instead of thousands of small ones),
    // each texture's PNG and animation file taken from it, then decoded in
    // parallel.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(read(jar)?)).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut wanted = Vec::new();
    for (name, rect) in rects {
        let r: Vec<u32> = rect.as_array().ok_or("An entity atlas rect is not a list")?.iter().map(|v| v.as_u64().unwrap_or(u64::MAX) as u32).collect();
        let [x, y, w, h] = r[..] else { return Err(format!("The entity atlas rect of {name} is not four numbers")) };
        if x + w > width || y + h > height {
            return Err(format!("The entity atlas rect of {name} lies outside the atlas"));
        }
        let (space, path) = name.split_once(':').ok_or_else(|| format!("The entity texture {name} has no namespace"))?;
        let entry = format!("assets/{space}/textures/{path}.png");
        let mut bytes = Vec::new();
        zip.by_name(&entry).map_err(|e| format!("{entry} in the client jar: {e}"))?.read_to_end(&mut bytes).map_err(|e| format!("{entry}: {e}"))?;
        let animated = match zip.by_name(&format!("{entry}.mcmeta")) {
            Ok(mut meta) => {
                let mut text = String::new();
                meta.read_to_string(&mut text).map_err(|e| format!("{entry}.mcmeta: {e}"))?;
                serde_json::from_str::<serde_json::Value>(&text).map_err(|e| format!("{entry}.mcmeta: {e}"))?.get("animation").is_some()
            }
            Err(zip::result::ZipError::FileNotFound) => false,
            Err(e) => return Err(format!("{entry}.mcmeta: {e}")),
        };
        wanted.push((name.clone(), entry, bytes, animated, [x, y, w, h]));
    }
    use rayon::prelude::*;
    let decoded: Vec<(String, image::RgbaImage, [u32; 4])> = wanted
        .into_par_iter()
        .map(|(name, entry, bytes, animated, [x, y, w, h])| {
            let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|e| format!("{entry}: {e}"))?.to_rgba8();
            let frame_height = if animated { image.width() } else { image.height() };
            if image.width() != w || frame_height != h {
                return Err(format!("{entry} is {}x{frame_height}; the entity capture placed it as {w}x{h}", image.width()));
            }
            Ok((name, image, [x, y, w, h]))
        })
        .collect::<Result<_, String>>()?;
    let mut boxes = HashMap::new();
    for (name, image, [x, y, w, h]) in decoded {
        blit(&mut pixels, side, &image.as_raw()[..(w * h * 4) as usize], w, h, x, y);
        boxes.insert(name, [x, y, x + w, y + h]);
    }
    let longest = width.max(height) as f32;
    let s = side as f32;
    Ok(Atlas { side, levels: vec![pixels], boxes, pixel: [width as f32 / (longest * s), height as f32 / (longest * s)], mipmapped: false })
}

// By the pack's material index, the texture's resource path (BlueMap's
// textures.json).
pub fn material_names(bytes: &[u8]) -> Result<Vec<String>, String> {
    let list: Vec<serde_json::Value> = serde_json::from_slice(bytes).map_err(|e| format!("textures.json: {e}"))?;
    list.iter().map(|t| t["resourcePath"].as_str().map(str::to_string).ok_or_else(|| "textures.json has an entry without a resourcePath".to_string())).collect()
}

// What see_build's page knows of each pack state from the block table and
// render layers: whether its six faces are full (fullFaces 63), whether its
// model and its fluid are drawn blended.
#[derive(Clone, Copy, Default, Debug)]
pub struct Facts {
    pub full: bool,
    pub model_blended: bool,
    pub fluid_blended: bool,
}

pub fn facts(table: &str, layers: &str, pack: &Pack) -> Result<Vec<Facts>, String> {
    // The table's last three lines are its pair tables (block-states.js).
    let mut lines: Vec<&str> = table.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() < 3 {
        return Err("The block table is too short".into());
    }
    lines.truncate(lines.len() - 3);
    let states: Vec<Vec<&str>> = lines.iter().filter(|l| !l.starts_with("default\t")).map(|l| l.split('\t').collect::<Vec<_>>()).collect();
    let mut blended = Vec::new();
    let mut layer_lines = Vec::new();
    for line in layers.lines().filter(|l| !l.is_empty()) {
        if let Some(rest) = line.strip_prefix("layer\t") {
            blended.push(rest.split('\t').nth(1) == Some("1"));
        } else {
            layer_lines.push(line.split('\t').collect::<Vec<_>>());
        }
    }
    if layer_lines.len() != states.len() {
        return Err(format!("The render layers have {} states, the block table {}", layer_lines.len(), states.len()));
    }
    let mut out = vec![Facts::default(); pack.states.len()];
    let layer = |text: &str| -> Result<bool, String> { blended.get(text.parse::<usize>().map_err(|e| format!("render layer {text}: {e}"))?).copied().ok_or_else(|| format!("No render layer {text}")) };
    for (fields, line) in states.iter().zip(&layer_lines) {
        if fields.len() < 4 || line.len() < 3 {
            return Err(format!("A block table or render layer line is short: {}", fields[0]));
        }
        if fields[0] != line[0] {
            return Err(format!("The render layers name {} where the block table names {}", line[0], fields[0]));
        }
        let state = pack.named(fields[0])?;
        out[state as usize] = Facts { full: fields[3] == "63", model_blended: layer(line[1])?, fluid_blended: layer(line[2])? };
    }
    Ok(out)
}

// Everything hello loads, ready to draw with. The entity capture and its
// atlas are loaded with the first world that holds an entity (as the page
// loads them, prepareEntities): most worlds hold none.
pub struct Assets {
    pub pack: Pack,
    pub names: Vec<String>,
    pub facts: Vec<Facts>,
    pub atlas: Atlas,
    pub paths: Paths,
    pub level: Level,
    entity: std::sync::OnceLock<EntityAssets>,
}

// The overworld's height (its dimension type in the client's own data:
// data/minecraft/dimension_type/overworld.json): what Minecraft's octree of
// sections is laid over (store.rs, Translucent::ordered).
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub min_y: i32,
    pub height: i32,
}

pub fn overworld(jar: &Path) -> Result<Level, String> {
    let file = std::fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", jar.display()))?;
    const NAME: &str = "data/minecraft/dimension_type/overworld.json";
    let mut text = String::new();
    std::io::Read::read_to_string(&mut zip.by_name(NAME).map_err(|e| format!("{}: {NAME}: {e}", jar.display()))?, &mut text).map_err(|e| format!("{}: {NAME}: {e}", jar.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{}: {NAME}: {e}", jar.display()))?;
    let field = |k: &str| json[k].as_i64().map(|v| v as i32).ok_or(format!("{}: {NAME} has no {k}", jar.display()));
    Ok(Level { min_y: field("min_y")?, height: field("height")? })
}

pub struct EntityAssets {
    pub capture: crate::entities::Capture,
    pub atlas: Atlas,
}

impl Assets {
    pub fn load(paths: &Paths) -> Result<Assets, String> {
        let pack = Pack::read(&read(&paths.pack)?).map_err(|e| format!("{}: {e}", paths.pack.display()))?;
        let names = material_names(&read(&paths.textures)?)?;
        let table = String::from_utf8(read(&paths.block_table)?).map_err(|_| "The block table is not UTF-8".to_string())?;
        let layers = String::from_utf8(read(&paths.render_layers)?).map_err(|_| "The render layers are not UTF-8".to_string())?;
        let facts = facts(&table, &layers, &pack)?;
        let atlas = block_atlas(&read(&paths.atlas)?, &read(&paths.atlas_map)?)?;
        let level = overworld(&paths.client_jar)?;
        Ok(Assets { pack, names, facts, atlas, paths: paths.clone(), level, entity: std::sync::OnceLock::new() })
    }

    // Whether a world with entities has loaded the capture.
    pub fn entity_loaded(&self) -> bool {
        self.entity.get().is_some()
    }

    pub fn entity(&self) -> Result<&EntityAssets, String> {
        if let Some(e) = self.entity.get() {
            return Ok(e);
        }
        let json: serde_json::Value = serde_json::from_slice(&read(&self.paths.entities)?).map_err(|e| format!("{}: {e}", self.paths.entities.display()))?;
        let atlas = entity_atlas(&json, &self.paths.client_jar)?;
        let capture = crate::entities::Capture::new(json)?;
        let _ = self.entity.set(EntityAssets { capture, atlas });
        Ok(self.entity.get().expect("just set"))
    }
}
