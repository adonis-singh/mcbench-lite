// What Minecraft's client draws of an entity, from what its own entity
// renderers drew in the bake (server/McbenchEntities.java, written to
// mcbench-entities.json): web/entity-draw.mjs ported line for line, in f64
// as the page computes (Math.fround where it rounds to f32), and the
// placing and lighting of each entity's quads for see_build (`build`,
// `behind`). tests/entities.rs runs the page's module in Node on the same
// input and compares.
//
// An entity is the engine's (core/src/entity/mod.rs json), as
// server/shot-world.js keeps it: its type, id, position at the page's
// heights, yaw and pitch, and the data its look changes with.

use crate::scene::{light_curve, Instance};
use serde_json::Value;

pub type Mat = [f64; 16];

pub const IDENTITY: Mat = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

pub fn multiply(a: &Mat, b: &Mat) -> Mat {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + r] * b[c * 4 + k];
            }
            out[c * 4 + r] = sum;
        }
    }
    out
}

pub fn translation(x: f64, y: f64, z: f64) -> Mat {
    [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, y, z, 1.]
}

// A turn of `degrees` about the line through `point` along the unit `axis`.
pub fn rotation(axis: [f64; 3], point: [f64; 3], degrees: f64) -> Mat {
    let t = (degrees * std::f64::consts::PI) / 180.0;
    let (s, c) = (t.sin(), t.cos());
    let [x, y, z] = axis;
    let r = [
        [
            c + x * x * (1. - c),
            x * y * (1. - c) - z * s,
            x * z * (1. - c) + y * s,
        ],
        [
            y * x * (1. - c) + z * s,
            c + y * y * (1. - c),
            y * z * (1. - c) - x * s,
        ],
        [
            z * x * (1. - c) - y * s,
            z * y * (1. - c) + x * s,
            c + z * z * (1. - c),
        ],
    ];
    let mut m = [0.0; 16];
    m[15] = 1.0;
    for row in 0..3 {
        for col in 0..3 {
            m[col * 4 + row] = r[row][col];
        }
    }
    for row in 0..3 {
        m[12 + row] =
            point[row] - (r[row][0] * point[0] + r[row][1] * point[1] + r[row][2] * point[2]);
    }
    m
}

// ModelPart.translateAndRotate: x, y, z sixteenths, then
// Quaternionf.rotationZYX(zRot, yRot, xRot), then the scales.
pub fn part_matrix(p: &[f64]) -> Mat {
    let (cx, sx, cy, sy, cz, sz) = (
        p[3].cos(),
        p[3].sin(),
        p[4].cos(),
        p[4].sin(),
        p[5].cos(),
        p[5].sin(),
    );
    let r = [
        [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
        [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
        [-sy, cy * sx, cy * cx],
    ];
    let mut m = [
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        0.,
        p[0] / 16.,
        p[1] / 16.,
        p[2] / 16.,
        1.,
    ];
    for row in 0..3 {
        for col in 0..3 {
            m[col * 4 + row] = r[row][col] * p[6 + col];
        }
    }
    m
}

pub fn transform_point(m: &Mat, x: f64, y: f64, z: f64) -> [f64; 3] {
    [
        m[0] * x + m[4] * y + m[8] * z + m[12],
        m[1] * x + m[5] * y + m[9] * z + m[13],
        m[2] * x + m[6] * y + m[10] * z + m[14],
    ]
}

fn fr(v: f64) -> f64 {
    v as f32 as f64
}

// Mth.sin: the client's table of 65,536 sines, indexed by the angle times
// 10430.378 (as a float), masked.
pub fn mth_sin(value: f64) -> f64 {
    let index = (fr(fr(value) * fr(10430.378)).trunc() as i64) & 65535;
    fr((index as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin())
}

// AbstractMinecartRenderer.extractRenderState: the offset a minecart is
// drawn at by its id.
pub fn minecart_offset(id: i64) -> [f64; 3] {
    let mut seed = id.wrapping_mul(493286711);
    seed = seed
        .wrapping_mul(seed)
        .wrapping_mul(4392167121)
        .wrapping_add(seed.wrapping_mul(98761));
    let part =
        |shift: u32| fr(fr(fr(fr(((seed >> shift) & 7) as f64 + 0.5) / 8.0) - 0.5) * fr(0.004));
    [part(16), part(20), part(24)]
}

pub fn item_bob(age: f64, offset: f64) -> f64 {
    fr(fr(mth_sin(fr(age / 10.0) + offset) * 0.1) + 0.1)
}

pub fn item_spin(age: f64, offset: f64) -> f64 {
    fr(age / 20.0) + offset
}

pub fn orb_color(age: f64) -> [f64; 3] {
    let f = fr(age / 2.0);
    [
        ((mth_sin(f) + 1.0) * 0.5 * 255.0).trunc(),
        255.0,
        ((mth_sin(fr(f + 4.1887903)) + 1.0) * 0.1 * 255.0).trunc(),
    ]
}

// OverlayTexture's colour at an overlay: [r, g, b, kept].
pub fn overlay_color(overlay: Option<i64>) -> [f64; 4] {
    let Some(overlay) = overlay else {
        return [1.0; 4];
    };
    let overlay = overlay as u32;
    let (u, v) = ((overlay & 0xffff) as f64, overlay >> 16);
    if v < 8 {
        return [1.0, 0.0, 0.0, 1.0 - 178.0 / 255.0];
    }
    [
        1.0,
        1.0,
        1.0,
        ((1.0 - (u / 15.0) * 0.75) * 255.0).trunc() / 255.0,
    ]
}

// An item entity's bob offset: a fixed draw of the id.
pub fn bob_offset(id: i64) -> f64 {
    let id = id as i32 as u32;
    let mut h = id.wrapping_mul(0x9e3779b1);
    h = (h ^ (h >> 15)).wrapping_mul(0x85ebca6b);
    fr(((h ^ (h >> 13)) as f64 / 4294967296.0) * std::f64::consts::PI * 2.0)
}

fn base64(text: &str) -> Result<Vec<u8>, String> {
    let value = |c: u8| -> Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err("The entity capture holds a malformed base64 field".into()),
        } as u32)
    };
    let bytes = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

fn le_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn le_i32(bytes: &[u8]) -> Vec<i32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

// The capture (EntityData of the page).
pub struct Capture {
    pub json: Value,
    pub textures: Vec<String>,
    pub shapes: Vec<f32>,
    pub quads: Vec<i32>,
    pub mesh_offsets: Vec<i32>,
    pub poses: Vec<Mat>,
}

fn field<'a>(v: &'a Value, name: &str, what: &str) -> Result<&'a Value, String> {
    v.get(name)
        .ok_or_else(|| format!("The entity capture's {what} has no {name}"))
}

