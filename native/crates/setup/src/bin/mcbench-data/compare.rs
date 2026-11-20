//! The bake from a local jar, its comparison with today's bake and with
//! misode/mcmeta's atlas, and a timed first start.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine;
use mcbench_setup::Data;
use mcbench_setup::bake::{Files, Image, bake, decode_png};
use mcbench_setup::bundle::Lock;
use mcbench_setup::fetch::{self, Options};
use mcbench_setup::jar::{self, write_zip};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// The bake of the staged data in `folder` from a whole local jar, into
/// `out` (as the first start writes it).
pub fn bake_local(folder: &Path, jar_file: &Path, out: &Path) -> Result<(), String> {
    let data = Data::at(folder);
    let manifest = data.manifest()?;
    let jar_bytes = read(jar_file)?;
    let started = Instant::now();
    let mut bytes = HashMap::new();
    let mut stored = Vec::new();
    for e in &manifest.entries {
        let s = jar::stored(&jar_bytes, e);
        bytes.insert(e.name.clone(), e.unpack(s)?);
        stored.push((e, s));
    }
    let checked = started.elapsed();
    let files = Files::decode(bytes, 8)?;
    let decoded = started.elapsed();
    let baked = bake(&manifest, &files)?;
    let zip = write_zip(&stored);
    let made = started.elapsed();
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (name, b) in [
        ("textures.json", &baked.textures_json),
        ("atlas.png", &baked.atlas_png),
        ("atlas_map.json", &baked.atlas_map),
        ("client-entries.zip", &zip),
    ] {
        std::fs::write(out.join(name), b).map_err(|e| format!("{name}: {e}"))?;
    }
    println!(
        "{}",
        serde_json::json!({ "entries": manifest.entries.len(), "checked_ms": checked.as_secs_f64() * 1e3, "decoded_ms": decoded.as_secs_f64() * 1e3, "baked_ms": made.as_secs_f64() * 1e3, "atlas": [baked.atlas.width, baked.atlas.height] })
    );
    Ok(())
}

fn data_url_png(url: &str) -> Result<Image, String> {
    let (_, b64) = url
        .split_once(',')
        .ok_or("A texture is not a data address")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| e.to_string())?;
    decode_png(&bytes)
}

/// Whether a PNG is grey with an alpha channel or a tRNS chunk (which Java's
/// ImageIO reads as linear grey).
fn grey_alpha(png_bytes: &[u8]) -> Result<bool, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info();
    Ok(info.color_type == png::ColorType::GrayscaleAlpha
        || (info.color_type == png::ColorType::Grayscale && info.trns.is_some()))
}

// halveRGBA of web/viewer.js as native/draw/src/assets.rs (native-stage2)
// ports it, with Uint8ClampedArray's rounding.
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

/// see_build's mip levels of an atlas (`block_atlas`: the image on a square
/// of the next power of two, four levels below it).
fn levels(image: &Image) -> (u32, Vec<Vec<u8>>) {
    let side = image.width.max(image.height).next_power_of_two();
    let mut base = vec![0u8; (side * side * 4) as usize];
    for row in 0..image.height {
        let from = (row * image.width * 4) as usize;
        let to = (row * side * 4) as usize;
        base[to..to + (image.width * 4) as usize]
            .copy_from_slice(&image.rgba[from..from + (image.width * 4) as usize]);
    }
    let mut out = vec![base];
    let mut width = side;
    for _ in 0..4 {
        let next = halve(out.last().expect("one level"), width);
        width /= 2;
        out.push(next);
    }
    (side, out)
}

/// The box `[x, y, w, h]` (first square frame of a `block/` strip, as
/// `block_atlas` takes it) of mip level `level`.
fn region(levels: &[Vec<u8>], side: u32, level: usize, b: [u32; 4]) -> Vec<u8> {
    let s = 1u32 << level;
    let width = side / s;
    let [x, y, w, h] = b;
    let mut out = Vec::new();
    for row in y / s..(y + h).div_ceil(s) {
        let from = ((row * width + x / s) * 4) as usize;
        out.extend_from_slice(&levels[level][from..from + ((w.div_ceil(s)) * 4) as usize]);
    }
    out
}

fn boxes(map: &Value) -> Result<BTreeMap<String, [u32; 4]>, String> {
    map.as_object()
        .ok_or("An atlas map is not an object")?
        .iter()
        .map(|(id, v)| {
            let n = |i: usize| {
                v.get(i)
                    .and_then(Value::as_u64)
                    .map(|n| n as u32)
                    .ok_or("An atlas box is not numbers")
            };
            let (u, v0, du, dv) = (n(0)?, n(1)?, n(2)?, n(3)?);
            let dv = if du != dv && id.starts_with("block/") {
                du
            } else {
                dv
            };
            Ok((id.clone(), [u, v0, du, dv]))
        })
        .collect()
}

