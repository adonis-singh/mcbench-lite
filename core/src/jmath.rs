// strict_sin and strict_cos below port FdLibm, which carries this notice
// (NOTICE.md):
//
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this software is freely
// granted, provided that this notice is preserved.
//
// Math.sin and Math.cos as the shipped native program computes them. On
// aarch64 the native image uses Java's StrictMath (FdLibm), whose results do
// not depend on the platform's libm: strict_sin and strict_cos below. On
// x86-64 it (and HotSpot) uses Intel's libm algorithm, which differs from
// FdLibm in the last place for some arguments: the `intel` module. Only
// arguments up to 2^19 * pi/2 in magnitude are reduced by the FdLibm port,
// as every use needs.
//
// Which one a build uses is decided here, in the open, and nowhere else:
// Intel's on x86-64 and on WebAssembly, FdLibm's on aarch64, and the
// features jmath-intel and jmath-fdlibm force one on any target (so a
// native build on this machine can be compared with the WebAssembly
// module). WebAssembly is Intel's because that is what Minecraft computes
// on x86-64, where vanilla servers and the Mojang client run (HotSpot's
// intrinsics and the native image agree there); FdLibm is only what the
// aarch64 native image happens to do. Both are plain IEEE arithmetic with
// no host call, so a build gives the same bits on every CPU and browser.

#[cfg(all(feature = "jmath-intel", feature = "jmath-fdlibm"))]
compile_error!("jmath-intel and jmath-fdlibm exclude each other");

#[cfg(not(any(feature = "jmath-intel", feature = "jmath-fdlibm", target_arch = "x86_64", target_arch = "aarch64", target_arch = "wasm32")))]
compile_error!("no Java Math.sin on this target: choose the feature jmath-intel or jmath-fdlibm");

const INTEL: bool = cfg!(any(feature = "jmath-intel", all(not(feature = "jmath-fdlibm"), any(target_arch = "x86_64", target_arch = "wasm32"))));

mod intel;

// Java's Math.sin on this build.
pub fn sin(x: f64) -> f64 {
    if INTEL {
        intel::sin(x)
    } else {
        strict_sin(x)
    }
}

// Java's Math.cos on this build.
pub fn cos(x: f64) -> f64 {
    if INTEL {
        intel::cos(x)
    } else {
        strict_cos(x)
    }
}

const EXP_SIGNIF_BITS: i32 = 0x7fff_ffff;
const EXP_BITS: i32 = 0x7ff0_0000;

fn hi(x: f64) -> i32 {
    (x.to_bits() >> 32) as i32
}

const S1: f64 = f64::from_bits(0xbfc5555555555549);
const S2: f64 = f64::from_bits(0x3f8111111110f8a6);
const S3: f64 = f64::from_bits(0xbf2a01a019c161d5);
const S4: f64 = f64::from_bits(0x3ec71de357b1fe7d);
const S5: f64 = f64::from_bits(0xbe5ae5e68a2b9ceb);
const S6: f64 = f64::from_bits(0x3de5d93a5acfd57c);

fn kernel_sin(x: f64, y: f64, iy: i32) -> f64 {
    let ix = hi(x) & EXP_SIGNIF_BITS;
    if ix < 0x3e40_0000 && x as i32 == 0 {
        return x;
    }
    let z = x * x;
    let v = z * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    if iy == 0 {
        x + v * (S1 + z * r)
    } else {
        x - ((z * (0.5 * y - v * r) - y) - v * S1)
    }
}

const C1: f64 = f64::from_bits(0x3fa555555555554c);
const C2: f64 = f64::from_bits(0xbf56c16c16c15177);
const C3: f64 = f64::from_bits(0x3efa01a019cb1590);
const C4: f64 = f64::from_bits(0xbe927e4f809c52ad);
const C5: f64 = f64::from_bits(0x3e21ee9ebdb4b1c4);
const C6: f64 = f64::from_bits(0xbda8fae9be8838d4);