fn num(v: &Value) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("{v} is not a number"))
}

impl Capture {
    pub fn new(json: Value) -> Result<Capture, String> {
        let text = |name: &str| {
            field(&json, name, "file").and_then(|v| {
                v.as_str()
                    .ok_or_else(|| format!("The entity capture's {name} is not text"))
            })
        };
        let textures = field(&json, "textures", "file")?
            .as_array()
            .ok_or("The entity capture's textures are not a list")?
            .iter()
            .map(|t| t.as_str().unwrap_or("").to_string())
            .collect();
        let shapes = le_f32(&base64(text("shapes")?)?);
        let quads = le_i32(&base64(text("quads")?)?);
        let mesh_offsets = le_i32(&base64(text("meshes")?)?);
        let poses = le_f32(&base64(text("poses")?)?)
            .as_chunks::<16>()
            .0
            .iter()
            .map(|p| std::array::from_fn(|i| p[i] as f64))
            .collect();
        Ok(Capture {
            json,
            textures,
            shapes,
            quads,
            mesh_offsets,
            poses,
        })
    }

    pub fn kind(&self, kind: &str) -> Result<&Value, String> {
        self.json["kinds"]
            .get(kind)
            .ok_or_else(|| format!("No entity kind {kind} was captured"))
    }

    fn pose(&self, index: &Value) -> Result<&Mat, String> {
        let i = index.as_u64().ok_or("A pose index is not a whole number")? as usize;
        self.poses
            .get(i)
            .ok_or_else(|| format!("No pose {i} was captured"))
    }

    // Mesh `index`'s quads: (shape, texture, colour) each.
    fn mesh(&self, index: usize) -> Result<Vec<(usize, usize, i32)>, String> {
        let (from, to) = (
            self.mesh_offsets.get(index).copied(),
            self.mesh_offsets.get(index + 1).copied(),
        );
        let (Some(from), Some(to)) = (from, to) else {
            return Err(format!("No entity mesh {index} was captured"));
        };
        Ok((from..to)
            .map(|q| {
                (
                    self.quads[q as usize * 3] as usize,
                    self.quads[q as usize * 3 + 1] as usize,
                    self.quads[q as usize * 3 + 2],
                )
            })
            .collect())
    }
}

// String(value) as JavaScript writes it for the values entities hold.
fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            if f.fract() == 0.0 && f.abs() < 1e21 {
                format!("{}", f as i64)
            } else {
                format!("{f}")
            }
        }
        other => other.to_string(),
    }
}

fn entity_type(entity: &Value) -> Result<&str, String> {
    entity["type"]
        .as_str()
        .ok_or_else(|| "An entity has no type".to_string())
}

fn by_field(entity: &Value, name: &str) -> Result<String, String> {
    match entity.get(name) {
        None => Err(format!(
            "An entity of {} does not say its {name}",
            entity_type(entity)?
        )),
        Some(v) => Ok(js_string(v)),
    }
}