// Java2D's SrcOver of a non-premultiplied image onto a cleared TYPE_INT_ARGB
// one (McbenchEntityAssets.load's drawImage), with Java's 8-bit tables
// (AlphaMath.c mul8table and div8table): a pixel of alpha 0 becomes 0, an
// opaque one is copied, any other is premultiplied and divided back. With
// `lut`, a grey PNG with alpha's values first go through what Java2D makes
// of ImageIO's linear grey (GrayLut.java's second line).
fn java2d_over(rgba: &[u8], lut: Option<&[u8]>) -> Vec<u8> {
    fn mul8(a: u32, b: u32) -> u32 {
        if a == 0 || b == 0 {
            return 0;
        }
        let inc = (a << 16) + (a << 8) + a;
        ((inc as u64 * b as u64 + (1u64 << 23)) >> 24) as u32
    }
    fn div8(a: u32, b: u32) -> u32 {
        if b >= a {
            return 255;
        }
        let inc = ((0xffu64 << 24) + (a / 2) as u64) / a as u64;
        (((1u64 << 23) + inc * b as u64) >> 24) as u32
    }
    let mut out = rgba.to_vec();
    for p in out.as_chunks_mut::<4>().0 {
        if let Some(lut) = lut {
            let v = lut[p[0] as usize];
            p[0] = v;
            p[1] = v;
            p[2] = v;
        }
        let a = p[3] as u32;
        if a == 0 {
            p.copy_from_slice(&[0, 0, 0, 0]);
        } else if a < 255 {
            for c in &mut p[..3] {
                *c = div8(a, mul8(a, *c as u32)) as u8;
            }
        }
    }
    out
}

