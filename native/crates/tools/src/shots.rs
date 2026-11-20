//! `see_build`'s shots: `server/shot.js` `resolveShot` (views, eye and
//! target, region, clip, projection, fov, grid, size), `renderShots` (the
//! worlds a renderer keeps, its shots of them, in today's order of
//! requests), `montageCells` and `composeMontage`, over the ONE renderer of
//! the process ([`SharedRenderer`]: in the app the live view's
//! `mcbench_draw::live::Draw`, started once).
//!
//! The arithmetic is JavaScript's: `Math.sin`, `cos`, `tan`, `asin` and
//! `atan2` are fdlibm's (the `libm` crate, V8's `ieee754` for `tan`,
//! `asin` and `atan2`), `Math.hypot` V8's, `Math.max`, `Math.min` and
//! `Math.round` the operations ECMAScript names, so a resolved shot carries
//! today's numbers. One difference stays (`tests/math.rs` against
//! `tests/node/math.js`): V8's `Math.sin` and `Math.cos` are a port of
//! glibc's, which fdlibm's differ from by one unit in the last place for
//! about 1% of arguments; a named view's angles agree, so only a camera of
//! the model's own eye and target can carry such a difference.
//!
//! What a renderer failure answers is today's: a failure the renderer
//! reports is named by its step (`Screenshot world loading: ...`,
//! `Screenshot capture: ...`, `Screenshot initialization: ...` for a
//! renderer that could not start), a lost graphics device is retried once
//! and then answers today's lost-context sentence, a cancelled call answers
//! the abort's reason. Today's 120 s stall bound is the renderer's
//! (`RenderError::Stalled`, today's whole sentence).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mcbench_render::{
    Camera, Cell, Clip, Edge, Grid, Label, Lines, Ortho, PictureFormat, RenderError, Renderer,
    Shot, WorldUpload,
};
use mcbench_world::{CHUNK_SIZE, World, coords_of};
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;

use crate::js;
use crate::settle::SettledWorld;
use crate::verify::ABORTED;

/// The two sizes (`SIZES`), in their key order.
pub const SIZES: [(&str, [u32; 2]); 2] = [("large", [2184, 1640]), ("small", [1092, 820])];
/// Shots are drawn at twice their size and scaled down (`SUPERSAMPLE`).
pub const SUPERSAMPLE: u32 = 2;
/// JPEG quality (`JPEG_QUALITY`).
pub const JPEG_QUALITY: u8 = 90;
/// `DEFAULT_FOV`.
pub const DEFAULT_FOV: f64 = 70.0;
/// `MARGIN`.
pub const MARGIN: f64 = 1.06;
/// `VIEWS`: each view's yaw (the eye's compass position, 0 south, 90 east)
/// and pitch (degrees below the horizon), in their key order.
pub const VIEWS: [(&str, f64, f64); 9] = [
    ("iso_se", 45.0, 30.0),
    ("iso_sw", -45.0, 30.0),
    ("iso_ne", 135.0, 30.0),
    ("iso_nw", -135.0, 30.0),
    ("front", 0.0, 0.0),
    ("back", 180.0, 0.0),
    ("left", 90.0, 0.0),
    ("right", -90.0, 0.0),
    ("top", 0.0, 90.0),
];

/// The words a model has been given for a lost graphics device
/// (`LOST_DEVICE`).
pub const LOST_DEVICE: &str = "page.evaluate: Error: Screenshot WebGL context lost";
/// `currentRun`'s error once the renderer is shut down.
pub const SHUT_DOWN: &str = "The screenshot renderer was shut down";
/// The check of a reply's pictures.
pub const NO_JPEG: &str = "Screenshot renderer returned no JPEG image";

/// The names a plain object inherits from `Object.prototype`, which a
/// property lookup on `VIEWS` or `SIZES` finds: `__proto__` is an object,
/// the rest are functions.
const OBJECT_PROTOTYPE: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// `Math.max(a, b)`: NaN if either is, `+0` over `-0`.
fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else if a > b {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: NaN if either is, `-0` over `+0`.
fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else if a < b {
        a
    } else {
        b
    }
}

/// `Math.round` as V8 computes it: the ceiling, less one when it is more
/// than a half above.
fn js_round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let up = x.ceil();
    if up - 0.5 > x { up - 1.0 } else { up }
}