pub fn variant_of<'a>(kind: &'a Value, entity: &Value) -> Result<&'a Value, String> {
    let icons = kind.get("icons");
    let by: Vec<&str> = kind["by"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let mut values: Vec<String> = if icons.is_some() {
        Vec::new()
    } else {
        by.iter()
            .map(|name| by_field(entity, name))
            .collect::<Result<_, _>>()?
    };
    if let Some(fold) = kind.get("fold") {
        let at = by.iter().position(|n| Some(*n) == fold["value"].as_str());
        let from = num(&fold["from"])?;
        let period = num(&fold["period"])?;
        if let Some(at) = at {
            let n: f64 = values[at].parse().unwrap_or(f64::NAN);
            if n >= from {
                values[at] = js_string(&serde_json::json!(from + (n - from) % period));
            }
        }
    }
    if let Some(icons) = icons {
        let list = icons.as_array().ok_or("A kind's icons are not a list")?;
        let mut icon = list.first().ok_or("A kind has no icons")?[1].clone();
        let value = num(entity.get("value").unwrap_or(&Value::Null)).unwrap_or(f64::NAN);
        for pair in list {
            if value >= num(&pair[0])? {
                icon = pair[1].clone();
            }
        }
        values = vec![js_string(&icon)];
    }
    let key = values.join(",");
    kind["variants"].get(&key).ok_or_else(|| {
        format!(
            "No variant {key} of {} was captured",
            entity_type(entity).unwrap_or("?")
        )
    })
}

const POSE_PARTS: [&str; 6] = ["Head", "Body", "LeftArm", "RightArm", "LeftLeg", "RightLeg"];

// The values an entity's variant is drawn at, by name, in the order of the
// variant's `base`.
pub fn values_of(entity: &Value, variant: &Value) -> Result<Vec<(String, f64)>, String> {
    let ty = entity_type(entity)?;
    let base = variant["base"].as_object().ok_or("A variant has no base")?;
    let mut out = Vec::new();
    for name in base.keys() {
        let value = match name.as_str() {
            "yaw" | "pitch" | "offset" => entity.get(name.as_str()).and_then(Value::as_f64),
            _ => {
                let (part, axis) = name
                    .split_once('.')
                    .ok_or_else(|| format!("Unknown value {name}"))?;
                let pose = POSE_PARTS
                    .iter()
                    .position(|p| *p == part)
                    .and_then(|i| entity.get("pose").and_then(|p| p.get(i)));
                let Some(pose) = pose else {
                    return Err(format!("An entity of {ty} does not say its pose"));
                };
                "xyz"
                    .find(axis)
                    .and_then(|i| pose.get(i))
                    .and_then(Value::as_f64)
            }
        };
        match value {
            Some(v) if v.is_finite() => out.push((name.clone(), v)),
            _ => return Err(format!("An entity of {ty} has no {name}")),
        }
    }
    Ok(out)
}

fn get(values: &[(String, f64)], name: &str) -> f64 {
    values
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| *v)
        .unwrap_or(f64::NAN)
}

fn set(values: &mut Vec<(String, f64)>, name: &str, value: f64) {
    match values.iter_mut().find(|(n, _)| n == name) {
        Some(entry) => entry.1 = value,
        None => values.push((name.to_string(), value)),
    }
}

fn base_of(variant: &Value, name: &str) -> f64 {
    variant["base"][name].as_f64().unwrap_or(f64::NAN)
}

fn vec3(v: &Value) -> Option<[f64; 3]> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ])
}

fn motion_matrix(motion: &Value, amount: f64) -> Mat {
    match vec3(&motion["axis"]) {
        Some(axis) => rotation(
            axis,
            vec3(&motion["point"]).unwrap_or([0.0; 3]),
            motion["rate"].as_f64().unwrap_or(f64::NAN) * amount,
        ),
        None => {
            let v = vec3(&motion["vector"]).unwrap_or([f64::NAN; 3]);
            translation(v[0] * amount, v[1] * amount, v[2] * amount)
        }
    }
}

fn moved(motions: &Value, values: &[(String, f64)], variant: &Value, pose: &Mat) -> Mat {
    let mut m = IDENTITY;
    for motion in motions.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let name = motion["value"].as_str().unwrap_or("");
        m = multiply(
            &m,
            &motion_matrix(motion, get(values, name) - base_of(variant, name)),
        );
    }
    multiply(&m, pose)
}

// The motions every draw of a variant moves alike with, outermost first.
pub fn frame_motions(variant: &Value) -> Vec<Value> {
    let draws: Vec<&Value> = variant["draws"]
        .as_array()
        .map(|a| a.iter().filter(|d| d.get("attach").is_none()).collect())
        .unwrap_or_default();
    let empty = Vec::new();
    let first = draws
        .first()
        .and_then(|d| d["motions"].as_array())
        .unwrap_or(&empty);
    let same = |a: &Value, b: &Value| {
        a["value"] == b["value"]
            && a.get("axis") == b.get("axis")
            && a.get("vector") == b.get("vector")
            && match (vec3(&a["point"]), vec3(&b["point"])) {
                (None, _) => true,
                (Some(p), Some(q)) => (0..3).all(|i| (p[i] - q[i]).abs() < 1e-4),
                (Some(_), None) => false,
            }
            && (a["rate"].as_f64().unwrap_or(0.0) - b["rate"].as_f64().unwrap_or(0.0)).abs() < 1e-4
    };
    let mut out = Vec::new();
    for (i, motion) in first.iter().enumerate() {
        if !draws
            .iter()
            .all(|d| d["motions"].get(i).is_some_and(|m| same(m, motion)))
        {
            break;
        }
        let value = motion["value"].as_str().unwrap_or("");
        let changes = variant["draws"].as_array().is_some_and(|all| {
            all.iter().any(|d| {
                d["parts"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["deps"].get(value).is_some()))
            })
        });
        if changes {
            break;
        }
        out.push(motion.clone());
    }
    out
}

