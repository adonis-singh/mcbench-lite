// see_build's camera (server/shot.js resolveShot's camera: its view and
// projection, `frame`), with gl-matrix's matrices: column-major, OpenGL clip
// space, in f64 as the page computes them. The GPU side maps OpenGL's depth
// range to wgpu's (shader.wgsl).

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Ortho {
    #[serde(rename = "halfWidth")]
    pub half_width: f64,
    #[serde(rename = "halfHeight")]
    pub half_height: f64,
    pub near: f64,
    pub far: f64,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub focus: [f64; 3],
    #[serde(rename = "focusDist")]
    pub focus_dist: f64,
    #[serde(default)]
    pub fov: Option<f64>,
    #[serde(default)]
    pub ortho: Option<Ortho>,
}

pub type Mat4 = [f64; 16];

pub const MAX_CAMERA_DISTANCE: f64 = 12000.0;

pub fn identity() -> Mat4 {
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

// out = a * b (column-major, as gl-matrix's mat4.multiply)
pub fn multiply(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

pub fn rotate_x(angle: f64) -> Mat4 {
    let (s, c) = angle.sin_cos();
    let mut m = identity();
    m[5] = c;
    m[6] = s;
    m[9] = -s;
    m[10] = c;
    m
}

pub fn rotate_y(angle: f64) -> Mat4 {
    let (s, c) = angle.sin_cos();
    let mut m = identity();
    m[0] = c;
    m[2] = -s;
    m[8] = s;
    m[10] = c;
    m
}

pub fn translate(v: [f64; 3]) -> Mat4 {
    let mut m = identity();
    m[12] = v[0];
    m[13] = v[1];
    m[14] = v[2];
    m
}

pub fn perspective(fovy: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let f = 1.0 / (fovy / 2.0).tan();
    let nf = 1.0 / (near - far);
    let mut m = [0.0; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) * nf;
    m[11] = -1.0;
    m[14] = 2.0 * far * near * nf;
    m
}

pub fn ortho(l: f64, r: f64, b: f64, t: f64, n: f64, f: f64) -> Mat4 {
    let lr = 1.0 / (l - r);
    let bt = 1.0 / (b - t);
    let nf = 1.0 / (n - f);
    let mut m = [0.0; 16];
    m[0] = -2.0 * lr;
    m[5] = -2.0 * bt;
    m[10] = 2.0 * nf;
    m[12] = (l + r) * lr;
    m[13] = (t + b) * bt;
    m[14] = (f + n) * nf;
    m[15] = 1.0;
    m
}

pub fn invert(a: &Mat4) -> Mat4 {
    let (a00, a01, a02, a03) = (a[0], a[1], a[2], a[3]);
    let (a10, a11, a12, a13) = (a[4], a[5], a[6], a[7]);
    let (a20, a21, a22, a23) = (a[8], a[9], a[10], a[11]);
    let (a30, a31, a32, a33) = (a[12], a[13], a[14], a[15]);
    let b00 = a00 * a11 - a01 * a10;
    let b01 = a00 * a12 - a02 * a10;
    let b02 = a00 * a13 - a03 * a10;
    let b03 = a01 * a12 - a02 * a11;
    let b04 = a01 * a13 - a03 * a11;
    let b05 = a02 * a13 - a03 * a12;
    let b06 = a20 * a31 - a21 * a30;
    let b07 = a20 * a32 - a22 * a30;
    let b08 = a20 * a33 - a23 * a30;
    let b09 = a21 * a32 - a22 * a31;
    let b10 = a21 * a33 - a23 * a31;
    let b11 = a22 * a33 - a23 * a32;
    let det = 1.0 / (b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06);
    [
        (a11 * b11 - a12 * b10 + a13 * b09) * det,
        (a02 * b10 - a01 * b11 - a03 * b09) * det,
        (a31 * b05 - a32 * b04 + a33 * b03) * det,
        (a22 * b04 - a21 * b05 - a23 * b03) * det,
        (a12 * b08 - a10 * b11 - a13 * b07) * det,
        (a00 * b11 - a02 * b08 + a03 * b07) * det,
        (a32 * b02 - a30 * b05 - a33 * b01) * det,
        (a20 * b05 - a22 * b02 + a23 * b01) * det,
        (a10 * b10 - a11 * b08 + a13 * b06) * det,
        (a01 * b08 - a00 * b10 - a03 * b06) * det,
        (a30 * b04 - a31 * b02 + a33 * b00) * det,
        (a21 * b02 - a20 * b04 - a23 * b00) * det,
        (a11 * b07 - a10 * b09 - a12 * b06) * det,
        (a00 * b09 - a01 * b07 + a02 * b06) * det,
        (a31 * b01 - a30 * b03 - a32 * b00) * det,
        (a20 * b03 - a21 * b01 + a22 * b00) * det,
    ]
}

pub struct Frame {
    pub view: Mat4,
    pub proj: Mat4,
    // The sky's: inverse(projection * rotation), a perspective lens of 70
    // degrees for an orthographic shot.
    pub sky_unproject: Mat4,
    pub ortho: bool,
    pub eye: [f64; 3],
}

impl Camera {
    pub fn forward(&self) -> [f64; 3] {
        let cp = self.pitch.cos();
        [cp * self.yaw.sin(), -self.pitch.sin(), -cp * self.yaw.cos()]
    }

    // A camera is perspective (a fov) or orthographic (a box), not both.
    pub fn check(&self) -> Result<(), String> {
        match (&self.fov, &self.ortho) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            _ => Err("A shot's camera has a fov (perspective) or an ortho box, one of the two".into()),
        }
    }

    pub fn frame(&self, aspect: f64) -> Frame {
        let f = self.forward();
        let eye = [0, 1, 2].map(|i| self.focus[i] - f[i] * self.focus_dist);
        let rot = multiply(&rotate_x(self.pitch), &rotate_y(self.yaw));
        let view = multiply(&rot, &translate([-eye[0], -eye[1], -eye[2]]));
        let persp = |fov: f64| perspective(fov * std::f64::consts::PI / 180.0, aspect, 0.1, MAX_CAMERA_DISTANCE);
        let (proj, sky_proj) = match &self.ortho {
            Some(o) => (ortho(-o.half_width, o.half_width, -o.half_height, o.half_height, o.near, o.far), persp(70.0)),
            None => {
                let p = persp(self.fov.expect("checked: a perspective camera has a fov"));
                (p, p)
            }
        };
        Frame { view, proj, sky_unproject: invert(&multiply(&sky_proj, &rot)), ortho: self.ortho.is_some(), eye }
    }
}

// see_build's fog: its start and end from the build's largest extent.
pub fn fog_range(bounds_min: [i32; 3], bounds_max: [i32; 3]) -> [f32; 2] {
    let size = (0..3).map(|i| bounds_max[i] - bounds_min[i]).max().unwrap() as f64 + 1.0;
    let start = (size * 3.0).clamp(90.0, MAX_CAMERA_DISTANCE * 0.6);
    let end = (start * 1.9).min(MAX_CAMERA_DISTANCE * 0.95);
    [start as f32, end as f32]
}

pub fn to_f32(m: &Mat4) -> [f32; 16] {
    m.map(|v| v as f32)
}