/// `Math.hypot` as V8 computes it: the largest magnitude, then Kahan's sum
/// of the squares of each over it.
pub fn hypot(values: &[f64]) -> f64 {
    let mut largest = 0.0f64;
    for &value in values {
        if value.is_infinite() {
            return f64::INFINITY;
        }
        if value.is_nan() {
            return f64::NAN;
        }
        largest = max(largest, value.abs());
    }
    if largest == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for &value in values {
        let n = value.abs() / largest;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * largest
}

/// `round`: `Math.round(value * 100) / 100`.
fn round(value: f64) -> f64 {
    js_round(value * 100.0) / 100.0
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn number(value: f64) -> Value {
    json!(value)
}

fn numbers(values: [f64; 3]) -> Value {
    Value::Array(values.iter().map(|&v| number(v)).collect())
}

/// A number's own value when it is one (`Number.isFinite`'s and the
/// arithmetic's reading of a JSON value).
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// `Object.keys(spec)`: array indices first, ascending, then the other keys
/// in the order they were written.
fn own_keys(spec: &Map<String, Value>) -> Vec<&String> {
    let index = |key: &str| -> Option<u32> {
        if key == "0" {
            return Some(0);
        }
        if key.starts_with('0') || key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
    };
    let mut indices: Vec<(u32, &String)> = spec
        .keys()
        .filter_map(|key| index(key).map(|n| (n, key)))
        .collect();
    indices.sort_by_key(|(n, _)| *n);
    let mut keys: Vec<&String> = indices.into_iter().map(|(_, key)| key).collect();
    keys.extend(spec.keys().filter(|key| index(key).is_none()));
    keys
}

/// `spec[name]` where `undefined` and `null` are `None` (`??`).
fn given<'a>(spec: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    spec.get(name).filter(|value| !value.is_null())
}

/// `point(value, name)`.
fn point(value: Option<&Value>, name: &str) -> Result<[f64; 3], String> {
    let error = || format!("{name} must be [x, y, z]");
    let Some(Value::Array(items)) = value else {
        return Err(error());
    };
    if items.len() != 3 {
        return Err(error());
    }
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = finite(item).ok_or_else(error)?;
    }
    Ok(out)
}

/// A box, inclusive: its lowest and highest corners.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BlockBox {
    min: [f64; 3],
    max: [f64; 3],
}

/// `blockBox(value, name)`.
fn block_box(value: &Value, name: &str) -> Result<BlockBox, String> {
    if !js::truthy(Some(value)) || !matches!(value, Value::Object(_) | Value::Array(_)) {
        return Err(format!("{name} must be {{from: [x, y, z], to: [x, y, z]}}"));
    }
    let a = point(value.get("from"), &format!("{name}.from"))?;
    let b = point(value.get("to"), &format!("{name}.to"))?;
    if !a.iter().chain(&b).all(|v| v.fract() == 0.0) {
        return Err(format!("{name} corners must be whole block coordinates"));
    }
    Ok(BlockBox {
        min: [0, 1, 2].map(|i| min(a[i], b[i])),
        max: [0, 1, 2].map(|i| max(a[i], b[i])),
    })
}

/// `intersect(a, b, name)`.
fn intersect(a: BlockBox, b: BlockBox, name: &str) -> Result<BlockBox, String> {
    let out = BlockBox {
        min: [0, 1, 2].map(|i| max(a.min[i], b.min[i])),
        max: [0, 1, 2].map(|i| min(a.max[i], b.max[i])),
    };
    if (0..3).any(|i| out.min[i] > out.max[i]) {
        return Err(format!(
            "Nothing to draw: {name} holds no blocks of the build"
        ));
    }
    Ok(out)
}

/// The world axis a camera axis lies along (`axisOf`): its index, sign and
/// name.
fn axis_of(vector: [f64; 3]) -> Option<(usize, f64, char)> {
    let index = vector.iter().position(|v| (v.abs() - 1.0).abs() < 1e-9)?;
    let v = vector[index];
    let sign = if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        v
    };
    Some((index, sign, ['x', 'y', 'z'][index]))
}

/// `gridFor`: pixel lines of block edges and labels of block coordinates.
fn grid_for(
    across: (usize, f64, char),
    down: (usize, f64, char),
    center: [f64; 3],
    half_width: f64,
    half_height: f64,
    width: f64,
    height: f64,
) -> Grid {
    let per_block = width / (2.0 * half_width);
    let step = [
        1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 500.0, 1000.0,
    ]
    .into_iter()
    .find(|value| value * per_block >= 36.0)
    .unwrap_or(1000.0);
    let lines = |(index, sign, _): (usize, f64, char), half: f64, size: f64, flip: f64| {
        let at = |b: f64| ((b - center[index]) * sign * flip / (2.0 * half) + 0.5) * size;
        let low = (center[index] - half).floor();
        let high = (center[index] + half).ceil();
        let mut edges = Vec::new();
        let mut labels = Vec::new();
        let mut b = low;
        while b <= high {
            if per_block >= 6.0 || b % step == 0.0 {
                edges.push(Edge {
                    at: at(b),
                    major: b % step == 0.0,
                });
            }
            if b % step == 0.0 {
                labels.push(Label {
                    at: at(b + 0.5),
                    text: mcbench_json::number_to_string(b),
                });
            }
            b += 1.0;
        }
        Lines { edges, labels }
    };
    let columns = lines(across, half_width, width, 1.0);
    let rows = lines(down, half_height, height, 1.0);
    let name = |(_, sign, axis): (usize, f64, char)| {
        format!("{}{axis}", if sign > 0.0 { '+' } else { '-' })
    };
    Grid {
        columns,
        rows,
        legend: format!(
            "{} →   {} ↓   grid: 1 block, labels every {}",
            name(across),
            name(down),
            mcbench_json::number_to_string(step)
        ),
        step,
    }
}