fn kernel_cos(x: f64, y: f64) -> f64 {
    let ix = hi(x) & EXP_SIGNIF_BITS;
    if ix < 0x3e40_0000 && x as i32 == 0 {
        return 1.0;
    }
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    if ix < 0x3FD3_3333 {
        1.0 - (0.5 * z - (z * r - x * y))
    } else {
        let qx = if ix > 0x3fe9_0000 { 0.28125 } else { f64::from_bits(((ix - 0x0020_0000) as u32 as u64) << 32) };
        let hz = 0.5 * z - qx;
        let a = 1.0 - qx;
        a - (hz - (z * r - x * y))
    }
}

const NPIO2_HW: [i32; 32] = [
    0x3FF921FB, 0x400921FB, 0x4012D97C, 0x401921FB, 0x401F6A7A, 0x4022D97C, 0x4025FDBB, 0x402921FB, 0x402C463A, 0x402F6A7A, 0x4031475C, 0x4032D97C, 0x40346B9C, 0x4035FDBB, 0x40378FDB, 0x403921FB, 0x403AB41B,
    0x403C463A, 0x403DD85A, 0x403F6A7A, 0x40407E4C, 0x4041475C, 0x4042106C, 0x4042D97C, 0x4043A28C, 0x40446B9C, 0x404534AC, 0x4045FDBB, 0x4046C6CB, 0x40478FDB, 0x404858EB, 0x404921FB,
];

const INVPIO2: f64 = f64::from_bits(0x3fe45f306dc9c883);
const PIO2_1: f64 = f64::from_bits(0x3ff921fb54400000);
const PIO2_1T: f64 = f64::from_bits(0x3dd0b4611a626331);
const PIO2_2: f64 = f64::from_bits(0x3dd0b4611a600000);
const PIO2_2T: f64 = f64::from_bits(0x3ba3198a2e037073);
const PIO2_3: f64 = f64::from_bits(0x3ba3198a2e000000);
const PIO2_3T: f64 = f64::from_bits(0x397b839a252049c1);

// __ieee754_rem_pio2: x rem pi/2 as y[0] + y[1], and the quadrant.
fn rem_pio2(x: f64) -> (i32, [f64; 2]) {
    let hx = hi(x);
    let ix = hx & EXP_SIGNIF_BITS;
    if ix <= 0x3fe9_21fb {
        return (0, [x, 0.0]);
    }
    if ix < 0x4002_d97c {
        if hx > 0 {
            let mut z = x - PIO2_1;
            let mut y = [0.0; 2];
            if ix != 0x3ff9_21fb {
                y[0] = z - PIO2_1T;
                y[1] = (z - y[0]) - PIO2_1T;
            } else {
                z -= PIO2_2;
                y[0] = z - PIO2_2T;
                y[1] = (z - y[0]) - PIO2_2T;
            }
            return (1, y);
        } else {
            let mut z = x + PIO2_1;
            let mut y = [0.0; 2];
            // FdLibm.java compares with 0x3ff_921fb here.
            if ix != 0x3ff_921fb {
                y[0] = z + PIO2_1T;
                y[1] = (z - y[0]) + PIO2_1T;
            } else {
                z += PIO2_2;
                y[0] = z + PIO2_2T;
                y[1] = (z - y[0]) + PIO2_2T;
            }
            return (-1, y);
        }
    }
    if ix <= 0x4139_21fb {
        let t = x.abs();
        let n = (t * INVPIO2 + 0.5) as i32;
        let fn_ = n as f64;
        let mut r = t - fn_ * PIO2_1;
        let mut w = fn_ * PIO2_1T;
        let mut y0;
        if n < 32 && ix != NPIO2_HW[(n - 1) as usize] {
            y0 = r - w;
        } else {
            let j = ix >> 20;
            y0 = r - w;
            let mut i = j - ((hi(y0) >> 20) & 0x7ff);
            if i > 16 {
                let t = r;
                w = fn_ * PIO2_2;
                r = t - w;
                w = fn_ * PIO2_2T - ((t - r) - w);
                y0 = r - w;
                i = j - ((hi(y0) >> 20) & 0x7ff);
                if i > 49 {
                    let t = r;
                    w = fn_ * PIO2_3;
                    r = t - w;
                    w = fn_ * PIO2_3T - ((t - r) - w);
                    y0 = r - w;
                }
            }
        }
        let y1 = (r - y0) - w;
        return if hx < 0 { (-n, [-y0, -y1]) } else { (n, [y0, y1]) };
    }
    panic!("a trigonometric argument too large to reduce: {x}");
}

