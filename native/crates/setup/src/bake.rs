//! The bake in Rust: from the client jar's entries the texture fetch
//! verified, the files the renderer and the block icons read
//! (`mcbench_render::Assets`):
//!
//! | File | What | Today |
//! |---|---|---|
//! | `textures.json` | by the pack's material index, each texture's path, average colour, half-transparency, pixels and animation | BlueMap's `textures.json` (`McbenchBake.java`) |
//! | `atlas.png`, `atlas_map.json` | see_build's block atlas: every texture the materials and the icons use that today's atlas holds (every texture under `block/`, `item/` and `entity/`), at the bake's own boxes ([`own_layout`]); the map lists only the placed textures | misode/mcmeta's atlas and map (`server/assets.js`) |
//! | `client-entries.zip` | every fetched entry, its stored bytes as the jar holds them | the client jar, which the renderer reads the entity textures from |
//!
//! Colours and half-transparency are BlueMap's `Texture.from` in f32; the
//! armour trims are Minecraft's `paletted_permutations` as BlueMap bakes
//! them. Pixels are the PNGs' stored values (no colour space conversion).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::Engine;
use serde_json::Value;

use crate::jar::{Manifest, texture_path};

/// A decoded texture.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// Its width.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// RGBA, 8 bits a channel, rows from the top.
    pub rgba: Vec<u8>,
}

/// A PNG's pixels as stored: palettes and low bit depths expanded, 16 bits
/// cut to 8, grey widened to RGB, no alpha made opaque.
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("The PNG is too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (width, height) = (info.width, info.height);
    let data = &buf[..info.buffer_size()];
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .for_each(|p| rgba.extend_from_slice(&[p[0], p[1], p[2], 255])),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .for_each(|p| rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
        png::ColorType::Grayscale => data
            .iter()
            .for_each(|&g| rgba.extend_from_slice(&[g, g, g, 255])),
        png::ColorType::Indexed => return Err("The PNG's palette was left unexpanded".into()),
    }
    Ok(Image {
        width,
        height,
        rgba,
    })
}

/// An RGBA image as a PNG, compressed fast.
pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// BlueMap's `Texture.from`: half-transparent when a pixel's alpha is
/// strictly between 0 and 1; the average of the premultiplied pixels in
/// f32, columns outer, then made straight (`BufferedImageUtil.averageColor`).
pub fn texture_facts(image: &Image) -> ([f32; 4], bool) {
    let mut half = false;
    let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
    let mut count = 0i32;
    for x in 0..image.width {
        for y in 0..image.height {
            let p = ((y * image.width + x) * 4) as usize;
            let pa = image.rgba[p + 3] as f32 / 255f32;
            if pa > 0f32 && pa < 1f32 {
                half = true;
            }
            r += (image.rgba[p] as f32 / 255f32) * pa;
            g += (image.rgba[p + 1] as f32 / 255f32) * pa;
            b += (image.rgba[p + 2] as f32 / 255f32) * pa;
            a += pa;
            count += 1;
        }
    }
    let q = 1f32 / count as f32;
    let (mut r, mut g, mut b, a) = (r * q, g * q, b * q, a * q);
    if a != 1f32 && a > 0f32 {
        let m = 1f32 / a;
        r *= m;
        g *= m;
        b *= m;
    }
    ([r, g, b, a], half)
}

// BlueMap's Color.set(int) then getInt(): each channel through c / 255f * 255, cut.
fn java_int(image: &Image, x: u32, y: u32) -> u32 {
    let p = ((y * image.width + x) * 4) as usize;
    let c = |v: u8| ((v as f32 / 255f32) * 255f32) as i32 as u32 & 0xFF;
    (c(image.rgba[p + 3]) << 24)
        | (c(image.rgba[p]) << 16)
        | (c(image.rgba[p + 1]) << 8)
        | c(image.rgba[p + 2])
}