/// `framingPoints(world, box)`: the corners of each chunk's occupied box
/// within `box`.
fn framing_points(world: &World, region: BlockBox) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for (key, chunk) in world.chunks() {
        let origin = coords_of(key).map(|c| f64::from(c * CHUNK_SIZE));
        let local = chunk.bounds().expect("a chunk in a world holds blocks");
        let lo = [0, 1, 2].map(|i| max(origin[i] + f64::from(local[i]), region.min[i]));
        let hi = [0, 1, 2].map(|i| min(origin[i] + f64::from(local[i + 3]), region.max[i]) + 1.0);
        if (0..3).any(|i| lo[i] >= hi[i]) {
            continue;
        }
        for x in [lo[0], hi[0]] {
            for y in [lo[1], hi[1]] {
                for z in [lo[2], hi[2]] {
                    points.push([x, y, z]);
                }
            }
        }
    }
    points
}

/// `entityPoints(entities, box)`: the corners of each entity's box, cut to
/// `region` when given.
fn entity_points(entities: &[Value], region: Option<BlockBox>) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for entity in entities {
        let read = |value: Option<&Value>| {
            value
                .and_then(Value::as_f64)
                .expect("the engine writes each entity's pos, width and height")
        };
        let pos = entity.get("pos").and_then(Value::as_array);
        let at = |i: usize| read(pos.and_then(|p| p.get(i)));
        let (x, y, z) = (at(0), at(1), at(2));
        let width = read(entity.get("width"));
        let height = read(entity.get("height"));
        let mut lo = [x - width / 2.0, y, z - width / 2.0];
        let mut hi = [x + width / 2.0, y + height, z + width / 2.0];
        if let Some(region) = region {
            lo = [0, 1, 2].map(|i| max(lo[i], region.min[i]));
            hi = [0, 1, 2].map(|i| min(hi[i], region.max[i] + 1.0));
            if (0..3).any(|i| lo[i] > hi[i]) {
                continue;
            }
        }
        for px in [lo[0], hi[0]] {
            for py in [lo[1], hi[1]] {
                for pz in [lo[2], hi[2]] {
                    points.push([px, py, pz]);
                }
            }
        }
    }
    points
}

/// A shot as `resolveShot` resolves it: what the renderer draws and what
/// the model reads of it (`described`, in today's key order).
#[derive(Clone, Debug)]
pub struct Resolved {
    /// What the renderer draws.
    pub shot: Shot,
    /// `described`.
    pub described: Map<String, Value>,
}

/// `resolveShot(spec, world, entities)`: the camera of a shot of `world`
/// and its `entities` (the engine's list at the tick shot), with today's
/// sentences for every refusal.
pub fn resolve_shot(
    spec: &Map<String, Value>,
    world: &World,
    entities: &[Value],
) -> Result<Resolved, String> {
    resolve_shot_sized(spec, world, entities, &SIZES)
}