pub fn strict_sin(x: f64) -> f64 {
    let ix = hi(x) & EXP_SIGNIF_BITS;
    if ix <= 0x3fe9_21fb {
        kernel_sin(x, 0.0, 0)
    } else if ix >= EXP_BITS {
        x - x
    } else {
        let (n, y) = rem_pio2(x);
        match n & 3 {
            0 => kernel_sin(y[0], y[1], 1),
            1 => kernel_cos(y[0], y[1]),
            2 => -kernel_sin(y[0], y[1], 1),
            _ => -kernel_cos(y[0], y[1]),
        }
    }
}

pub fn strict_cos(x: f64) -> f64 {
    let ix = hi(x) & EXP_SIGNIF_BITS;
    if ix <= 0x3fe9_21fb {
        kernel_cos(x, 0.0)
    } else if ix >= EXP_BITS {
        x - x
    } else {
        let (n, y) = rem_pio2(x);
        match n & 3 {
            0 => kernel_cos(y[0], y[1]),
            1 => -kernel_sin(y[0], y[1], 1),
            2 => -kernel_cos(y[0], y[1]),
            _ => kernel_sin(y[0], y[1], 1),
        }
    }
}

// Java's Math.acos, which is StrictMath.acos (FdLibm.Acos, e_acos.c).
pub fn acos(x: f64) -> f64 {
    const PIO2_HI: f64 = f64::from_bits(0x3FF921FB54442D18);
    const PIO2_LO: f64 = f64::from_bits(0x3C91A62633145C07);
    const PI: f64 = f64::from_bits(0x400921FB54442D18);
    const P_S0: f64 = f64::from_bits(0x3FC5555555555555);
    const P_S1: f64 = f64::from_bits(0xBFD4D61203EB6F7D);
    const P_S2: f64 = f64::from_bits(0x3FC9C1550E884455);
    const P_S3: f64 = f64::from_bits(0xBFA48228B5688F3B);
    const P_S4: f64 = f64::from_bits(0x3F49EFE07501B288);
    const P_S5: f64 = f64::from_bits(0x3F023DE10DFDF709);
    const Q_S1: f64 = f64::from_bits(0xC0033A271C8A2D4B);
    const Q_S2: f64 = f64::from_bits(0x40002AE59C598AC8);
    const Q_S3: f64 = f64::from_bits(0xBFE6066C1B8D0159);
    const Q_S4: f64 = f64::from_bits(0x3FB3B8C5B12E9282);
    let hx = hi(x);
    let ix = hx & EXP_SIGNIF_BITS;
    let poly = |z: f64| {
        let p = z * (P_S0 + z * (P_S1 + z * (P_S2 + z * (P_S3 + z * (P_S4 + z * P_S5)))));
        let q = 1.0 + z * (Q_S1 + z * (Q_S2 + z * (Q_S3 + z * Q_S4)));
        p / q
    };
    if ix >= 0x3ff0_0000 {
        // |x| >= 1.
        if ((ix - 0x3ff0_0000) as u32 | x.to_bits() as u32) == 0 {
            return if hx > 0 { 0.0 } else { PI + 2.0 * PIO2_LO };
        }
        return (x - x) / (x - x);
    }
    if ix < 0x3fe0_0000 {
        // |x| < 0.5.
        if ix <= 0x3c60_0000 {
            return PIO2_HI + PIO2_LO;
        }
        let r = poly(x * x);
        PIO2_HI - (x - (PIO2_LO - x * r))
    } else if hx < 0 {
        // x < -0.5.
        let z = (1.0 + x) * 0.5;
        let s = z.sqrt();
        let r = poly(z);
        let w = r * s - PIO2_LO;
        PI - 2.0 * (s + w)
    } else {
        // x > 0.5.
        let z = (1.0 - x) * 0.5;
        let s = z.sqrt();
        let df = f64::from_bits(s.to_bits() & 0xFFFF_FFFF_0000_0000);
        let c = (z - df * df) / (s + df);
        let r = poly(z);
        let w = r * s + c;
        2.0 * (df + w)
    }
}