// Where an entity is drawn: its position, yaw, pitch and age (ticks).
#[derive(Clone, Copy, Debug)]
pub struct Place {
    pub pos: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub age: f64,
}

impl Place {
    pub fn of(entity: &Value) -> Result<Place, String> {
        let pos = vec3(&entity["pos"]).ok_or("An entity has no position")?;
        let n = |name: &str| entity.get(name).and_then(Value::as_f64).unwrap_or(f64::NAN);
        Ok(Place {
            pos,
            yaw: n("yaw"),
            pitch: n("pitch"),
            age: n("ticks"),
        })
    }
}

fn id_of(entity: &Value) -> i64 {
    entity["id"].as_i64().unwrap_or(0)
}

// The entity's frame at its place; `camera` is the view's turn for kinds
// drawn facing it.
pub fn frame(
    data: &Capture,
    entity: &Value,
    place: &Place,
    camera: Option<&Mat>,
) -> Result<Mat, String> {
    let ty = entity_type(entity)?;
    let kind = data.kind(ty)?;
    let mut m = translation(place.pos[0], place.pos[1], place.pos[2]);
    if ty.ends_with("minecart") {
        let own = minecart_offset(id_of(entity));
        let captured = minecart_offset(data.json["id"].as_i64().unwrap_or(0));
        m = multiply(
            &m,
            &translation(
                own[0] - captured[0],
                own[1] - captured[1],
                own[2] - captured[2],
            ),
        );
    }
    if ty == "minecraft:item" {
        let offset = bob_offset(id_of(entity));
        m = multiply(
            &m,
            &translation(0.0, item_bob(place.age, offset) - item_bob(0.0, 0.0), 0.0),
        );
        m = multiply(
            &m,
            &rotation(
                [0.0, 1.0, 0.0],
                [0.0; 3],
                (item_spin(place.age, offset) * 180.0) / std::f64::consts::PI,
            ),
        );
        return Ok(m);
    }
    let variant = variant_of(kind, entity)?;
    let mut values = values_of(entity, variant)?;
    set(&mut values, "yaw", place.yaw);
    set(&mut values, "pitch", place.pitch);
    for motion in frame_motions(variant) {
        let name = motion["value"].as_str().unwrap_or("");
        m = multiply(
            &m,
            &motion_matrix(&motion, get(&values, name) - base_of(variant, name)),
        );
    }
    if let (Some(billboard), Some(camera)) = (
        variant["draws"].get(0).and_then(|d| vec3(&d["billboard"])),
        camera,
    ) {
        let [bx, by, bz] = billboard;
        m = multiply(
            &m,
            &multiply(
                &translation(bx, by, bz),
                &multiply(camera, &translation(-bx, -by, -bz)),
            ),
        );
    }
    Ok(m)
}

fn kind_light(kind: &Value, [block, sky]: [i32; 2]) -> [f64; 2] {
    let at = |list: &Value, i: i32| {
        list.get(i as usize)
            .and_then(Value::as_f64)
            .unwrap_or(f64::NAN)
    };
    [
        at(&kind["light"]["block"], block),
        at(&kind["light"]["sky"], sky),
    ]
}

// The cells an entity's light is read from: its probe's, then a painting's
// own cell for each of its quads.
pub fn light_cells(data: &Capture, entity: &Value) -> Result<Vec<[i32; 3]>, String> {
    let ty = entity_type(entity)?;
    let kind = data.kind(ty)?;
    let probe = vec3(&kind["light"]["probe"]).ok_or("A kind has no light probe")?;
    let pos = vec3(&entity["pos"]).ok_or("An entity has no position")?;
    let mut cells = vec![[0, 1, 2].map(|i| (pos[i] + probe[i]).floor() as i32)];
    if ty == "minecraft:item" {
        return Ok(cells);
    }
    let base = pos.map(|v| v.floor() as i32);
    for draw in variant_of(kind, entity)?["draws"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        if let Some(steps) = draw["light"]["cells"].as_array() {
            for step in steps {
                let s = vec3(step).ok_or("A light step is not three numbers")?;
                cells.push([
                    base[0] + s[0] as i32,
                    base[1] + s[1] as i32,
                    base[2] + s[2] as i32,
                ]);
            }
        }
    }
    Ok(cells)
}

// A quad of an entity in its frame (geometry()'s output).
#[derive(Clone, Debug)]
pub struct EntityQuad {
    pub corners: [[f64; 3]; 4],
    pub uv: [f64; 8],
    pub texture: String,
    pub color: [f64; 4],
    pub overlay: [f64; 4],
    pub layer: String,
    pub light: [f64; 2],
    // The [block, sky] light Minecraft lights the quad with, from the cells'
    // own levels (View::levels), for the live view's night.
    pub levels: [f64; 2],
}

