// What "the same picture" is, as the renderer spike measured it
// (docs/native-app.md, The definition of the same picture): colour
// statistics and SSIM of luma between two pictures, coverage of the
// surfaces two geometries show, and quads compared as multisets. The
// thresholds are the report's.

use crate::scene::Quad;
use serde::Serialize;
use std::collections::HashMap;

// Same quads drawn by two rasterisers (and a backend against another).
pub const SAME_MEAN: f64 = 0.5;
pub const SAME_P99: f32 = 8.0;
pub const SAME_OVER_16: f64 = 0.005;
pub const SAME_SSIM: f64 = 0.995;
pub const BACKEND_MEAN: f64 = 0.5;
pub const BACKEND_P99: f32 = 4.0;
pub const COVERAGE_INTERIOR: f64 = 0.0005;

// The same across backends on the path models get: the decoded JPEGs at
// quality 90. A JPEG turns a difference of a level or two at a few pixels
// into a different rounding of whole 8x8 blocks' coefficients, so the same
// pictures differ more after it: over the verifier's 112 pictures (five
// fixed worlds, twelve community builds, every see_build call kind) the
// pictures of Metal, an RTX 5090 (Vulkan), SwiftShader and WARP agree
// within the pre-JPEG thresholds (mean at most 0.17, p99 at most 3), and
// their JPEGs at worst at mean 0.60, p99 10, over 16 levels 0.14% of
// pixels, SSIM 0.9957; each JPEG differs from its own source by mean 0.39
// to 2.51 and p99 2 to 16. Against those worst cases the thresholds are
// 1.7 times the mean, 1.2 times the p99, 2.1 times the share over 16 and
// 1.6 times SSIM's distance from 1; they still fail a wrong shader,
// matrix, mip level, blend or order (mean 1 to 6, p99 over 25).
pub const JPEG_MEAN: f64 = 1.0;
pub const JPEG_P99: f32 = 12.0;
pub const JPEG_OVER_16: f64 = 0.003;
pub const JPEG_SSIM: f64 = 0.993;

pub struct Image {
    pub w: usize,
    pub h: usize,
    // RGB as the JPEG holds it, 0..255.
    pub rgb: Vec<[f32; 3]>,
}