/// Minecraft's `paletted_permutations` as BlueMap bakes them
/// (`PalettedPermutationsSource.bake`): each pixel's colour mapped through
/// the key palette to the permutation's, its alpha multiplied by the
/// mapped colour's.
fn permute(image: &Image, key: &Image, values: &Image) -> Image {
    let mut map = HashMap::new();
    for x in 0..key.width {
        for y in 0..key.height {
            map.insert(java_int(key, x, y) | 0xFF00_0000, java_int(values, x, y));
        }
    }
    let mut rgba = vec![0u8; image.rgba.len()];
    for x in 0..image.width {
        for y in 0..image.height {
            let color = java_int(image, x, y);
            let mut alpha = ((color >> 24) & 0xFF) as f32 / 255f32;
            let operand = color | 0xFF00_0000;
            let mapped = *map.get(&operand).unwrap_or(&operand);
            alpha *= ((mapped >> 24) & 0xFF) as f32 / 255f32;
            let p = ((y * image.width + x) * 4) as usize;
            rgba[p] = (mapped >> 16) as u8;
            rgba[p + 1] = (mapped >> 8) as u8;
            rgba[p + 2] = mapped as u8;
            rgba[p + 3] = ((alpha * 255f32) as i32 & 0xFF) as u8;
        }
    }
    Image {
        width: image.width,
        height: image.height,
        rgba,
    }
}

/// BlueMap's own texture for a material it has none for
/// (`bluemap:block/missing`): its colour and its image, as BlueMap writes
/// them (`Texture.MISSING`), held by a test against today's textures.json.
const MISSING: &str = "bluemap:block/missing";
const MISSING_COLOR: [f32; 4] = [0.5, 0.0, 0.5, 1.0];
const MISSING_TEXTURE: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAPklEQVR4Xu3MsQkAMAwDQe2/tFPnBB4gpLhG8MpkZpNkZ6AKZKAKZKAKZKAKZKAKZKAKZKAKWg0XD/UPnjg4MbX+EDdeTUwAAAAASUVORK5CYII=";

/// One material of the pack, baked.
#[derive(Clone, Debug)]
pub struct Material {
    /// Its texture (`resourcePath`).
    pub id: String,
    /// Its pixels, `None` for BlueMap's missing texture.
    pub image: Option<Image>,
    /// Its average colour.
    pub color: [f32; 4],
    /// Whether a pixel is half transparent.
    pub half_transparent: bool,
    /// Its animation as BlueMap writes it, from its `.mcmeta`.
    pub animation: Option<Value>,
}

/// What the bake makes.
pub struct Baked {
    /// By material index.
    pub materials: Vec<Material>,
    /// `textures.json`.
    pub textures_json: Vec<u8>,
    /// `atlas.png`.
    pub atlas_png: Vec<u8>,
    /// `atlas_map.json`.
    pub atlas_map: Vec<u8>,
    /// The atlas's boxes, `[x, y, width, height]` by key (`block/stone`).
    pub boxes: BTreeMap<String, [u32; 4]>,
    /// The atlas's pixels.
    pub atlas: Image,
}

/// The fetched entries' bytes by name, and the PNGs among them decoded.
pub struct Files {
    /// Each entry's bytes.
    pub bytes: HashMap<String, Vec<u8>>,
    /// Each PNG decoded.
    pub images: HashMap<String, Image>,
}

impl Files {
    /// Decodes every PNG of `bytes` on `threads` threads.
    pub fn decode(bytes: HashMap<String, Vec<u8>>, threads: usize) -> Result<Files, String> {
        let pngs: Vec<(&String, &Vec<u8>)> = bytes
            .iter()
            .filter(|(name, _)| name.ends_with(".png"))
            .collect();
        let chunk = pngs.len().div_ceil(threads.max(1)).max(1);
        let images = std::thread::scope(|scope| {
            let workers: Vec<_> = pngs
                .chunks(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter()
                            .map(|(name, png)| {
                                Ok((
                                    (*name).clone(),
                                    decode_png(png).map_err(|e| format!("{name}: {e}"))?,
                                ))
                            })
                            .collect::<Result<Vec<_>, String>>()
                    })
                })
                .collect();
            let mut images = HashMap::new();
            for worker in workers {
                images.extend(
                    worker
                        .join()
                        .map_err(|_| "A PNG decoder thread panicked".to_string())??,
                );
            }
            Ok::<_, String>(images)
        })?;
        Ok(Files { bytes, images })
    }

    fn mcmeta(&self, path: &str) -> Result<Option<Value>, String> {
        match self.bytes.get(&format!("{path}.mcmeta")) {
            None => Ok(None),
            Some(bytes) => serde_json::from_slice(bytes)
                .map(Some)
                .map_err(|e| format!("{path}.mcmeta: {e}")),
        }
    }
}