// A block state's own models' faces alone (loneQuads of the page, from the
// same mesher): corners, uv over the texture, texture name, colour.
#[derive(Clone, Debug)]
pub struct BlockQuad {
    pub corners: [f64; 12],
    pub uv: [f64; 8],
    pub texture: String,
    pub color: [f64; 4],
}

pub fn lone_quads(
    pack: &mcbench_renderer::pack::Pack,
    names: &[String],
    state: &str,
) -> Result<Vec<BlockQuad>, String> {
    let state = pack.named(state)?;
    let mut models = mcbench_renderer::tile::Tile::new();
    let mut entities = mcbench_renderer::tile::Tile::new();
    mcbench_renderer::mesh::lone(pack, state, 0, &mut models, &mut entities);
    if !models.size.is_multiple_of(2) {
        return Err(format!("{state} has a face that is not half of a quad"));
    }
    let mut out = Vec::new();
    for first in (0..models.size).step_by(2) {
        let mut corners = [0.0; 12];
        let mut uv = [0.0; 8];
        for (k, (half, c)) in [(0, 0), (0, 1), (0, 2), (1, 2)].into_iter().enumerate() {
            let face = first + half;
            for i in 0..3 {
                corners[k * 3 + i] = models.position[face * 9 + c * 3 + i] as f64;
            }
            uv[k * 2] = models.uv[face * 6 + c * 2] as f64;
            uv[k * 2 + 1] = models.uv[face * 6 + c * 2 + 1] as f64;
        }
        let m = models.material[first];
        out.push(BlockQuad {
            corners,
            uv,
            texture: names.get(m as usize).cloned().unwrap_or_default(),
            color: [
                models.color[first * 3] as f64,
                models.color[first * 3 + 1] as f64,
                models.color[first * 3 + 2] as f64,
                1.0,
            ],
        });
    }
    Ok(out)
}

// What geometry() reads of the world: the [block, sky] light of a cell and
// a block state's lone quads.
pub trait View {
    fn light(&self, cell: [i32; 3]) -> [i32; 2];
    // The cell's own [block, sky] light levels (see_build's `light` gives
    // the brighter of the two for both).
    fn levels(&self, cell: [i32; 3]) -> [i32; 2];
    fn block_quads(&self, state: &str) -> Result<Vec<BlockQuad>, String>;
}

// A quad's light and its levels (EntityQuad).
type Light = ([f64; 2], [f64; 2]);

enum Lit<'a> {
    Fixed(Light),
    Cells(&'a Value, [i32; 3]),
}

fn draw_light<'a>(
    kind: &Value,
    entity: &Value,
    draw: &'a Value,
    probe: Light,
) -> Result<Lit<'a>, String> {
    let light = &draw["light"];
    if light.as_str() == Some("entity") {
        return Ok(Lit::Fixed(probe));
    }
    if let Some(packed) = light.as_i64() {
        let l = [(packed >> 4 & 15) as f64, (packed >> 20 & 15) as f64];
        return Ok(Lit::Fixed((l, l)));
    }
    let _ = kind;
    let pos = vec3(&entity["pos"]).ok_or("An entity has no position")?;
    Ok(Lit::Cells(&light["cells"], pos.map(|v| v.floor() as i32)))
}

impl Lit<'_> {
    fn at(&self, kind: &Value, view: &dyn View, q: usize) -> Result<Light, String> {
        match self {
            Lit::Fixed(l) => Ok(*l),
            Lit::Cells(cells, base) => {
                let step = vec3(&cells[q]).ok_or("A painting quad has no light step")?;
                let cell = [
                    base[0] + step[0] as i32,
                    base[1] + step[1] as i32,
                    base[2] + step[2] as i32,
                ];
                Ok((
                    kind_light(kind, view.light(cell)),
                    kind_light(kind, view.levels(cell)),
                ))
            }
        }
    }
}

struct Out<'a> {
    data: &'a Capture,
    kind: &'a Value,
    view: &'a dyn View,
    quads: Vec<EntityQuad>,
}