impl Image {
    pub fn from_rgb(rgb: &[u8], w: u32, h: u32) -> Image {
        Image {
            w: w as usize,
            h: h as usize,
            rgb: rgb
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
                .collect(),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Image, String> {
        let im = image::load_from_memory(bytes)
            .map_err(|e| e.to_string())?
            .to_rgb8();
        Ok(Image::from_rgb(im.as_raw(), im.width(), im.height()))
    }

    // A JPEG as decoded; a PNG with alpha as the drawing buffer the helper
    // reads back (its JPEG holds min(colour, alpha): picture::Canvas).
    pub fn open(path: &std::path::Path) -> Result<Image, String> {
        let im = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !im.color().has_alpha() {
            let im = im.to_rgb8();
            return Ok(Image::from_rgb(im.as_raw(), im.width(), im.height()));
        }
        let im = im.to_rgba8();
        let rgb: Vec<u8> = im
            .pixels()
            .flat_map(|p| [p.0[0].min(p.0[3]), p.0[1].min(p.0[3]), p.0[2].min(p.0[3])])
            .collect();
        Ok(Image::from_rgb(&rgb, im.width(), im.height()))
    }
}

fn luma(im: &Image) -> Vec<f32> {
    im.rgb
        .iter()
        .map(|p| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2])
        .collect()
}

// Separable Gaussian blur (sigma 1.5, 11 taps), edges clamped.
fn blur(v: &[f32], w: usize, h: usize) -> Vec<f32> {
    let k: Vec<f32> = (-5..=5)
        .map(|i: i32| (-(i * i) as f32 / (2.0 * 1.5 * 1.5)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    let k: Vec<f32> = k.iter().map(|x| x / sum).collect();
    let mut tmp = vec![0.0; v.len()];
    for y in 0..h {
        for x in 0..w {
            tmp[y * w + x] = k
                .iter()
                .enumerate()
                .map(|(i, kk)| {
                    kk * v[y * w + (x as i32 + i as i32 - 5).clamp(0, w as i32 - 1) as usize]
                })
                .sum();
        }
    }
    let mut out = vec![0.0; v.len()];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = k
                .iter()
                .enumerate()
                .map(|(i, kk)| {
                    kk * tmp[(y as i32 + i as i32 - 5).clamp(0, h as i32 - 1) as usize * w + x]
                })
                .sum();
        }
    }
    out
}

// SSIM of luma per pixel (Wang et al. 2004, 8-bit constants).
fn ssim_map(a: &Image, b: &Image) -> Vec<f32> {
    let (w, h) = (a.w, a.h);
    let (x, y) = (luma(a), luma(b));
    let mx = blur(&x, w, h);
    let my = blur(&y, w, h);
    let xx = blur(&x.iter().map(|v| v * v).collect::<Vec<_>>(), w, h);
    let yy = blur(&y.iter().map(|v| v * v).collect::<Vec<_>>(), w, h);
    let xy = blur(
        &x.iter().zip(&y).map(|(a, b)| a * b).collect::<Vec<_>>(),
        w,
        h,
    );
    let (c1, c2) = ((0.01f32 * 255.0).powi(2), (0.03f32 * 255.0).powi(2));
    (0..w * h)
        .map(|i| {
            let (sx, sy, sxy) = (
                xx[i] - mx[i] * mx[i],
                yy[i] - my[i] * my[i],
                xy[i] - mx[i] * my[i],
            );
            ((2.0 * mx[i] * my[i] + c1) * (2.0 * sxy + c2))
                / ((mx[i] * mx[i] + my[i] * my[i] + c1) * (sx + sy + c2))
        })
        .collect()
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Stats {
    pub pixels: usize,
    pub mean_abs: f64,
    pub p50: f32,
    pub p99: f32,
    pub max: f32,
    pub over_8: f64,
    pub over_16: f64,
    pub over_32: f64,
    pub ssim: f64,
}

impl Stats {
    // Within the same-quads thresholds.
    pub fn same(&self) -> bool {
        self.mean_abs <= SAME_MEAN
            && self.p99 <= SAME_P99
            && self.over_16 <= SAME_OVER_16
            && self.ssim >= SAME_SSIM
    }

    // Within the across-backends thresholds for decoded JPEGs.
    pub fn across_backends_jpeg(&self) -> bool {
        self.mean_abs <= JPEG_MEAN
            && self.p99 <= JPEG_P99
            && self.over_16 <= JPEG_OVER_16
            && self.ssim >= JPEG_SSIM
    }

    // Within the across-backends thresholds.
    pub fn across_backends(&self) -> bool {
        self.mean_abs <= BACKEND_MEAN && self.p99 <= BACKEND_P99
    }
}

// Per-pixel differences of two pictures of one size, over the pixels
// `pick` keeps (all for None).
pub fn colour(a: &Image, b: &Image, pick: Option<&[bool]>) -> Result<Stats, String> {
    if (a.w, a.h) != (b.w, b.h) {
        return Err(format!(
            "Pictures of different sizes: {}x{} and {}x{}",
            a.w, a.h, b.w, b.h
        ));
    }
    let ssim = ssim_map(a, b);
    let mut maxes = Vec::with_capacity(a.rgb.len());
    let (mut sum, mut ssim_sum) = (0f64, 0f64);
    for i in 0..a.rgb.len() {
        if pick.is_some_and(|p| !p[i]) {
            continue;
        }
        let d = [0, 1, 2].map(|c| (a.rgb[i][c] - b.rgb[i][c]).abs());
        sum += (d[0] + d[1] + d[2]) as f64 / 3.0;
        maxes.push(d[0].max(d[1]).max(d[2]));
        ssim_sum += ssim[i] as f64;
    }
    let n = maxes.len().max(1);
    maxes.sort_by(|x, y| x.total_cmp(y));
    let frac = |t: f32| maxes.iter().filter(|&&m| m > t).count() as f64 / n as f64;
    let pct = |p: f64| {
        maxes
            .get(((maxes.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    Ok(Stats {
        pixels: maxes.len(),
        mean_abs: sum / n as f64,
        p50: pct(0.5),
        p99: pct(0.99),
        max: maxes.last().copied().unwrap_or(0.0),
        over_8: frac(8.0),
        over_16: frac(16.0),
        over_32: frac(32.0),
        ssim: ssim_sum / n as f64,
    })
}

// --- coverage ---

fn same_surface(a: [f32; 4], b: [f32; 4]) -> bool {
    match (a[3] > 0.0, b[3] > 0.0) {
        (false, false) => true,
        (true, true) => a[1] == b[1] && (a[0] - b[0]).abs() <= 0.02 + 1e-3 * a[0].abs(),
        _ => false,
    }
}

// Pixels whose 3x3 neighbourhood shows one surface.
pub fn interior(s: &[[f32; 4]], w: usize, h: usize) -> Vec<bool> {
    let mut out = vec![false; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 1..w.saturating_sub(1) {
            let c = s[y * w + x];
            out[y * w + x] = (-1i32..=1).all(|dy| {
                (-1i32..=1).all(|dx| {
                    same_surface(
                        c,
                        s[(y as i32 + dy) as usize * w + (x as i32 + dx) as usize],
                    )
                })
            });
        }
    }
    out
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Coverage {
    pub pixels: usize,
    pub covered_a: usize,
    pub covered_b: usize,
    pub differ: f64,
    pub interior_pixels: usize,
    pub differ_interior: f64,
}

pub fn coverage(a: &[[f32; 4]], b: &[[f32; 4]], w: usize, h: usize) -> Coverage {
    let (ia, ib) = (interior(a, w, h), interior(b, w, h));
    let mut c = Coverage {
        pixels: w * h,
        ..Default::default()
    };
    let (mut differ, mut differ_inner) = (0usize, 0usize);
    for i in 0..w * h {
        let d = !same_surface(a[i], b[i]);
        differ += d as usize;
        c.covered_a += (a[i][3] > 0.0) as usize;
        c.covered_b += (b[i][3] > 0.0) as usize;
        if ia[i] && ib[i] {
            c.interior_pixels += 1;
            differ_inner += d as usize;
        }
    }
    c.differ = differ as f64 / (w * h).max(1) as f64;
    c.differ_interior = differ_inner as f64 / c.interior_pixels.max(1) as f64;
    c
}

// --- geometry ---

const Q: f32 = 4096.0;

fn qp(p: [f32; 3]) -> [i32; 3] {
    p.map(|v| (v * Q).round() as i32)
}

// A quad's place: its corners as a sorted set (to 1/4096 of a block), and
// which way it faces.
type Place = ([[i32; 3]; 4], [i32; 3]);

// Everything compared of a quad at its place, by sorted corner.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct Canon {
    pub texture: String,
    pub uv: [[i32; 2]; 4],
    pub shade: [i32; 4],
    pub tint: [i32; 3],
    pub transparent: bool,
    pub kind: u8,
}

fn canon(q: &Quad, names: &[String]) -> (Place, Canon) {
    let mut idx = [0usize, 1, 2, 3];
    idx.sort_by_key(|&i| qp(q.corners[i]));
    let mut corners = q.corners.map(qp);
    corners.sort();
    (
        (corners, q.normal.map(|v| (v * 64.0).round() as i32)),
        Canon {
            texture: names.get(q.texture as usize).cloned().unwrap_or_default(),
            uv: idx.map(|i| q.uv[i].map(|v| (v * 256.0).round() as i32)),
            shade: idx.map(|i| (q.shade[i] * 256.0).round() as i32),
            tint: q.tint.map(|v| (v * 255.0).round() as i32),
            transparent: q.transparent,
            kind: q.kind,
        },
    )
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Geometry {
    pub a: usize,
    pub b: usize,
    // Quads at the same place in both (as multisets).
    pub matched: usize,
    // Of those, the same in every field compared.
    pub identical: usize,
    pub only_a: usize,
    pub only_b: usize,
}

impl Geometry {
    pub fn equal(&self) -> bool {
        self.a == self.b && self.identical == self.a
    }
}

// Two sets of quads as multisets: matched by place, then compared on
// texture, uv, shade, tint, layer and kind (shade and uv to 1/256, tint to
// 1/255).
pub fn geometry(a: &[Quad], a_names: &[String], b: &[Quad], b_names: &[String]) -> Geometry {
    let mut by_place: HashMap<Place, Vec<Canon>> = HashMap::new();
    for q in b {
        let (p, c) = canon(q, b_names);
        by_place.entry(p).or_default().push(c);
    }
    let mut g = Geometry {
        a: a.len(),
        b: b.len(),
        ..Default::default()
    };
    for q in a {
        let (p, c) = canon(q, a_names);
        let Some(list) = by_place.get_mut(&p) else {
            g.only_a += 1;
            continue;
        };
        let pick = list
            .iter()
            .position(|d| *d == c)
            .or_else(|| list.iter().position(|d| d.texture == c.texture))
            .unwrap_or(0);
        let d = list.swap_remove(pick);
        if list.is_empty() {
            by_place.remove(&p);
        }
        g.matched += 1;
        g.identical += (d == c) as usize;
    }
    g.only_b = by_place.values().map(Vec::len).sum();
    g
}