/// BlueMap's animation of a texture from its `.mcmeta`'s `animation`:
/// interpolate, width, height and frametime with their defaults, and the
/// frames, each an index and its time, when the `.mcmeta` lists them.
fn animation(meta: &Value) -> Result<Value, String> {
    let int = |key: &str, default: i64| -> Result<i64, String> {
        match meta.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_i64()
                .ok_or_else(|| format!("The animation's {key} is not a whole number")),
        }
    };
    let frametime = int("frametime", 1)?;
    let mut out = serde_json::Map::new();
    out.insert(
        "interpolate".into(),
        Value::Bool(
            meta.get("interpolate")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );
    out.insert("width".into(), int("width", 1)?.into());
    out.insert("height".into(), int("height", 1)?.into());
    out.insert("frametime".into(), frametime.into());
    if let Some(frames) = meta.get("frames") {
        let list = frames
            .as_array()
            .ok_or("The animation's frames are not a list")?;
        let frames: Vec<Value> = list
            .iter()
            .map(|frame| {
                let (index, time) = match frame {
                    Value::Object(f) => (
                        f.get("index").and_then(Value::as_i64),
                        f.get("time").and_then(Value::as_i64).unwrap_or(frametime),
                    ),
                    other => (other.as_i64(), frametime),
                };
                let index = index.ok_or("An animation frame has no index")?;
                Ok(serde_json::json!({ "index": index, "time": time }))
            })
            .collect::<Result<_, String>>()?;
        out.insert("frames".into(), Value::Array(frames));
    }
    Ok(Value::Object(out))
}

/// Java's `Double.toString` of `value` (shortest digits that round-trip;
/// plain from 10^-3 to 10^7, else computerised scientific notation), as
/// Gson writes BlueMap's colours.
pub fn java_double(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    let magnitude = value.abs();
    if (1e-3..1e7).contains(&magnitude) {
        let text = format!("{value}");
        return if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        };
    }
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').expect("{:e} writes an exponent");
    let mantissa = if mantissa.contains('.') {
        mantissa.to_string()
    } else {
        format!("{mantissa}.0")
    };
    format!("{mantissa}E{exponent}")
}

/// A texture id with its namespace (`block/stone` is `minecraft:block/stone`).
fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

/// The key see_build's atlas gives a texture the materials or the icons
/// use, when the atlas holds it: today's atlas (misode/mcmeta's) holds every
/// texture of the jar under `block/`, `item/` and `entity/` (measured: its
/// 2,478 keys are exactly those), keyed `block/stone`; the trims and
/// BlueMap's missing texture are not in it, so see_build leaves out their
/// quads, as today.
fn atlas_key(id: &str) -> Option<String> {
    let (space, name) = id.split_once(':').unwrap_or(("minecraft", id));
    (space == "minecraft"
        && ["block/", "item/", "entity/"]
            .iter()
            .any(|f| name.starts_with(f)))
    .then(|| name.to_string())
}

/// The mip levels see_build's renderer makes below the atlas
/// (`native/draw` `block_atlas`: four halvings).
pub const MIP_LEVELS: u32 = 4;

/// An atlas's width, height and each texture's box `[x, y, width, height]`.
pub type Placement = (u32, u32, BTreeMap<String, [u32; 4]>);

/// The bake's own atlas layout. The rule: the textures sorted by height,
/// tallest first, then by width, widest first, then by key (bytes); placed
/// left to right on shelves 2,048 texels wide, a new shelf when a box does
/// not fit; every box starts at a multiple of 2^[`MIP_LEVELS`] (16) and
/// takes its size rounded up to 16 in both directions, so each 16 by 16
/// cell belongs to one texture and no mip level the renderer samples
/// averages two textures; a shelf is as tall as its tallest rounded box,
/// and the atlas as tall as its shelves. Integer arithmetic over sorted
/// input: the same boxes on every machine and run. Answers the atlas's
/// width and height and each box `[x, y, width, height]`.
pub fn own_layout(sizes: &BTreeMap<String, (u32, u32)>) -> Result<Placement, String> {
    const WIDTH: u32 = 2048;
    let cell = 1u32 << MIP_LEVELS;
    let round = |v: u32| v.div_ceil(cell) * cell;
    let mut order: Vec<(&String, &(u32, u32))> = sizes.iter().collect();
    order.sort_by(|a, b| b.1.1.cmp(&a.1.1).then(b.1.0.cmp(&a.1.0)).then(a.0.cmp(b.0)));
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    let mut boxes = BTreeMap::new();
    for (key, &(w, h)) in order {
        if w > WIDTH {
            return Err(format!("{key} is wider than the atlas"));
        }
        if x + round(w) > WIDTH {
            y += shelf;
            x = 0;
            shelf = 0;
        }
        boxes.insert(key.clone(), [x, y, w, h]);
        x += round(w);
        shelf = shelf.max(round(h));
    }
    Ok((WIDTH, y + shelf, boxes))
}