impl Out<'_> {
    fn mesh(
        &mut self,
        mesh: &Value,
        pose: &Mat,
        layer: &Value,
        lit: &Lit,
        overlay: [f64; 4],
    ) -> Result<(), String> {
        let index = mesh.as_u64().ok_or("A mesh index is not a whole number")? as usize;
        for (q, (shape, texture, c)) in self.data.mesh(index)?.into_iter().enumerate() {
            let at = shape * 20;
            let s = &self.data.shapes;
            let corners = std::array::from_fn(|k| {
                transform_point(
                    pose,
                    s[at + k * 3] as f64,
                    s[at + k * 3 + 1] as f64,
                    s[at + k * 3 + 2] as f64,
                )
            });
            let c = c as u32;
            let light = lit.at(self.kind, self.view, q)?;
            self.quads.push(EntityQuad {
                corners,
                uv: std::array::from_fn(|i| s[at + 12 + i] as f64),
                texture: self.data.textures.get(texture).cloned().unwrap_or_default(),
                color: [
                    (c >> 16 & 255) as f64 / 255.0,
                    (c >> 8 & 255) as f64 / 255.0,
                    (c & 255) as f64 / 255.0,
                    (c >> 24 & 255) as f64 / 255.0,
                ],
                overlay,
                layer: layer.as_str().unwrap_or("").to_string(),
                light: light.0,
                levels: light.1,
            });
        }
        Ok(())
    }

    fn block(
        &mut self,
        state: &str,
        pose: &Mat,
        lit: &Lit,
        overlay: [f64; 4],
        special: bool,
    ) -> Result<(), String> {
        for quad in self.view.block_quads(state)? {
            let corners = std::array::from_fn(|k| {
                transform_point(
                    pose,
                    quad.corners[k * 3],
                    quad.corners[k * 3 + 1],
                    quad.corners[k * 3 + 2],
                )
            });
            let (light, levels) = lit.at(self.kind, self.view, 0)?;
            self.quads.push(EntityQuad {
                corners,
                uv: quad.uv,
                texture: quad.texture,
                color: quad.color,
                overlay,
                layer: "block".into(),
                light,
                levels,
            });
        }
        let block = state.split('[').next().unwrap_or(state);
        if special {
            if let Some(draws) = self.data.json["specials"][block].as_array() {
                for draw in draws {
                    let pose = multiply(pose, self.data.pose(&draw["pose"])?);
                    self.mesh(&draw["mesh"], &pose, &draw["layer"], lit, overlay)?;
                }
            }
        }
        Ok(())
    }

    fn item(
        &mut self,
        item: &str,
        context: &Value,
        pose: &Mat,
        lit: &Lit,
        overlay: [f64; 4],
    ) -> Result<(), String> {
        let own = self.data.json["items"]
            .get(item)
            .ok_or_else(|| format!("No item {item} was captured"))?;
        for draw in own[context.as_str().unwrap_or("")]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let pose = multiply(pose, self.data.pose(&draw["pose"])?);
            self.mesh(&draw["mesh"], &pose, &draw["layer"], lit, overlay)?;
        }
        Ok(())
    }

    // A draw's parts, each at its pose under its parents', under `pose`.
    fn parts(
        &self,
        draw: &Value,
        values: &[(String, f64)],
        variant: &Value,
        pose: &Mat,
    ) -> Result<Vec<(String, Mat)>, String> {
        let parts = draw["parts"]
            .as_array()
            .ok_or("A parts draw has no parts")?;
        let fields: Vec<(String, Vec<f64>)> = parts
            .iter()
            .map(|part| {
                let mut p: Vec<f64> = part["pose"]
                    .as_array()
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect())
                    .unwrap_or_default();
                if let Some(deps) = part["deps"].as_object() {
                    for (value, rate) in deps {
                        let change = get(values, value) - base_of(variant, value);
                        for (i, slot) in p.iter_mut().enumerate().take(9) {
                            *slot += rate[i].as_f64().unwrap_or(f64::NAN) * change;
                        }
                    }
                }
                (part["path"].as_str().unwrap_or("").to_string(), p)
            })
            .collect();
        let field = |path: &str| {
            fields
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, f)| f.as_slice())
                .ok_or_else(|| format!("No part {path}"))
        };
        let parent = |path: &str| {
            parts
                .iter()
                .find(|p| p["path"].as_str() == Some(path))
                .and_then(|p| p["parent"].as_str())
        };
        let mut out = Vec::new();
        for part in parts {
            let path = part["path"].as_str().unwrap_or("");
            let mut m = part_matrix(field(path)?);
            let mut at = part["parent"].as_str();
            while let Some(up) = at {
                m = multiply(&part_matrix(field(up)?), &m);
                at = parent(up);
            }
            out.push((path.to_string(), multiply(pose, &m)));
        }
        Ok(out)
    }
}

fn part_of<'a>(parts: &'a [(String, Mat)], path: &str) -> Result<&'a Mat, String> {
    parts
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, m)| m)
        .ok_or_else(|| format!("No part {path} to attach to"))
}