/// `resolveShot(spec, world, entities, sizes)`: [`resolve_shot`] with a
/// caller's own `sizes` in place of `see_build`'s (a shared build's square
/// preview, [`crate::share`]).
pub fn resolve_shot_sized(
    spec: &Map<String, Value>,
    world: &World,
    entities: &[Value],
    sizes: &[(&str, [u32; 2])],
) -> Result<Resolved, String> {
    let Some(bounds) = world.bounds() else {
        return Err("Nothing to render (empty world)".into());
    };
    let bounds = BlockBox {
        min: bounds.min.map(f64::from),
        max: bounds.max.map(f64::from),
    };
    const KNOWN: [&str; 9] = [
        "view",
        "eye",
        "target",
        "region",
        "clip",
        "projection",
        "fov",
        "grid",
        "size",
    ];
    for key in own_keys(spec) {
        if !KNOWN.contains(&key.as_str()) {
            return Err(format!("Unknown shot field \"{key}\""));
        }
    }
    let size = given(spec, "size").map_or_else(|| "large".to_string(), js::to_string);
    let [width, height] = match sizes.iter().find(|(name, _)| *name == size) {
        Some((_, dimensions)) => *dimensions,
        None if size == "__proto__" => {
            return Err(
                "object is not iterable (cannot read property Symbol(Symbol.iterator))".into(),
            );
        }
        None if OBJECT_PROTOTYPE.contains(&size.as_str()) => {
            return Err(
                "function is not iterable (cannot read property Symbol(Symbol.iterator))".into(),
            );
        }
        None => {
            return Err(format!(
                "size must be {}",
                sizes
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(" or ")
            ));
        }
    };
    let projection = match given(spec, "projection") {
        None => "perspective",
        Some(Value::String(p)) if p == "perspective" || p == "orthographic" => p.as_str(),
        Some(_) => return Err("projection must be perspective or orthographic".into()),
    };
    let ortho = projection == "orthographic";
    if ortho && spec.contains_key("fov") {
        return Err("fov is for perspective shots".into());
    }
    let fov = match given(spec, "fov") {
        None => DEFAULT_FOV,
        Some(value) => match finite(value) {
            Some(fov) if (5.0..=120.0).contains(&fov) => fov,
            _ => return Err("fov must be between 5 and 120 degrees".into()),
        },
    };
    let clip = match spec.get("clip") {
        Some(value) if js::truthy(Some(value)) => {
            Some(intersect(block_box(value, "clip")?, bounds, "clip")?)
        }
        _ => None,
    };
    let shown = clip.unwrap_or(bounds);
    let region_given = js::truthy(spec.get("region"));
    let region = if region_given {
        intersect(block_box(&spec["region"], "region")?, shown, "region")?
    } else {
        shown
    };

    let (yaw, pitch, mut target, mut distance);
    if spec.contains_key("eye") || spec.contains_key("target") {
        if spec.contains_key("view") {
            return Err("A shot takes a view or an eye and target, not both".into());
        }
        let eye = point(spec.get("eye"), "eye")?;
        let aim = point(spec.get("target"), "target")?;
        let d = [0, 1, 2].map(|i| aim[i] - eye[i]);
        distance = hypot(&d);
        if distance == 0.0 {
            return Err("eye and target must differ".into());
        }
        if !ortho && region_given {
            return Err(
                "region frames a view; a perspective shot from your own eye frames itself".into(),
            );
        }
        pitch = libm::asin(-d[1] / distance);
        yaw = libm::atan2(d[0], -d[2]);
        target = Some(aim);
    } else {
        let view = given(spec, "view").map_or_else(|| "iso_se".to_string(), js::to_string);
        let (view_yaw, view_pitch) = match VIEWS.iter().find(|(name, _, _)| *name == view) {
            Some(&(_, yaw, pitch)) => (yaw, pitch),
            // An inherited property: its `yaw` and `pitch` are undefined.
            None if OBJECT_PROTOTYPE.contains(&view.as_str()) => (f64::NAN, f64::NAN),
            None => {
                return Err(format!(
                    "Unknown view \"{view}\" (valid: {})",
                    VIEWS.map(|(name, _, _)| name).join(", ")
                ));
            }
        };
        yaw = -(view_yaw * std::f64::consts::PI) / 180.0;
        pitch = (view_pitch * std::f64::consts::PI) / 180.0;
        target = None;
        distance = 0.0;
    }
    let snap = |mut vector: [f64; 3]| {
        for v in &mut vector {
            if v.abs() < 1e-12 {
                *v = 0.0;
            }
        }
        vector
    };
    let (sy, cy, sp, cp) = (
        libm::sin(yaw),
        libm::cos(yaw),
        libm::sin(pitch),
        libm::cos(pitch),
    );
    let right = snap([cy, 0.0, sy]);
    let up = snap([sy * sp, cp, -cy * sp]);
    let forward = snap([cp * sy, -sp, -cp * cy]);
    let mut points = framing_points(world, region);
    if points.is_empty() {
        return Err("Nothing to draw: the framed box holds no blocks".into());
    }
    points.extend(entity_points(
        entities,
        if region_given { Some(region) } else { clip },
    ));
    let mut lowest = [f64::INFINITY; 3];
    let mut highest = [f64::NEG_INFINITY; 3];
    for p in &points {
        for i in 0..3 {
            if p[i] < lowest[i] {
                lowest[i] = p[i];
            }
            if p[i] > highest[i] {
                highest[i] = p[i];
            }
        }
    }
    let box_center = [0, 1, 2].map(|i| (lowest[i] + highest[i]) / 2.0);
    let offsets: Vec<[f64; 3]> = points
        .iter()
        .map(|p| {
            let d = [0, 1, 2].map(|i| p[i] - box_center[i]);
            [dot(d, right), dot(d, up), dot(d, forward)]
        })
        .collect();
    let range = |axis: usize| {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for o in &offsets {
            if o[axis] < low {
                low = o[axis];
            }
            if o[axis] > high {
                high = o[axis];
            }
        }
        (low, high)
    };
    let (w, h) = (f64::from(width), f64::from(height));
    let aspect = w / h;

    let camera;
    let mut described = Map::new();
    let mut grid = None;
    described.insert("projection".into(), json!(projection));
    if ortho {
        let ((r0, r1), (u0, u1), (f0, f1)) = (range(0), range(1), range(2));
        let center =
            [0, 1, 2].map(|i| box_center[i] + right[i] * (r0 + r1) / 2.0 + up[i] * (u0 + u1) / 2.0);
        let half_height = max((u1 - u0) / 2.0, (r1 - r0) / 2.0 / aspect) * MARGIN;
        let half_width = half_height * aspect;
        let depth = -f0 + 2.0;
        camera = Camera {
            yaw,
            pitch,
            focus: center,
            focus_dist: depth,
            fov: None,
            ortho: Some(Ortho {
                half_width,
                half_height,
                near: 0.05,
                far: depth + f1 + 2.0,
            }),
        };
        described.insert("looking".into(), numbers(forward.map(round)));
        described.insert("center".into(), numbers(center.map(round)));
        described.insert("widthBlocks".into(), number(round(2.0 * half_width)));
        described.insert("heightBlocks".into(), number(round(2.0 * half_height)));
        if js::truthy(spec.get("grid")) {
            let (Some(across), Some(down)) = (axis_of(right), axis_of(up.map(|v| -v))) else {
                return Err("grid needs a straight orthographic shot: view top, front, back, left or right, or an eye and target along one axis".into());
            };
            let made = grid_for(across, down, center, half_width, half_height, w, h);
            described.insert("grid".into(), json!(made.legend));
            grid = Some(made);
        }
    } else {
        if js::truthy(spec.get("grid")) {
            return Err("grid is for orthographic shots".into());
        }
        let tan_half_fov = libm::tan((fov / 2.0) * std::f64::consts::PI / 180.0);
        if target.is_none() {
            target = Some(box_center);
            let tan = tan_half_fov / MARGIN;
            distance = f64::NEG_INFINITY;
            for &[r, u, f] in &offsets {
                let nearest = max(max(u.abs() / tan, r.abs() / (tan * aspect)), 1.0);
                distance = max(distance, nearest - f);
            }
        }
        let target = target.expect("set above");
        camera = Camera {
            yaw,
            pitch,
            focus: target,
            focus_dist: distance,
            fov: Some(fov),
            ortho: None,
        };
        described.insert(
            "eye".into(),
            numbers([0, 1, 2].map(|i| round(target[i] - forward[i] * distance))),
        );
        described.insert("target".into(), numbers(target.map(round)));
        described.insert("fov".into(), number(fov));
    }
    let clip = clip.map(|clip| Clip {
        min: clip.min.map(|v| v as i32),
        max: clip.max.map(|v| v as i32),
    });
    if let Some(clip) = clip {
        described.insert("clip".into(), json!({"from": clip.min, "to": clip.max}));
    }
    Ok(Resolved {
        shot: Shot {
            width,
            height,
            camera,
            clip,
            grid,
        },
        described,
    })
}