/// The bake (see the module's doc). `files` holds every entry of the
/// manifest.
pub fn bake(manifest: &Manifest, files: &Files) -> Result<Baked, String> {
    let image = |path: &str| files.images.get(path);
    // The trims Minecraft's atlas definitions make (paletted_permutations).
    let mut made: HashMap<String, Image> = HashMap::new();
    let mut atlases: Vec<&String> = files
        .bytes
        .keys()
        .filter(|n| n.starts_with("assets/minecraft/atlases/"))
        .collect();
    atlases.sort();
    for name in atlases {
        let atlas: Value =
            serde_json::from_slice(&files.bytes[name]).map_err(|e| format!("{name}: {e}"))?;
        for source in atlas["sources"]
            .as_array()
            .ok_or_else(|| format!("{name} has no sources"))?
        {
            if source["type"] != "minecraft:paletted_permutations" {
                continue;
            }
            let text = |v: &Value| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{name} has a source that is not text"))
            };
            let separator = source
                .get("separator")
                .and_then(Value::as_str)
                .unwrap_or("_");
            let key_id = text(&source["palette_key"])?;
            let key = image(&texture_path(&key_id))
                .ok_or_else(|| format!("The trims' palette key {key_id} was not fetched"))?;
            let permutations: Vec<(String, &Image)> = source["permutations"]
                .as_object()
                .ok_or_else(|| format!("{name} has permutations that are not an object"))?
                .iter()
                .map(|(suffix, id)| {
                    let id = text(id)?;
                    image(&texture_path(&id))
                        .map(|i| (suffix.clone(), i))
                        .ok_or_else(|| format!("The trims' palette {id} was not fetched"))
                })
                .collect::<Result<_, String>>()?;
            for texture in source["textures"]
                .as_array()
                .ok_or_else(|| format!("{name} has textures that are not a list"))?
            {
                let id = text(texture)?;
                let Some(base) = image(&texture_path(&id)) else {
                    continue;
                };
                for (suffix, values) in &permutations {
                    made.entry(namespaced(&format!("{id}{separator}{suffix}")))
                        .or_insert_with(|| permute(base, key, values));
                }
            }
        }
    }
    let mut materials = Vec::with_capacity(manifest.materials.len());
    for id in &manifest.materials {
        let path = texture_path(id);
        let material = if let Some(i) = image(&path) {
            let (color, half_transparent) = texture_facts(i);
            let animation = match files.mcmeta(&path)? {
                Some(meta) => meta.get("animation").map(animation).transpose()?,
                None => None,
            };
            Material {
                id: id.clone(),
                image: Some(i.clone()),
                color,
                half_transparent,
                animation,
            }
        } else if let Some(i) = made.get(id) {
            let (color, half_transparent) = texture_facts(i);
            Material {
                id: id.clone(),
                image: Some(i.clone()),
                color,
                half_transparent,
                animation: None,
            }
        } else if id == MISSING {
            Material {
                id: id.clone(),
                image: None,
                color: MISSING_COLOR,
                half_transparent: false,
                animation: None,
            }
        } else {
            return Err(format!("No texture {id} was fetched or made"));
        };
        materials.push(material);
    }
    let textures_json = textures_json(&materials)?;
    // see_build's atlas and the icons' textures.
    let wanted: BTreeSet<&String> = manifest
        .materials
        .iter()
        .chain(manifest.particles.iter().map(|(_, texture)| texture))
        .collect();
    let mut placed: BTreeMap<String, &Image> = BTreeMap::new();
    for id in wanted {
        if let Some(key) = atlas_key(id) {
            let i = image(&texture_path(id))
                .ok_or_else(|| format!("No texture {id} was fetched for the atlas"))?;
            placed.insert(key, i);
        }
    }
    let sizes: BTreeMap<String, (u32, u32)> = placed
        .iter()
        .map(|(k, i)| (k.clone(), (i.width, i.height)))
        .collect();
    let (width, height, boxes) = own_layout(&sizes)?;
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (key, i) in &placed {
        let [x, y, w, h] = boxes[key];
        for row in 0..h {
            let from = (row * w * 4) as usize;
            let to = (((y + row) * width + x) * 4) as usize;
            rgba[to..to + (w * 4) as usize].copy_from_slice(&i.rgba[from..from + (w * 4) as usize]);
        }
    }
    let atlas = Image {
        width,
        height,
        rgba,
    };
    let atlas_png = encode_png(atlas.width, atlas.height, &atlas.rgba)?;
    let map: serde_json::Map<String, Value> = boxes
        .iter()
        .map(|(k, b)| (k.clone(), serde_json::json!(b)))
        .collect();
    let atlas_map = serde_json::to_vec(&map).map_err(|e| e.to_string())?;
    Ok(Baked {
        materials,
        textures_json,
        atlas_png,
        atlas_map,
        boxes,
        atlas,
    })
}