/// The bake in `out` against today's bake folder and mcmeta's atlas.
pub fn compare(out: &Path, today: &Path, mcmeta: &Path) -> Result<(), String> {
    let ours = json(&out.join("textures.json"))?;
    let theirs = json(&today.join("web/maps/live/textures.json"))?;
    let (ours, theirs) = (
        ours.as_array().ok_or("ours")?,
        theirs.as_array().ok_or("theirs")?,
    );
    if ours.len() != theirs.len() {
        return Err(format!(
            "{} materials against today's {}",
            ours.len(),
            theirs.len()
        ));
    }
    let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut examples = Vec::new();
    let mut count =
        |what: &'static str, same: bool, example: String, examples: &mut Vec<String>| {
            let c = counts.entry(what).or_default();
            if same {
                c.0 += 1
            } else {
                c.1 += 1;
                if examples.len() < 20 {
                    examples.push(example);
                }
            }
        };
    for (o, t) in ours.iter().zip(theirs) {
        let id = t["resourcePath"].as_str().unwrap_or_default().to_string();
        count(
            "resourcePath",
            o["resourcePath"] == t["resourcePath"],
            format!("path {id}"),
            &mut examples,
        );
        let (a, b) = (
            data_url_png(o["texture"].as_str().unwrap_or_default())?,
            data_url_png(t["texture"].as_str().unwrap_or_default())?,
        );
        count("pixels", a == b, format!("pixels {id}"), &mut examples);
        let bits = |v: &Value| -> Vec<u32> {
            v.as_array()
                .into_iter()
                .flatten()
                .map(|c| (c.as_f64().unwrap_or(f64::NAN) as f32).to_bits())
                .collect()
        };
        count(
            "color (f32 bits)",
            bits(&o["color"]) == bits(&t["color"]),
            format!("color {id} {} {}", o["color"], t["color"]),
            &mut examples,
        );
        count(
            "halfTransparent",
            o["halfTransparent"] == t["halfTransparent"],
            format!("half {id}"),
            &mut examples,
        );
        count(
            "animation",
            o.get("animation") == t.get("animation"),
            format!(
                "animation {id} {:?} {:?}",
                o.get("animation"),
                t.get("animation")
            ),
            &mut examples,
        );
        let strip = |v: &Value| {
            let mut v = v.clone();
            v.as_object_mut().map(|m| m.remove("texture"));
            v.to_string()
        };
        count(
            "entry without its image, as text",
            strip(o) == strip(t),
            format!("text {id}"),
            &mut examples,
        );
    }
    // The entity atlas as the renderer makes it from the client entries
    // (native/draw entity_atlas: raw values, first square frame), against
    // today's mcbench-entities.png.
    let capture = json(&today.join("web/mcbench-entities.json"))?;
    let (width, height) = (
        capture["atlas"]["width"].as_u64().ok_or("width")? as u32,
        capture["atlas"]["height"].as_u64().ok_or("height")? as u32,
    );
    let zip = read(&out.join("client-entries.zip"))?;
    let zip_entries: HashMap<String, jar::Entry> = jar::entries(&zip)?
        .into_iter()
        .map(|e| (e.name.clone(), e))
        .collect();
    let entry = |name: &str| -> Option<Vec<u8>> {
        let e = zip_entries.get(name)?;
        let stored = jar::stored(&zip, e);
        let mut bytes = Vec::new();
        if e.method == 8 {
            use std::io::Read;
            flate2::read::DeflateDecoder::new(stored)
                .read_to_end(&mut bytes)
                .ok()?;
        } else {
            bytes = stored.to_vec();
        }
        Some(bytes)
    };
    let today_atlas = decode_png(&read(&today.join("web/mcbench-entities.png"))?)?;
    let lut: Option<Vec<u8>> = match std::env::var("MCBENCH_GRAY_LUT") {
        Ok(file) => Some(
            String::from_utf8(read(Path::new(&file))?)
                .map_err(|e| e.to_string())?
                .lines()
                .nth(1)
                .ok_or("The grey table has no second line")?
                .split(',')
                .map(|v| v.parse::<u8>().map_err(|e| e.to_string()))
                .collect::<Result<_, _>>()?,
        ),
        Err(_) => None,
    };
    let mut causes: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut entity_same, mut entity_diff, mut entity_diff_grey, mut entity_grey) = (0, 0, 0, 0);
    let mut entity_examples = Vec::new();
    for (name, rect) in capture["atlas"]["rects"].as_object().ok_or("rects")? {
        let r: Vec<u32> = rect
            .as_array()
            .ok_or("rect")?
            .iter()
            .map(|v| v.as_u64().unwrap_or(0) as u32)
            .collect();
        let (space, path) = name.split_once(':').ok_or("namespace")?;
        let png_name = format!("assets/{space}/textures/{path}.png");
        let png_bytes =
            entry(&png_name).ok_or_else(|| format!("{png_name} is not in the client entries"))?;
        let image = decode_png(&png_bytes)?;
        let grey = grey_alpha(&png_bytes)?;
        entity_grey += usize::from(grey);
        let frame = &image.rgba[..(r[2] * r[3] * 4) as usize];
        let mut theirs = Vec::with_capacity(frame.len());
        for row in 0..r[3] {
            let at = (((r[1] + row) * today_atlas.width + r[0]) * 4) as usize;
            theirs.extend_from_slice(&today_atlas.rgba[at..at + (r[2] * 4) as usize]);
        }
        // What the renderer draws (the stored values) against today's atlas;
        // a texture that differs is put down to Java2D's round trip, or to
        // ImageIO's linear grey, only when that alone makes today's pixels.
        if frame == theirs.as_slice() {
            entity_same += 1;
        } else {
            entity_diff += 1;
            entity_diff_grey += usize::from(grey);
            let clear_only = frame
                .as_chunks::<4>()
                .0
                .iter()
                .zip(theirs.as_chunks::<4>().0)
                .all(|(a, b)| a == b || (a[3] == 0 && b[3] == 0));
            let cause = if clear_only {
                "only the colour of pixels of alpha 0, which Java2D clears (nothing drawn differs)"
            } else if java2d_over(frame, None) == theirs {
                "Java2D's premultiplied round trip of half-transparent and clear pixels"
            } else if grey
                && lut
                    .as_deref()
                    .is_some_and(|l| java2d_over(frame, Some(l)) == theirs)
            {
                "ImageIO reads a grey PNG with alpha or tRNS as linear grey (and the round trip)"
            } else {
                "not explained"
            };
            *causes.entry(cause).or_default() += 1;
            if entity_examples.len() < 8 {
                entity_examples.push(format!("{name} grey-with-alpha={grey}"));
            }
        }
    }
    // see_build's atlas against mcmeta's, per material box, at each level.
    let ours_map = boxes(&json(&out.join("atlas_map.json"))?)?;
    let theirs_map = boxes(&json(&mcmeta.join("atlas_map.json"))?)?;
    let ours_atlas = decode_png(&read(&out.join("atlas.png"))?)?;
    let theirs_atlas = decode_png(&read(&mcmeta.join("atlas.png"))?)?;
    let (our_side, our_levels) = levels(&ours_atlas);
    let (their_side, their_levels) = levels(&theirs_atlas);
    let key = |id: &str| -> String { id.strip_prefix("minecraft:").unwrap_or(id).to_string() };
    let materials: BTreeSet<String> = theirs
        .iter()
        .map(|t| key(t["resourcePath"].as_str().unwrap_or_default()))
        .collect();
    let mut presence = (0, 0);
    let mut per_level = [(0usize, 0usize); 5];
    let mut atlas_examples = Vec::new();
    for m in &materials {
        match (ours_map.get(m), theirs_map.get(m)) {
            (None, None) => presence.0 += 1,
            (Some(a), Some(b)) => {
                presence.0 += 1;
                for (level, slot) in per_level.iter_mut().enumerate() {
                    let same = a[2] == b[2]
                        && a[3] == b[3]
                        && region(&our_levels, our_side, level, *a)
                            == region(&their_levels, their_side, level, *b);
                    if same {
                        slot.0 += 1;
                    } else {
                        slot.1 += 1;
                        if atlas_examples.len() < 10 {
                            atlas_examples.push(format!("{m} level {level}"));
                        }
                    }
                }
            }
            _ => {
                presence.1 += 1;
                if atlas_examples.len() < 10 {
                    atlas_examples.push(format!("{m} in one atlas only"));
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "materials": ours.len(),
            "textures.json": counts.iter().map(|(k, (s, d))| (k.to_string(), serde_json::json!({ "same": s, "different": d }))).collect::<serde_json::Map<_, _>>(),
            "examples": examples,
            "entity atlas": { "textures": entity_same + entity_diff, "same": entity_same, "different": entity_diff, "different and grey with alpha or tRNS": entity_diff_grey, "grey with alpha or tRNS in all": entity_grey, "causes": causes, "size": [width, height], "examples": entity_examples },
            "see_build atlas against mcmeta's, by material": { "present in both or neither": presence.0, "present in one only": presence.1,
                "levels": per_level.iter().enumerate().map(|(l, (s, d))| serde_json::json!({ "level": l, "same": s, "different": d })).collect::<Vec<_>>(), "examples": atlas_examples },
        }))
        .map_err(|e| e.to_string())?
    );
    Ok(())
}