/// `montageCells(count, side)`: the cells of a square card of `side`
/// pixels holding `count` shots, in order: one row for 1 or 2, else two,
/// the top one holding the larger half; each row's cells split the width
/// evenly.
pub fn montage_cells(count: usize, side: u32) -> Result<Vec<Cell>, String> {
    if count < 1 {
        return Err("A card holds at least one shot".into());
    }
    let rows: Vec<usize> = if count <= 2 {
        vec![count]
    } else {
        vec![count.div_ceil(2), count / 2]
    };
    let edge =
        |i: usize, parts: usize| js_round((i as f64 * f64::from(side)) / parts as f64) as u32;
    let mut cells = Vec::new();
    for (row, &columns) in rows.iter().enumerate() {
        for column in 0..columns {
            cells.push(Cell {
                x: edge(column, columns),
                y: edge(row, rows.len()),
                width: edge(column + 1, columns) - edge(column, columns),
                height: edge(row + 1, rows.len()) - edge(row, rows.len()),
            });
        }
    }
    Ok(cells)
}

/// Whether `bytes` start as a JPEG does (`isJpeg`).
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() > 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff
}

/// The renderer's state as the tools see it.
#[derive(Clone)]
pub enum Started {
    /// Not started yet: a call waits.
    Starting,
    /// Started.
    Ready(Arc<dyn Renderer>),
    /// It could not start: its sentence.
    Failed(String),
    /// Shut down: nothing draws again in this process.
    Closed,
}

/// A world a renderer was sent of a conversation: its name and which
/// edition of the build it is (the authored world's identity and revision).
#[derive(Clone, Debug, PartialEq)]
struct Sent {
    name: String,
    edition: (u64, i64),
}

/// The worlds the renderer was sent of a conversation: by tick, and the
/// last.
#[derive(Default)]
struct Conversation {
    ticks: HashMap<i64, Sent>,
    last: Option<Sent>,
}

/// What the calls share behind today's queue: the worlds sent, by
/// conversation.
#[derive(Default)]
struct Queue {
    sent: HashMap<String, Conversation>,
}

/// THE renderer of the process, shared by every tool call (today's one
/// helper and its queue): started once by whoever owns it (the app's live
/// view, `headless turn`, `headless shots`), waited for by a call that
/// comes first.
pub struct SharedRenderer {
    state: watch::Sender<Started>,
    queue: Arc<Mutex<Queue>>,
}

impl Default for SharedRenderer {
    fn default() -> SharedRenderer {
        SharedRenderer {
            state: watch::channel(Started::Starting).0,
            queue: Arc::default(),
        }
    }
}

/// A failure of a step: named by it, the helper's own failure (a lost
/// device, retried once), or as it is.
enum Failure {
    Restart,
    Message(String),
}