// The quads of an entity in its frame (entity-draw.mjs geometry).
pub fn geometry(
    data: &Capture,
    entity: &Value,
    view: &dyn View,
) -> Result<Vec<EntityQuad>, String> {
    let ty = entity_type(entity)?;
    let kind = data.kind(ty)?;
    let cell = light_cells(data, entity)?[0];
    let probe = (
        kind_light(kind, view.light(cell)),
        kind_light(kind, view.levels(cell)),
    );
    let mut out = Out {
        data,
        kind,
        view,
        quads: Vec::new(),
    };
    if ty == "minecraft:item" {
        let item = entity["item"]
            .as_str()
            .ok_or("An item entity has no item")?;
        let own = data.json["items"]
            .get(item)
            .ok_or_else(|| format!("No item {item} was captured"))?;
        let tiers = kind["tiers"]
            .as_array()
            .ok_or("The item kind has no tiers")?;
        let count = entity["count"].as_u64().unwrap_or(1) as usize;
        let copies = tiers[count.min(tiers.len()).max(1) - 1]
            .as_u64()
            .unwrap_or(1) as usize;
        for c in 0..copies {
            let mv = if c == 0 {
                IDENTITY
            } else {
                *data.pose(&own["copies"][c - 1])?
            };
            for draw in own["dropped"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                let pose = multiply(&mv, data.pose(&draw["pose"])?);
                out.mesh(
                    &draw["mesh"],
                    &pose,
                    &draw["layer"],
                    &Lit::Fixed(probe),
                    overlay_color(draw["overlay"].as_i64()),
                )?;
            }
        }
        return Ok(out.quads);
    }
    let variant = variant_of(kind, entity)?;
    let mut values = values_of(entity, variant)?;
    for motion in frame_motions(variant) {
        let name = motion["value"].as_str().unwrap_or("").to_string();
        set(&mut values, &name, base_of(variant, &name));
    }
    let draws = variant["draws"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut poses: Vec<(Mat, Vec<(String, Mat)>)> = Vec::new();
    for draw in draws {
        let pose = match draw.get("attach") {
            Some(attach) => {
                let d = attach["draw"]
                    .as_u64()
                    .ok_or("An attachment names no draw")? as usize;
                multiply(
                    part_of(
                        &poses.get(d).ok_or("An attachment names a later draw")?.1,
                        attach["part"].as_str().unwrap_or(""),
                    )?,
                    data.pose(&attach["matrix"])?,
                )
            }
            None => moved(
                &draw["motions"],
                &values,
                variant,
                data.pose(&draw["pose"])?,
            ),
        };
        let parts = if draw.get("parts").is_some() {
            out.parts(draw, &values, variant, &pose)?
        } else {
            Vec::new()
        };
        let lit = draw_light(kind, entity, draw, probe)?;
        let overlay = overlay_color(draw["overlay"].as_i64());
        match draw["kind"].as_str() {
            Some("parts") => {
                for (part, (_, m)) in draw["parts"].as_array().unwrap().iter().zip(&parts) {
                    out.mesh(&part["mesh"], m, &draw["layer"], &lit, overlay)?;
                }
            }
            Some("mesh") => {
                let from = out.quads.len();
                out.mesh(&draw["mesh"], &pose, &draw["layer"], &lit, overlay)?;
                if kind.get("icons").is_some() {
                    let [r, g, b] = orb_color(entity["ticks"].as_f64().unwrap_or(f64::NAN));
                    for q in &mut out.quads[from..] {
                        q.color = [r / 255.0, g / 255.0, b / 255.0, q.color[3]];
                    }
                }
            }
            Some(k @ ("block" | "moving")) => {
                let state = if draw["block"].as_str() == Some("entity") {
                    entity["block"].as_str()
                } else {
                    draw["block"].as_str()
                };
                let state =
                    state.ok_or_else(|| format!("An entity of {ty} does not say its block"))?;
                out.block(state, &pose, &lit, overlay, k == "block")?;
            }
            Some("item") => {
                let slot = draw["slot"].as_str().unwrap_or("");
                let item = if slot == "item" {
                    entity["item"].as_str()
                } else {
                    entity["equipment"][slot].as_str()
                };
                if let Some(item) = item {
                    let worn = || -> Result<bool, String> {
                        let table = kind
                            .get("equipment")
                            .ok_or_else(|| format!("{ty} has no equipment table"))?;
                        Ok(entity["equipment"]["head"].as_str().is_some_and(|i| {
                            table[i]["head"].is_object() || table[i]["head"].is_array()
                        }))
                    };
                    if !(slot == "head" && worn()?) {
                        out.item(item, &draw["context"], &pose, &lit, overlay)?;
                    }
                }
            }
            other => return Err(format!("Unknown entity draw kind {other:?}")),
        }
        poses.push((pose, parts));
    }
    if let (Some(table), Some(equipment)) = (kind.get("equipment"), entity["equipment"].as_object())
    {
        for (slot, item) in equipment {
            let Some(item) = item.as_str() else { continue };
            let Some(added) = table[item][slot][js_string(&entity["small"])].as_array() else {
                continue;
            };
            for draw in added {
                let pose = match draw.get("attach") {
                    Some(attach) => {
                        let d = attach["draw"]
                            .as_u64()
                            .ok_or("An attachment names no draw")?
                            as usize;
                        multiply(
                            part_of(
                                &poses.get(d).ok_or("An attachment names a missing draw")?.1,
                                attach["part"].as_str().unwrap_or(""),
                            )?,
                            data.pose(&attach["matrix"])?,
                        )
                    }
                    None => moved(
                        &draw["motions"],
                        &values,
                        variant,
                        data.pose(&draw["pose"])?,
                    ),
                };
                let lit = draw_light(kind, entity, draw, probe)?;
                let overlay = overlay_color(draw["overlay"].as_i64());
                if draw["kind"].as_str() == Some("parts") {
                    let parts = out.parts(draw, &values, variant, &pose)?;
                    for (part, (_, m)) in draw["parts"].as_array().unwrap().iter().zip(&parts) {
                        out.mesh(&part["mesh"], m, &draw["layer"], &lit, overlay)?;
                    }
                } else {
                    out.mesh(&draw["mesh"], &pose, &draw["layer"], &lit, overlay)?;
                }
            }
        }
    }
    Ok(out.quads)
}

// An entity quad placed in the world (buildEntities): its corners after
// the entity's frame, or, for a kind drawn facing the camera, in its frame
// still (placed for each shot).
#[derive(Clone, Debug)]
pub struct Placed {
    pub quad: EntityQuad,
    pub corners: [[f64; 3]; 4],
    pub entity: usize,
    pub place: Place,
}

#[derive(Default)]
pub struct Sorted {
    pub solid: Vec<Placed>,
    pub blended: Vec<Placed>,
    pub facing: Vec<Placed>,
}

// Every entity's quads placed, by how they are drawn (buildEntities).
pub fn build(data: &Capture, entities: &[Value], view: &dyn View) -> Result<Sorted, String> {
    let mut out = Sorted::default();
    for (index, entity) in entities.iter().enumerate() {
        let quads = geometry(data, entity, view)?;
        let facing = data.kind(entity_type(entity)?)?.get("icons").is_some();
        let place = Place::of(entity)?;
        let m = if facing {
            None
        } else {
            Some(frame(data, entity, &place, None)?)
        };
        for quad in quads {
            if quad.texture.is_empty() {
                continue;
            }
            let corners = match &m {
                Some(m) => quad.corners.map(|c| transform_point(m, c[0], c[1], c[2])),
                None => quad.corners,
            };
            let translucent = quad.layer.contains("translucent");
            let placed = Placed {
                quad,
                corners,
                entity: index,
                place,
            };
            if facing {
                out.facing.push(placed);
            } else if translucent {
                out.blended.push(placed);
            } else {
                out.solid.push(placed);
            }
        }
    }
    Ok(out)
}

// For one shot: the blended quads and the facing ones turned to the camera,
// back to front from `eye` (drawEntities).
pub fn behind(
    data: &Capture,
    entities: &[Value],
    sorted: &Sorted,
    yaw: f64,
    pitch: f64,
    eye: [f64; 3],
) -> Result<Vec<Placed>, String> {
    let turn = multiply(
        &crate::camera::rotate_y(-yaw),
        &crate::camera::rotate_x(-pitch),
    );
    let mut faced = Vec::with_capacity(sorted.facing.len());
    for p in &sorted.facing {
        let m = frame(data, &entities[p.entity], &p.place, Some(&turn))?;
        faced.push(Placed {
            corners: p
                .quad
                .corners
                .map(|c| transform_point(&m, c[0], c[1], c[2])),
            ..p.clone()
        });
    }
    let centre = |p: &Placed| {
        let c = &p.corners;
        let d = [0, 1, 2].map(|i| (c[0][i] + c[1][i] + c[2][i] + c[3][i]) / 4.0 - eye[i]);
        d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
    };
    let mut all: Vec<Placed> = sorted.blended.iter().cloned().chain(faced).collect();
    let keys: Vec<f64> = all.iter().map(centre).collect();
    let mut order: Vec<usize> = (0..all.len()).collect();
    order.sort_by(|&a, &b| keys[b].total_cmp(&keys[a]));
    let mut taken: Vec<Option<Placed>> = all.drain(..).map(Some).collect();
    Ok(order
        .into_iter()
        .map(|i| taken[i].take().unwrap())
        .collect())
}

// A placed quad as an instance over the entity atlas (uploadQuads): the
// colour times the light curve at the brighter of its two lights, the
// overlay as it is.
pub fn instance(p: &Placed, atlas: &crate::assets::Atlas) -> Result<Instance, String> {
    let b = atlas
        .boxes
        .get(&p.quad.texture)
        .ok_or_else(|| format!("The entity atlas has no texture {}", p.quad.texture))?;
    let side = atlas.side as f64;
    let (w, h) = ((b[2] - b[0]) as f64, (b[3] - b[1]) as f64);
    let uv = std::array::from_fn(|k| {
        [
            ((b[0] as f64 + p.quad.uv[k * 2] * w) / side) as f32,
            ((b[1] as f64 + p.quad.uv[k * 2 + 1] * h) / side) as f32,
        ]
    });
    let level = light_curve(p.quad.light[0].max(p.quad.light[1]) as f32);
    let c = p.quad.color;
    // Its light levels as a block corner's: [sky, block].
    let levels = [p.quad.levels[1] as f32, p.quad.levels[0] as f32];
    Ok(Instance::new(
        p.corners.map(|c| c.map(|v| v as f32)),
        uv,
        [level; 4],
        [c[0] as f32, c[1] as f32, c[2] as f32],
        p.quad.overlay.map(|v| v as f32),
        [levels; 4],
    ))
}
