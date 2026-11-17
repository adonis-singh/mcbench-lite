// Minecraft's own trigonometry (net.minecraft.util.Mth), which
// LiquidBlockRenderer turns a flowing fluid's texture by.

// Mth.sin and Mth.cos: a table of 65,536 sines of a turn.
fn sine_table() -> &'static [f32; 65536] {
    static TABLE: std::sync::OnceLock<Box<[f32; 65536]>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Box::new([0f32; 65536]);
        for (i, value) in table.iter_mut().enumerate() {
            *value = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
        }
        table
    })
}

pub fn mth_sin(value: f32) -> f32 {
    sine_table()[((value * 10430.378) as i32 & 65535) as usize]
}

pub fn mth_cos(value: f32) -> f32 {
    sine_table()[((value * 10430.378 + 16384.0) as i32 & 65535) as usize]
}

// Mth.atan2, by its table of arcsines (ASIN_TAB, COS_TAB) and fastInvSqrt.
pub fn mth_atan2(mut y: f64, mut x: f64) -> f64 {
    static TABLES: std::sync::OnceLock<([f64; 257], [f64; 257])> = std::sync::OnceLock::new();
    let (asin_tab, cos_tab) = TABLES.get_or_init(|| {
        let (mut asin_tab, mut cos_tab) = ([0f64; 257], [0f64; 257]);
        for i in 0..257 {
            let asin = (i as f64 / 256.0).asin();
            cos_tab[i] = asin.cos();
            asin_tab[i] = asin;
        }
        (asin_tab, cos_tab)
    });
    let squared = x * x + y * y;
    if squared.is_nan() {
        return f64::NAN;
    }
    let negative_y = y < 0.0;
    if negative_y {
        y = -y;
    }
    let negative_x = x < 0.0;
    if negative_x {
        x = -x;
    }
    let steep = y > x;
    if steep {
        std::mem::swap(&mut x, &mut y);
    }
    let inverse = {
        let half = 0.5 * squared;
        let bits = 6910469410427058090i64.wrapping_sub((squared.to_bits() as i64) >> 1);
        let guess = f64::from_bits(bits as u64);
        guess * (1.5 - half * guess * guess)
    };
    x *= inverse;
    y *= inverse;
    let frac_bias = f64::from_bits(4805340802404319232u64);
    let biased = frac_bias + y;
    let index = biased.to_bits() as i32 as usize;
    let asin = asin_tab[index];
    let cos = cos_tab[index];
    let near = biased - frac_bias;
    let delta = y * cos - x * near;
    let correction = (6.0 + delta * delta) * delta * 0.16666666666666666;
    let mut angle = asin + correction;
    if steep {
        angle = 1.5707963267948966 - angle;
    }
    if negative_x {
        angle = std::f64::consts::PI - angle;
    }
    if negative_y {
        angle = -angle;
    }
    angle
}