impl Failure {
    fn named(label: &str, error: RenderError) -> Failure {
        match error {
            RenderError::DeviceLost(_) => Failure::Restart,
            RenderError::Cancelled => Failure::Message(ABORTED.into()),
            // A renderer that could not start: today's `hello`, the step
            // `initialization` (a request can make the renderer again).
            RenderError::NoAdapter(text) => {
                Failure::Message(format!("Screenshot initialization: {text}"))
            }
            // Today's whole sentence (`stage`'s stalled).
            RenderError::Stalled(text) => Failure::Message(text),
            RenderError::UnknownWorld(text) | RenderError::Failed(text) => {
                Failure::Message(format!("Screenshot {label}: {text}"))
            }
        }
    }
}

/// One shot to draw: the resolved shot, its tick and the settle it shows.
pub struct ToDraw<'a> {
    /// The shot.
    pub shot: &'a Shot,
    /// Its tick.
    pub tick: i64,
    /// The settle drawn.
    pub settled: &'a Arc<SettledWorld>,
}

impl SharedRenderer {
    /// A renderer still to start.
    pub fn new() -> Arc<SharedRenderer> {
        Arc::new(SharedRenderer::default())
    }

    /// The renderer started, or the sentence of why it did not.
    pub fn set(&self, started: Result<Arc<dyn Renderer>, String>) {
        self.state.send_if_modified(|state| {
            if matches!(state, Started::Closed) {
                return false;
            }
            *state = match started {
                Ok(renderer) => Started::Ready(renderer),
                Err(error) => Started::Failed(error),
            };
            true
        });
    }

    /// Shut down: the renderer is let go (a call under way keeps it until
    /// it ends) and every later call answers [`SHUT_DOWN`].
    pub fn close(&self) {
        self.state.send_replace(Started::Closed);
    }

    /// The renderer once started: `startup`, then `initialization`.
    async fn renderer(&self, cancel: &CancellationToken) -> Result<Arc<dyn Renderer>, Failure> {
        let mut state = self.state.subscribe();
        let started = tokio::select! {
            started = state.wait_for(|state| !matches!(state, Started::Starting)) => started
                .expect("the renderer's state is kept by its owner")
                .clone(),
            _ = cancel.cancelled() => return Err(Failure::Message(ABORTED.into())),
        };
        match started {
            Started::Ready(renderer) => Ok(renderer),
            Started::Failed(error) => Err(Failure::Message(format!(
                "Screenshot initialization: {error}"
            ))),
            Started::Closed => Err(Failure::Message(SHUT_DOWN.into())),
            Started::Starting => unreachable!("waited for a start"),
        }
    }

