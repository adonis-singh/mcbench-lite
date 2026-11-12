// net.minecraft.util.Mth: the functions entities use whose results depend on
// how Mojang computes them (the sine table, the table-driven atan2).

use crate::jmath;
use crate::registry::Registry;
use std::sync::OnceLock;

fn sin_table() -> &'static [f32] {
    static TABLE: OnceLock<Vec<f32>> = OnceLock::new();
    TABLE.get_or_init(|| (0..65536).map(|i| jmath::sin(i as f64 * std::f64::consts::PI * 2.0 / 65536.0) as f32).collect())
}

// Mth.sin(float).
pub fn sin(value: f32) -> f32 {
    sin_table()[((value * 10430.378f32) as i32 & 65535) as usize]
}

// Mth.cos(float).
pub fn cos(value: f32) -> f32 {
    sin_table()[((value * 10430.378f32 + 16384.0f32) as i32 & 65535) as usize]
}

// Mth.wrapDegrees(float).
pub fn wrap_degrees(value: f32) -> f32 {
    let mut f = value % 360.0f32;
    if f >= 180.0f32 {
        f -= 360.0f32;
    }
    if f < -180.0f32 {
        f += 360.0f32;
    }
    f
}

// Mth.fastInvSqrt(double).
fn fast_inv_sqrt(number: f64) -> f64 {
    let d = 0.5 * number;
    let mut l = number.to_bits() as i64;
    l = 6910469410427058090i64.wrapping_sub(l >> 1);
    let number = f64::from_bits(l as u64);
    number * (1.5 - d * number * number)
}

// Mth.atan2(double y, double x).
pub fn atan2(reg: &Registry, mut y: f64, mut x: f64) -> f64 {
    let frac_bias = f64::from_bits(4805340802404319232u64);
    let d = x * x + y * y;
    if d.is_nan() {
        return f64::NAN;
    }
    let flag = y < 0.0;
    if flag {
        y = -y;
    }
    let flag1 = x < 0.0;
    if flag1 {
        x = -x;
    }
    let flag2 = y > x;
    if flag2 {
        std::mem::swap(&mut x, &mut y);
    }
    let d1 = fast_inv_sqrt(d);
    x *= d1;
    y *= d1;
    let d2 = frac_bias + y;
    let i = d2.to_bits() as i32;
    let d3 = reg.asin_tab[i as usize];
    let d4 = reg.cos_tab[i as usize];
    let d5 = d2 - frac_bias;
    let d6 = y * d4 - x * d5;
    let d7 = (6.0 + d6 * d6) * d6 * 0.16666666666666666;
    let mut d8 = d3 + d7;
    if flag2 {
        d8 = (std::f64::consts::PI / 2.0) - d8;
    }
    if flag1 {
        d8 = std::f64::consts::PI - d8;
    }
    if flag {
        d8 = -d8;
    }
    d8
}

// Mth.floor(float).
pub fn floor_float(value: f32) -> i32 {
    let i = value as i32;
    if value < i as f32 {
        i - 1
    } else {
        i
    }
}