/// A first start on a new data folder, timed: the bundle unpacked, the
/// textures fetched from Mojang (at most `rate` bytes a second when given)
/// and baked.
pub fn first_start(
    bundle_file: &Path,
    lock_file: &Path,
    folder: &Path,
    rate: Option<f64>,
) -> Result<(), String> {
    if folder.exists() {
        return Err(format!(
            "{} exists; a first start needs a new data folder",
            folder.display()
        ));
    }
    let started = Instant::now();
    let lock = Lock::parse(&String::from_utf8(read(lock_file)?).map_err(|_| "lock")?)?;
    let bundle_bytes = read(bundle_file)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let marks: Arc<Mutex<Vec<(String, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let mark = |what: &str| {
        marks
            .lock()
            .expect("marks")
            .push((what.to_string(), started.elapsed().as_secs_f64()))
    };
    let fetched = runtime.block_on(async {
        let data = tokio::task::spawn_blocking({
            let folder = folder.to_path_buf();
            move || Data::unpack(&bundle_bytes, &lock, &folder)
        })
        .await
        .map_err(|e| e.to_string())??;
        mark("data unpacked");
        let client = fetch::client();
        let manifest = data.manifest()?;
        let mut options = Options::of(&manifest);
        options.rate = rate;
        let first = Mutex::new(false);
        let progress = |bytes: mcbench_setup::Bytes| {
            if bytes.done > 0 && !std::mem::replace(&mut *first.lock().expect("first"), true) {
                mark("first range done");
            }
            if Some(bytes.done) == bytes.total {
                mark("textures downloaded and checked");
            }
        };
        let textures = mcbench_setup::start::make_textures(
            folder,
            &data,
            &client,
            &options,
            &progress,
            &CancellationToken::new(),
        )
        .await?;
        mark("textures baked");
        let ranges = fetch::ranges(&manifest.entries, options.gap);
        Ok::<_, String>((
            textures,
            ranges.len(),
            ranges.iter().map(|r| r.end - r.start).sum::<u64>(),
        ))
    })?;
    let marks = marks.lock().expect("marks").clone();
    println!(
        "{}",
        serde_json::json!({ "requests": fetched.1, "bytes": fetched.2, "rate": rate, "marks": marks.iter().map(|(k, v)| serde_json::json!([k, (v * 1000.0).round() / 1000.0])).collect::<Vec<_>>() })
    );
    Ok(())
}