    /// Runs `job` on the renderer off the runtime's workers; a cancel
    /// answers at once with the abort's reason (the job ends by itself).
    async fn blocking<T: Send + 'static>(
        renderer: &Arc<dyn Renderer>,
        cancel: &CancellationToken,
        job: impl FnOnce(&dyn Renderer) -> T + Send + 'static,
    ) -> Result<T, Failure> {
        let renderer = Arc::clone(renderer);
        let running = tokio::task::spawn_blocking(move || job(&*renderer));
        tokio::select! {
            done = running => Ok(done.expect("a renderer's request does not panic")),
            _ = cancel.cancelled() => Err(Failure::Message(ABORTED.into())),
        }
    }

    /// `renderShots(conversationId, shots, signal)`: the JPEG of each shot,
    /// in order. The shots of one settle are one request; a world the
    /// renderer does not keep is sent first (as an edit of the
    /// conversation's world sent before it, of the same tick when there is
    /// one), then the request again. A lost device is retried once with
    /// every world sent again; a cancel answers at once and cancels the
    /// renderer's request in hand.
    pub async fn render(
        &self,
        conversation: &str,
        shots: &[ToDraw<'_>],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<u8>>, String> {
        if cancel.is_cancelled() {
            return Err(ABORTED.into());
        }
        let mut queue = tokio::select! {
            queue = Arc::clone(&self.queue).lock_owned() => queue,
            _ = cancel.cancelled() => return Err(ABORTED.into()),
        };
        for tries in 0.. {
            match self.capture(&mut queue, conversation, shots, cancel).await {
                Ok(buffers) => return Ok(buffers),
                Err(Failure::Restart) if tries == 0 && !cancel.is_cancelled() => {
                    // A fresh helper holds no world.
                    queue.sent.clear();
                }
                Err(Failure::Restart) => return Err(LOST_DEVICE.into()),
                Err(Failure::Message(message)) => return Err(message),
            }
        }
        unreachable!("the loop returns")
    }

    async fn capture(
        &self,
        queue: &mut Queue,
        conversation: &str,
        shots: &[ToDraw<'_>],
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<u8>>, Failure> {
        if cancel.is_cancelled() {
            return Err(Failure::Message(ABORTED.into()));
        }
        // One world per settle: its shots in one request, in their order.
        let mut groups: Vec<(String, &ToDraw<'_>, Vec<usize>)> = Vec::new();
        for (index, shot) in shots.iter().enumerate() {
            match groups
                .iter_mut()
                .find(|(_, first, _)| Arc::ptr_eq(first.settled, shot.settled))
            {
                Some((_, _, indices)) => indices.push(index),
                None => groups.push((
                    format!(
                        "{conversation}/tick {}/{}",
                        shot.tick, shot.settled.scene.serial
                    ),
                    shot,
                    vec![index],
                )),
            }
        }
        let renderer = self.renderer(cancel).await?;
        let sent = queue.sent.entry(conversation.to_string()).or_default();
        let mut buffers: Vec<Option<Vec<u8>>> = vec![None; shots.len()];
        for (name, first, indices) in &groups {
            let drawn: Vec<Shot> = indices.iter().map(|&i| shots[i].shot.clone()).collect();
            let draw = |drawn: Vec<Shot>| {
                let name = name.clone();
                let flag = Arc::new(AtomicBool::new(false));
                let set = Arc::clone(&flag);
                let renderer = Arc::clone(&renderer);
                async move {
                    let asked = SharedRenderer::blocking(&renderer, cancel, move |renderer| {
                        renderer.shots(
                            &name,
                            &drawn,
                            SUPERSAMPLE,
                            PictureFormat::Jpeg(JPEG_QUALITY),
                            &set,
                        )
                    })
                    .await;
                    if asked.is_err() {
                        // A call cancelled under way has its job cancelled.
                        flag.store(true, Ordering::SeqCst);
                    }
                    asked
                }
            };
            let pictures = match draw(drawn.clone()).await? {
                Err(RenderError::UnknownWorld(_)) => {
                    send(&renderer, sent, name, first, cancel).await?;
                    draw(drawn).await?
                }
                other => other,
            }
            .map_err(|error| Failure::named("capture", error))?;
            if pictures.len() != indices.len() || !pictures.iter().all(|p| is_jpeg(&p.bytes)) {
                return Err(Failure::Message(NO_JPEG.into()));
            }
            for (&index, picture) in indices.iter().zip(pictures) {
                buffers[index] = Some(picture.bytes);
            }
        }
        Ok(buffers
            .into_iter()
            .map(|b| b.expect("every shot drawn"))
            .collect())
    }

    /// `composeMontage(tiles, cells, [width, height])`: one JPEG of the
    /// tiles placed in their cells.
    pub async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        if tiles.len() != cells.len() {
            return Err("A card needs one shot per cell".into());
        }
        let never = CancellationToken::new();
        let _queue = Arc::clone(&self.queue).lock_owned().await;
        let tiles = Arc::new(tiles);
        let cells = Arc::new(cells);
        for tries in 0.. {
            let attempt = async {
                let renderer = self.renderer(&never).await?;
                let (tiles, cells) = (Arc::clone(&tiles), Arc::clone(&cells));
                let card = SharedRenderer::blocking(&renderer, &never, move |renderer| {
                    let jpegs: Vec<&[u8]> = tiles.iter().map(Vec::as_slice).collect();
                    renderer.montage(size, &cells, &jpegs, JPEG_QUALITY)
                })
                .await?
                .map_err(|error| Failure::named("montage", error))?;
                if !is_jpeg(&card) {
                    return Err(Failure::Message(NO_JPEG.into()));
                }
                Ok(card)
            };
            match attempt.await {
                Ok(card) => return Ok(card),
                Err(Failure::Restart) if tries == 0 => {}
                Err(Failure::Restart) => return Err(LOST_DEVICE.into()),
                Err(Failure::Message(message)) => return Err(message),
            }
        }
        unreachable!("the loop returns")
    }
}

/// `send()`: the world of `first`'s settle kept under `name`, as an edit of
/// the conversation's world sent before it (of the same tick when there is
/// one), or whole when the renderer no longer holds that base.
async fn send(
    renderer: &Arc<dyn Renderer>,
    sent: &mut Conversation,
    name: &str,
    first: &ToDraw<'_>,
    cancel: &CancellationToken,
) -> Result<(), Failure> {
    let scene = &first.settled.scene;
    let base = sent.ticks.get(&first.tick).or(sent.last.as_ref()).cloned();
    let edit = base.filter(|base| base.name != name);
    let entities =
        mcbench_json::stringify(&Value::Array(first.settled.entities.clone())).into_bytes();
    let world = |edit: Option<&Sent>| {
        let upload = WorldUpload {
            name: name.to_string(),
            y_offset: scene.y_offset,
            blueprint: scene.blueprint.clone(),
            entities: Some(entities.clone()),
            base: edit.map(|edit| (edit.name.clone(), edit.edition == scene.edition)),
        };
        SharedRenderer::blocking(renderer, cancel, move |renderer| renderer.world(upload))
    };
    let reply = match world(edit.as_ref()).await? {
        Err(RenderError::UnknownWorld(_)) if edit.is_some() => world(None).await?,
        other => other,
    };
    reply.map_err(|error| Failure::named("world loading", error))?;
    // A base the renderer dropped is no one's base again.
    if let Some(edit) = &edit
        && edit.edition != scene.edition
    {
        sent.ticks.retain(|_, held| held.name != edit.name);
    }
    let held = Sent {
        name: name.to_string(),
        edition: scene.edition,
    };
    sent.ticks.insert(first.tick, held.clone());
    sent.last = Some(held);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcbench_world::BlockState;

    fn spec(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn two_blocks() -> World {
        let mut world = World::new();
        let stone = BlockState::new("minecraft:stone", &[]);
        world.set([0, 0, 0], stone.clone()).unwrap();
        world.set([3, 2, 1], stone).unwrap();
        world
    }

    #[test]
    fn javascript_arithmetic_is_kept() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert!(js_round(-0.4).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert!(min(0.0, -0.0).is_sign_negative());
        assert!(max(f64::NAN, 1.0).is_nan());
        assert_eq!(hypot(&[3.0, 4.0, 0.0]), 5.0);
        assert_eq!(
            own_keys(&spec(json!({"view": 1, "1": 2, "0": 3}))),
            ["0", "1", "view"]
        );
    }

    // Each case answered by today's resolveShot on the same two blocks
    // (node -e with server/shot.js, 2026-11-18).
    #[test]
    fn shots_resolve_as_resolve_shot_resolves_them() {
        let world = two_blocks();
        let resolved = |value: Value| match resolve_shot(&spec(value), &world, &[]) {
            Ok(r) => mcbench_json::stringify(&Value::Object(r.described)),
            Err(e) => format!("!! {e}"),
        };
        assert_eq!(
            resolved(json!({"view": ["top"]})),
            r#"{"projection":"perspective","eye":[2,5.27,1],"target":[2,1.5,1],"fov":70}"#
        );
        assert_eq!(
            resolved(json!({"view": 5})),
            r#"!! Unknown view "5" (valid: iso_se, iso_sw, iso_ne, iso_nw, front, back, left, right, top)"#
        );
        assert_eq!(
            resolved(json!({"size": "constructor"})),
            "!! function is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            resolved(json!({"size": "__proto__"})),
            "!! object is not iterable (cannot read property Symbol(Symbol.iterator))"
        );
        assert_eq!(
            resolved(json!({"size": ["small"]})),
            r#"{"projection":"perspective","eye":[4.85,3.83,3.85],"target":[2,1.5,1],"fov":70}"#
        );
        assert_eq!(
            resolved(json!({"1": 2, "view": "x"})),
            r#"!! Unknown shot field "1""#
        );
        assert_eq!(
            resolved(json!({"fov": "70"})),
            "!! fov must be between 5 and 120 degrees"
        );
        assert_eq!(
            resolved(json!({"clip": [1]})),
            "!! clip.from must be [x, y, z]"
        );
        assert_eq!(
            resolved(json!({"clip": "a"})),
            "!! clip must be {from: [x, y, z], to: [x, y, z]}"
        );
        assert_eq!(
            resolved(
                json!({"region": {"from": [0, 0, 0], "to": [0, 0, 0]}, "eye": [0, 0, 5], "target": [0, 0, 0]})
            ),
            "!! region frames a view; a perspective shot from your own eye frames itself"
        );
        assert_eq!(
            resolved(json!({"grid": 1})),
            "!! grid is for orthographic shots"
        );
        assert_eq!(
            resolved(json!({"projection": "orthographic", "grid": 1})),
            "!! grid needs a straight orthographic shot: view top, front, back, left or right, or an eye and target along one axis"
        );
        assert_eq!(
            resolved(json!({"projection": "orthographic", "grid": 1, "view": "top"})),
            r#"{"projection":"orthographic","looking":[0,-1,0],"center":[2,1.5,1],"widthBlocks":4.24,"heightBlocks":3.18,"grid":"+x →   +z ↓   grid: 1 block, labels every 1"}"#
        );
        let top = resolve_shot(
            &spec(json!({"projection": "orthographic", "grid": 1, "view": "top"})),
            &world,
            &[],
        )
        .unwrap();
        assert_eq!(
            mcbench_json::stringify(&serde_json::to_value(&top.shot.camera).unwrap()),
            r#"{"yaw":0,"pitch":1.5707963267948966,"focus":[2,1.5,1],"focusDist":3.5,"fov":null,"ortho":{"halfWidth":2.12,"halfHeight":1.591941391941392,"near":0.05,"far":7}}"#
        );
        let iso = resolve_shot(&spec(json!({})), &world, &[]).unwrap();
        // -(45 * PI) / 180, as today's resolveShot computes it.
        assert_eq!(iso.shot.camera.yaw, -std::f64::consts::FRAC_PI_4);
        assert_eq!(iso.shot.camera.focus_dist, 4.659315602037392);
    }

    #[test]
    fn a_card_is_laid_out_as_montage_cells_lays_it_out() {
        let cells = |n| {
            montage_cells(n, 1000)
                .unwrap()
                .iter()
                .map(|c| [c.x, c.y, c.width, c.height])
                .collect::<Vec<_>>()
        };
        assert_eq!(cells(1), [[0, 0, 1000, 1000]]);
        assert_eq!(
            cells(3),
            [[0, 0, 500, 500], [500, 0, 500, 500], [0, 500, 1000, 500]]
        );
        assert_eq!(cells(7)[3], [750, 0, 250, 500]);
        assert!(montage_cells(0, 1000).is_err());
    }
}