#[cfg(test)]
mod tests {
    // StrictMath's sin and cos for arguments up to 1e5 and TNT's angles, from
    // a file of "<argument bits> <sin bits> <cos bits>" lines (JMATH_ORACLE).
    #[test]
    fn against_java() {
        let Ok(path) = std::env::var("JMATH_ORACLE") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let mut count = 0;
        for line in text.lines() {
            let p: Vec<i64> = line.split(' ').map(|v| v.parse().unwrap()).collect();
            let x = f64::from_bits(p[0] as u64);
            assert_eq!(super::strict_sin(x).to_bits() as i64, p[1], "sin of {x}");
            assert_eq!(super::strict_cos(x).to_bits() as i64, p[2], "cos of {x}");
            count += 1;
        }
        assert!(count > 100000);
    }

    // acos against Java's Math.acos, from a file of "<argument bits> <acos bits>"
    // lines (JMATH_ACOS_ORACLE, written by `MovementReference --acos`).
    #[test]
    fn acos_against_java() {
        let Ok(path) = std::env::var("JMATH_ACOS_ORACLE") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let mut count = 0;
        for line in text.lines() {
            let p: Vec<i64> = line.split(' ').map(|v| v.parse().unwrap()).collect();
            let x = f64::from_bits(p[0] as u64);
            assert_eq!(super::acos(x).to_bits() as i64, p[1], "acos of {x}");
            count += 1;
        }
        assert!(count > 100000);
    }