/// `textures.json` as BlueMap writes it: by material, its path, colour,
/// half-transparency, image as a PNG data address and its animation.
fn textures_json(materials: &[Material]) -> Result<Vec<u8>, String> {
    let mut out = String::from("[");
    for (i, m) in materials.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let texture = match &m.image {
            Some(image) => format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(encode_png(
                    image.width,
                    image.height,
                    &image.rgba
                )?)
            ),
            None => MISSING_TEXTURE.to_string(),
        };
        let color: Vec<String> = m.color.iter().map(|&c| java_double(c as f64)).collect();
        out.push_str(&format!(
            "{{\"resourcePath\":{},\"color\":[{}],\"halfTransparent\":{},\"texture\":{}",
            Value::String(m.id.clone()),
            color.join(","),
            m.half_transparent,
            Value::String(texture)
        ));
        if let Some(animation) = &m.animation {
            out.push_str(&format!(",\"animation\":{animation}"));
        }
        out.push('}');
    }
    out.push(']');
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_own_layout_puts_every_box_on_the_16_grid_tallest_first_and_is_the_same_each_time() {
        let sizes: BTreeMap<String, (u32, u32)> = [
            ("block/a", (16, 16)),
            ("block/b", (8, 8)),
            ("block/c", (32, 64)),
            ("block/d", (16, 32)),
            ("item/e", (2040, 16)),
        ]
        .into_iter()
        .map(|(k, s)| (k.to_string(), s))
        .collect();
        let (w, h, boxes) = own_layout(&sizes).unwrap();
        assert_eq!(boxes["block/c"], [0, 0, 32, 64]);
        assert_eq!(boxes["block/d"], [32, 0, 16, 32]);
        assert_eq!(
            boxes["item/e"],
            [0, 64, 2040, 16],
            "a box that does not fit starts a shelf"
        );
        assert_eq!(boxes["block/a"], [0, 80, 16, 16]);
        assert_eq!(
            boxes["block/b"],
            [16, 80, 8, 8],
            "an 8 by 8 texture takes a 16 by 16 cell"
        );
        assert_eq!((w, h), (2048, 96));
        assert!(boxes.values().all(|b| b[0] % 16 == 0 && b[1] % 16 == 0));
        assert_eq!(own_layout(&sizes).unwrap(), (w, h, boxes));
    }

    #[test]
    fn colours_are_written_as_java_writes_a_double() {
        assert_eq!(java_double(0.5), "0.5");
        assert_eq!(java_double(0.0), "0.0");
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(
            java_double("0.4049025".parse::<f32>().unwrap() as f64),
            "0.40490248799324036"
        );
        assert_eq!(java_double(0.0001), "1.0E-4");
        assert_eq!(java_double(0.00012345), "1.2345E-4");
    }

    #[test]
    fn a_png_round_trips_through_the_encoder_and_decoder() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 37 % 256) as u8).collect();
        let image = decode_png(&encode_png(2, 3, &rgba).unwrap()).unwrap();
        assert_eq!((image.width, image.height), (2, 3));
        assert_eq!(image.rgba, rgba);
    }

    #[test]
    fn an_animation_takes_blue_maps_defaults_and_frames_their_times() {
        let meta: Value = serde_json::from_str(
            r#"{"frametime":8,"interpolate":true,"frames":[0,{"index":2,"time":3}]}"#,
        )
        .unwrap();
        assert_eq!(
            animation(&meta).unwrap().to_string(),
            r#"{"interpolate":true,"width":1,"height":1,"frametime":8,"frames":[{"index":0,"time":8},{"index":2,"time":3}]}"#
        );
        assert_eq!(
            animation(&serde_json::json!({})).unwrap().to_string(),
            r#"{"interpolate":false,"width":1,"height":1,"frametime":1}"#
        );
    }
}