    // The FdLibm port against raw Math.sin/Math.cos bits recorded on an Apple
    // Silicon native image (StrictMath), in the format of
    // intel_against_java (JMATH_STRICT_ORACLE). Arguments beyond what the
    // port reduces are skipped.
    #[test]
    fn strict_against_java() {
        let Ok(path) = std::env::var("JMATH_STRICT_ORACLE") else { return };
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.len() % 24, 0);
        let mut count = 0;
        for rec in bytes.chunks_exact(24) {
            let word = |i: usize| u64::from_le_bytes(rec[i * 8..i * 8 + 8].try_into().unwrap());
            let x = f64::from_bits(word(0));
            if x.abs() >= 524288.0 * std::f64::consts::FRAC_PI_2 {
                continue;
            }
            for (got, want, name) in [(super::strict_sin(x), word(1), "sin"), (super::strict_cos(x), word(2), "cos")] {
                if f64::from_bits(want).is_nan() {
                    assert!(got.is_nan(), "{name} of {:#x}", x.to_bits());
                } else {
                    assert_eq!(got.to_bits(), want, "{name} of {:#x}", x.to_bits());
                }
                count += 1;
            }
        }
        assert!(count > 1_000_000, "{count}");
    }

    // Values computed by StrictMath in Java (raw bits), with the arguments'.
    #[test]
    fn matches_strict_math() {
        let cases: [(u64, i64, i64); 5] = [
            (4616856424660075023, -4616254549350524694, -4630071016251352290),
            (4612984437277554459, 4602997655605100477, -4617589337512384235),
            (4608556516024407912, 4606866380930668172, 4598401673696746892),
            (4611889858047038902, 4605993062904388130, -4620753693821225912),
            (4618532154007095708, -4626954577648440572, 4606998200369662647),
        ];
        for (bits, sin, cos) in cases {
            let x = f64::from_bits(bits);
            assert_eq!(super::strict_sin(x).to_bits() as i64, sin);
            assert_eq!(super::strict_cos(x).to_bits() as i64, cos);
        }
    }

    // The Intel port against raw Math.sin/Math.cos bits recorded on x86-64
    // (HotSpot and the native image agree): a file of little-endian u64
    // triples "<argument bits> <sin bits> <cos bits>" (JMATH_INTEL_ORACLE).
    // NaN results compare by being NaN. The port is plain f64/u64 arithmetic,
    // so this runs on any target.
    #[test]
    fn intel_against_java() {
        let Ok(path) = std::env::var("JMATH_INTEL_ORACLE") else { return };
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(bytes.len() % 24, 0);
        let mut count = 0;
        for rec in bytes.chunks_exact(24) {
            let word = |i: usize| u64::from_le_bytes(rec[i * 8..i * 8 + 8].try_into().unwrap());
            let x = f64::from_bits(word(0));
            let (s, c) = (super::intel::sin(x), super::intel::cos(x));
            for (got, want, name) in [(s, word(1), "sin"), (c, word(2), "cos")] {
                let want = f64::from_bits(want);
                if want.is_nan() {
                    assert!(got.is_nan(), "{name} of {:#x}", x.to_bits());
                } else {
                    assert_eq!(got.to_bits(), want.to_bits(), "{name} of {:#x}", x.to_bits());
                }
                count += 1;
            }
        }
        assert!(count > 1_000_000, "{count}");
    }

    // Values Math.sin/Math.cos return on x86-64 (raw bits), each through a
    // different path of the port.
    #[test]
    fn intel_matches_x86_java() {
        let cases: [(u64, u64, u64); 52] = [
            (0x0000000000000000, 0x0000000000000000, 0x3ff0000000000000),
            (0x8000000000000000, 0x8000000000000000, 0x3ff0000000000000),
            (0x7ff0000000000000, 0xfff8000000000000, 0xfff8000000000000),
            (0x7ff8000000000000, 0x7ff8000000000000, 0x7ff8000000000000),
            (0x4100000000000000, 0xbfeff8bd7b10d6b0, 0x3fa58ced65ec8b50),
            (0x3030000000000000, 0x3030000000000000, 0x3ff0000000000000),
            (0x000f3e8d79eadfe9, 0x000f3e8d79eadfe9, 0x3ff0000000000000),
            (0x849e57a18f5d42f0, 0x849e57a18f5d42f0, 0x3ff0000000000000),
            (0x092c09531b185c4b, 0x092c09531b185c4b, 0x3ff0000000000000),
            (0x8db0a768536717d2, 0x8db0a768536717d2, 0x3ff0000000000000),
            (0x12407071ec92c863, 0x12407071ec92c863, 0x3ff0000000000000),
            (0x96df2dd782c444fe, 0x96df2dd782c444fe, 0x3ff0000000000000),
            (0x1b656412f2cbc1a7, 0x1b656412f2cbc1a7, 0x3ff0000000000000),
            (0x9ff922d0a9b4cb15, 0x9ff922d0a9b4cb15, 0x3ff0000000000000),
            (0x2480000000000000, 0x2480000000000000, 0x3ff0000000000000),
            (0xa92e96e6a5e5cf21, 0xa92e96e6a5e5cf21, 0x3ff0000000000000),
            (0x2db95581b407de0f, 0x2db95581b407de0f, 0x3ff0000000000000),
            (0xb24d71fc6d788548, 0xb24d71fc6d788548, 0x3ff0000000000000),
            (0x36d83bad7c0d72de, 0x36d83bad7c0d72de, 0x3ff0000000000000),
            (0xbb6989b30e0ee55b, 0xbb6989b30e0ee55b, 0x3ff0000000000000),
            (0xbff681caa012be21, 0xbfef91ef56d8e689, 0x3fc4e96ab38c1e2f),
            (0x448f94dd0209d14a, 0x3feffd7af5640c6d, 0xbf99654d734e6504),
            (0xc91b0761d2487a89, 0xbfe0d67433369999, 0xbfeb3643e57b34ec),
            (0x4da00166e3e2dfcb, 0x3fd566822f77128f, 0xbfee28733b0e063f),
            (0xd24150984e71a54f, 0xbfd60c531221a5ca, 0xbfee0a87e39fe472),
            (0x56dc211136113a78, 0xbfdea4594e46da5b, 0x3fec1809bfc304aa),
            (0xdb6e92420354abe4, 0x3fe8696c9f73d360, 0xbfe4b08fa64d9069),
            (0x5ffb5a83ca123c44, 0x3fe73c138aeadbe0, 0x3fe600df57eaafb7),
            (0xe48fafb3aa409b7d, 0xbfec62bed63ddecb, 0xbfdd8bef1c042fde),
            (0x691a20f897e739f0, 0xbfee08e68ee725b5, 0xbfd61533899788e8),
            (0xedaeed58baf05fd6, 0xbfe62f6e954667f7, 0xbfe70fa2c8becbc3),
            (0x7236caa534feab31, 0x3fe1a3f9fcd6bbf0, 0xbfeab2d2f6839a86),
            (0xf6c75481e5bd8eb8, 0xbfefef54aa252938, 0xbfb052a9a9946ca8),
            (0x7b6eb74d20e8ca9a, 0x3fe154094f26d529, 0x3feae6f9fc375fae),
            (0x3fc856913973b471, 0x3fc8311784201b17, 0x3fef6c5bf4128762),
            (0x401113a91641cce1, 0xbfece88f430a1d3c, 0xbfdb71d52d46d231),
            (0x4003b3ab90b88d50, 0x3fe417d3b9d05266, 0xbfe8e7ba5cb5354a),
            (0x3ff7b00bf663ae91, 0x3fefde9c91389547, 0x3fb716eab7929dcb),
            (0xc015057c9ea870c8, 0x3feb65d013c0cf1e, 0x3fe088a56e6367bb),
            (0x3ff07db86f63a0dd, 0x3feb71ea90504dee, 0x3fe074865509768a),
            (0xc0f63a28223d25ba, 0x3fe7f8d769b64de2, 0x3fe5329adbe5200a),
            (0xc0e3cc1b07a52313, 0x3fe0ba947d780fc7, 0x3feb476f80c9a818),
            (0x41c61a549d66521b, 0x3fd32aa7f7907509, 0xbfee88031a28518c),
            (0xc1c3fe4d4d3cee55, 0x3fe1e6a6b019c452, 0x3fea864cc5f30137),
            (0x66588837d5dc2732, 0x3fee62387dc5b0a4, 0xbfd415436b41bebb),
            (0x3b8451a8ac7e34f2, 0x3b8451a8ac7e34f2, 0x3ff0000000000000),
            (0xc2d9f58aa7c8ab02, 0x3fd4106c0e532006, 0xbfee63052ef97493),
            (0x34fed9ea1dead0a6, 0x34fed9ea1dead0a6, 0x3ff0000000000000),
            (0x4083d4d0507dcb92, 0xbd593da34489c214, 0x3ff0000000000000),
            (0x406019c501fbace5, 0xbd272c63e1d1bd43, 0xbff0000000000000),
            (0x001aaa9911674d61, 0x001aaa9911674d61, 0x3ff0000000000000),
            (0x10400006ccb00000, 0x10400006ccb00000, 0x3ff0000000000000),
        ];
        for (bits, sin, cos) in cases {
            let x = f64::from_bits(bits);
            for (got, want, name) in [(super::intel::sin(x), sin, "sin"), (super::intel::cos(x), cos, "cos")] {
                if f64::from_bits(want).is_nan() {
                    assert!(got.is_nan(), "{name} of {bits:#x}");
                } else {
                    assert_eq!(got.to_bits(), want, "{name} of {bits:#x}");
                }
            }
        }
    }
}
