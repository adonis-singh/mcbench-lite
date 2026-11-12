// SPDX-License-Identifier: GPL-2.0-only
// Copyright (c) 2016, Intel Corporation
// Copyright (c) Oracle and/or its affiliates
// This file is a transliteration of Intel's LIBM sine and cosine as OpenJDK's
// HotSpot and GraalVM emit them on x86-64. That code is under the GNU General
// Public License, version 2, and so is this file: it is not under the MIT
// licence (NOTICE.md).
//
// Math.sin and Math.cos as x86-64 Java computes them: Intel's libm algorithm,
// which HotSpot's stubGenerator_x86_64_{sin,cos}.cpp and GraalVM's
// jdk.graal.compiler.lir.amd64.AMD64MathSinOp / AMD64MathCosOp emit as SSE
// code (the native image uses the Graal ones; both are the same assembly).
// This is that assembly executed instruction by instruction on f64/u64
// values: xmm registers are [f64; 2] (lane 0 low), general registers are u64
// with 32-bit operations truncating to 32 bits. Every floating operation is
// the IEEE one the instruction performs (scalar ops touch lane 0 only), and
// the large-argument path is the same integer multiplication by 2/pi.
//
// Only a transliteration: no operation is reassociated or fused.

type V = [f64; 2];

fn f(bits: u64) -> f64 {
    f64::from_bits(bits)
}

fn vec2(c: [u64; 2]) -> V {
    [f(c[0]), f(c[1])]
}

fn mulpd(a: V, b: V) -> V {
    [a[0] * b[0], a[1] * b[1]]
}

fn addpd(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1]]
}

fn subpd(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1]]
}

// 32-bit register write: the upper half is cleared.
fn w(v: u64) -> u64 {
    v as u32 as u64
}

// cvttsd2si with a 32-bit destination: "integer indefinite" when out of range.
fn cvtt32(x: f64) -> u64 {
    if x > -2147483649.0 && x < 2147483648.0 {
        (x as i32) as u32 as u64
    } else {
        0x8000_0000
    }
}

// cvttsd2si with a 64-bit destination.
fn cvtt64(x: f64) -> u64 {
    if x >= -9223372036854775808.0 && x < 9223372036854775808.0 {
        (x as i64) as u64
    } else {
        0x8000_0000_0000_0000
    }
}

// ctable entry `i` as its four doubles' bit patterns.
fn entry(i: u64) -> [u64; 4] {
    let b = (i as usize) * 4;
    [CTABLE[b], CTABLE[b + 1], CTABLE[b + 2], CTABLE[b + 3]]
}

#[derive(Clone, Copy)]
enum L {
    B4,
    B5,
    B7,
    B8,
    B9,
    B10,
    B11,
    B13,
}

// The argument reduction for |x| beyond 2^17 or so: a 192-bit product of x's
// significand with 2/pi's bits. Returns xmm0 (low lane), xmm6 (low lane) and
// rax, the registers the common second stage starts from.
fn reduce(x: f64) -> (f64, f64, u64) {
    let bits = x.to_bits();
    let top = ((bits >> 48) & 0xffff) as u32;
    let word = |off: u64| PI_INV_TABLE[(off / 4) as usize] as u64;

    let mut rcx: u64 = (((top & 0x7ff0).wrapping_sub(16224)) >> 7) as u64 & 65532; // table byte offset
    let off = rcx;
    let mut rax: u64 = bits;
    let mut r10: u64 = word(off + 20);
    let mut r8: u64 = word(off + 24);
    let mut rdx: u64 = w(rax);
    rax >>= 21;
    rax = w(rax) | 0x8000_0000;
    rax = w(rax) >> 11;
    let mut r9: u64 = w(r10);
    r10 = r10.wrapping_mul(rdx);
    r9 = r9.wrapping_mul(rax);
    r8 = r8.wrapping_mul(rax);
    let mut rsi: u64 = word(off + 16);
    let mut rdi: u64 = word(off + 12);
    let mut r11: u64 = w(r10);
    r10 >>= 32;
    r9 = r9.wrapping_add(r10);
    r11 = r11.wrapping_add(r8);
    r8 = w(r11);
    r11 >>= 32;
    r9 = r9.wrapping_add(r11);
    r10 = w(rsi);
    rsi = rsi.wrapping_mul(rdx);
    r10 = r10.wrapping_mul(rax);
    r11 = w(rdi);
    rdi = rdi.wrapping_mul(rdx);
    let mut rbx: u64 = w(rsi);
    rsi >>= 32;
    r9 = r9.wrapping_add(rbx);
    rbx = w(r9);
    r9 >>= 32;
    r10 = r10.wrapping_add(rsi);
    r10 = r10.wrapping_add(r9);
    rbx <<= 32;
    r8 |= rbx;
    r11 = r11.wrapping_mul(rax);
    r9 = word(off + 8);
    rsi = word(off + 4);
    rbx = w(rdi);
    rdi >>= 32;
    r10 = r10.wrapping_add(rbx);
    rbx = w(r10);
    r10 >>= 32;
    r11 = r11.wrapping_add(rdi);
    r11 = r11.wrapping_add(r10);
    rdi = r9;
    r9 = r9.wrapping_mul(rdx);
    rdi = rdi.wrapping_mul(rax);
    r10 = w(r9);
    r9 >>= 32;
    r11 = r11.wrapping_add(r10);
    r10 = w(r11);
    r11 >>= 32;
    rdi = rdi.wrapping_add(r9);
    rdi = rdi.wrapping_add(r11);
    r9 = rsi;
    rsi = rsi.wrapping_mul(rdx);
    r9 = r9.wrapping_mul(rax);
    r10 <<= 32;
    r10 |= rbx;
    rax = word(off);
    r11 = w(rsi);
    rsi >>= 32;
    rdi = rdi.wrapping_add(r11);
    r11 = w(rdi);
    rdi >>= 32;
    r9 = r9.wrapping_add(rsi);
    r9 = r9.wrapping_add(rdi);
    rdx = rdx.wrapping_mul(rax);
    rbx = top as u64;
    rcx = off;
    rcx = w(rcx.wrapping_add(rcx));
    rcx = w(rcx.wrapping_add(rcx));
    rcx = w(rcx.wrapping_add(rcx));
    rcx = w(rcx + 19);
    rsi = 32768 & rbx;
    rbx = w(rbx >> 4);
    rbx &= 2047;
    rbx = w(rbx.wrapping_sub(1023));
    rcx = w(rcx.wrapping_sub(rbx));
    r9 = r9.wrapping_add(rdx);
    rdx = w(rcx);
    rdx = w(rdx + 32);

    let mut pc;
    if (rcx as u32 as i32) < 1 {
        pc = L::B4;
    } else {
        rcx = w(0u64.wrapping_sub(rcx));
        rcx = w(rcx + 29);
        r9 = w((r9 as u32).wrapping_shl(rcx as u32 & 31) as u64);
        rdi = w(r9);
        r9 = w(r9 & 536870911);
        if r9 & 268435456 != 0 {
            pc = L::B5;
        } else {
            r9 = w((r9 as u32 >> (rcx & 31)) as u64);
            rbx = 0;
            r9 <<= 32;
            r9 |= r11;
            pc = L::B7;
        }
    }

    loop {
        match pc {
            L::B7 => {
                pc = if r9 == 0 { L::B8 } else { L::B9 };
            }
            L::B9 => {
                r11 = 63 - r9.leading_zeros() as u64;
                rcx = 29;
                let r = (rcx as u32 as i32).wrapping_sub(r11 as u32 as i32);
                rcx = w(r as u32 as u64);
                if r <= 0 {
                    pc = L::B10;
                } else {
                    r9 <<= rcx & 63;
                    rax = r10;
                    r10 <<= rcx & 63;
                    rdx = w(rdx.wrapping_add(rcx));
                    rcx = w(0u64.wrapping_sub(rcx));
                    rcx = w(rcx + 64);
                    rax >>= rcx & 63;
                    r8 >>= rcx & 63;
                    r9 |= rax;
                    r10 |= r8;
                    pc = L::B11;
                }
            }
            L::B10 => {
                if rcx == 0 {
                    pc = L::B11;
                } else {
                    rcx = w(0u64.wrapping_sub(rcx));
                    r10 >>= rcx & 63;
                    rax = r9;
                    r9 >>= rcx & 63;
                    rdx = w(rdx.wrapping_sub(rcx));
                    rcx = w(0u64.wrapping_sub(rcx));
                    rcx = w(rcx + 64);
                    rax <<= rcx & 63;
                    r10 |= rax;
                    pc = L::B11;
                }
            }
            L::B11 => {
                let mut xmm0 = (r9 as i64) as f64;
                r10 >>= 1;
                let mut xmm3 = (r10 as i64) as f64;
                rdx = w(rdx << 4);
                rdx = w(0u64.wrapping_sub(rdx));
                rdx = w(rdx + 16368);
                rdx |= rsi;
                rdx ^= rbx;
                let xmm4 = f((rdx & 0xffff) << 48); // pinsrw 3 into zero
                let xmm2 = f(PI4);
                let mut xmm6 = f(PI48);
                rdx = w(rdx.wrapping_sub(1008));
                let xmm5 = f((rdx & 0xffff) << 48);
                xmm0 *= xmm4;
                rsi = w(rsi << 16);
                rsi = w(((rsi as u32 as i32) >> 31) as u32 as u64);
                xmm3 *= xmm5;
                let mut xmm1 = xmm0;
                xmm0 *= xmm2;
                rdi = w(rdi) >> 29;
                xmm1 += xmm3;
                xmm3 *= xmm2;
                rdi = w(rdi.wrapping_add(rsi));
                rdi ^= rsi;
                xmm6 *= xmm1;
                rax = w(rdi);
                xmm6 += xmm3;
                let mut xmm2 = xmm0;
                xmm0 += xmm6;
                xmm2 -= xmm0;
                xmm6 += xmm2;
                return (xmm0, xmm6, rax);
            }
            L::B8 => {
                rdx = w(rdx + 64);
                r9 = r10;
                r10 = r8;
                r8 = 0;
                if r9 != 0 {
                    pc = L::B9;
                } else {
                    rdx = w(rdx + 64);
                    r9 = r10;
                    r10 = r8;
                    if r9 != 0 {
                        pc = L::B9;
                    } else {
                        return (0.0, 0.0, rax);
                    }
                }
            }
            L::B4 => {
                rcx = w(0u64.wrapping_sub(rcx));
                r9 <<= 32;
                r9 |= r11;
                r9 <<= rcx & 63;
                rdi = r9;
                if (r9 as u32) & 0x8000_0000 != 0 {
                    pc = L::B13;
                } else {
                    r9 = w((r9 as u32 >> (rcx & 31)) as u64);
                    rbx = 0;
                    rdi >>= 3;
                    pc = L::B7;
                }
            }
            L::B5 => {
                r9 = w((r9 as u32 >> (rcx & 31)) as u64);
                rbx = 536870912;
                rbx = w((rbx as u32 >> (rcx & 31)) as u64);
                r9 <<= 32;
                r9 |= r11;
                rbx <<= 32;
                rdi = w(rdi + 536870912);
                // 192-bit negate: [rbx r11 rcx] = [rbx 0 0] - [r9 r10 r8]
                let (c, b1) = 0u64.overflowing_sub(r8);
                let (m, b2a) = 0u64.overflowing_sub(r10);
                let (m, b2b) = m.overflowing_sub(b1 as u64);
                let b2 = b2a | b2b;
                let h = rbx.wrapping_sub(r9).wrapping_sub(b2 as u64);
                r8 = c;
                r10 = m;
                r9 = h;
                rbx = 32768;
                pc = L::B7;
            }
            L::B13 => {
                r9 = w((r9 as u32 >> (rcx & 31)) as u64);
                rbx = 4294967296;
                rbx >>= rcx & 63;
                let (c, b1) = 0u64.overflowing_sub(r8);
                let (m, b2a) = 0u64.overflowing_sub(r10);
                let (m, b2b) = m.overflowing_sub(b1 as u64);
                let b2 = b2a | b2b;
                let h = rbx.wrapping_sub(r9).wrapping_sub(b2 as u64);
                r8 = c;
                r10 = m;
                r9 = h;
                rbx = 32768;
                rdi >>= 3;
                rdi = w(rdi + 536870912);
                pc = L::B7;
            }
        }
    }
}

// The second stage, common to both functions' large-argument and the
// remainder of the reduction path: reduced argument in xmm0 (low lane) with
// its tail in xmm6, quadrant bits in rax.
fn stage2(a0: f64, a6: f64, mut rax: u64, cos: bool) -> f64 {
    let mut xmm0: V = [a0, 0.0];
    let mut xmm6: V = [a6, 0.0];
    let mut xmm1: V = [f(PI32_INV) * xmm0[0], 0.0];
    let sign = xmm0[0].to_bits() & SIGN_MASK;
    let mut xmm5: V = [f(ONEHALF[0] | sign), 0.0];
    xmm1 = addpd(xmm1, xmm5);
    let mut rdx;
    if cos {
        rdx = cvtt64(xmm1[0]);
        xmm1[0] = (rdx as i64) as f64;
    } else {
        rdx = cvtt32(xmm1[0]);
        xmm1[0] = (rdx as u32 as i32) as f64;
    }
    let mut xmm3: V = [f(P1), 0.0];
    let mut xmm2: V = vec2(P2);
    xmm3[0] *= xmm1[0];
    xmm1 = [xmm1[0], xmm1[0]];
    rax = w(rax << 3);
    rdx = w(rdx.wrapping_add(if cos { 1865232 } else { 1865216 }));
    let mut xmm4: V = xmm0;
    rdx = w(rdx.wrapping_add(rax));
    rdx &= 63;
    xmm5 = vec2(SC4);
    let c = entry(rdx);
    xmm2 = mulpd(xmm2, xmm1);
    xmm0[0] -= xmm3[0];
    xmm1[0] *= f(P3);
    xmm4[0] -= xmm3[0];
    let mut xmm7: V = [f(c[1]), 0.0];
    xmm0 = [xmm0[0], xmm0[0]];
    xmm3 = xmm4;
    xmm4[0] -= xmm2[0];
    xmm5 = mulpd(xmm5, xmm0);
    xmm0 = subpd(xmm0, xmm2);
    xmm7[0] *= xmm4[0];
    xmm3[0] -= xmm4[0];
    xmm5 = mulpd(xmm5, xmm0);
    xmm0 = mulpd(xmm0, xmm0);
    xmm3[0] -= xmm2[0];
    xmm2 = [f(c[0]), f(c[1])];
    xmm1[0] -= xmm3[0];
    xmm3 = [f(c[3]), 0.0];
    xmm2[0] += xmm3[0];
    xmm7[0] -= xmm2[0];
    xmm1[0] -= xmm6[0];
    xmm6 = vec2(SC2);
    xmm2[0] *= xmm4[0];
    xmm6 = mulpd(xmm6, xmm0);
    xmm3[0] *= xmm4[0];
    xmm2 = mulpd(xmm2, xmm0);
    xmm0 = mulpd(xmm0, xmm0);
    xmm5 = addpd(xmm5, vec2(SC3));
    xmm4[0] *= f(c[0]);
    xmm6 = addpd(xmm6, vec2(SC1));
    xmm5 = mulpd(xmm5, xmm0);
    xmm0 = xmm3;
    xmm3[0] += f(c[1]);
    xmm1 = mulpd(xmm1, xmm7);
    xmm7 = xmm4;
    xmm4[0] += xmm3[0];
    xmm6 = addpd(xmm6, xmm5);
    xmm5 = [f(c[1]), 0.0];
    xmm5[0] -= xmm3[0];
    xmm3[0] -= xmm4[0];
    xmm1[0] += f(c[2]);
    xmm6 = mulpd(xmm6, xmm2);
    xmm5[0] += xmm0[0];
    xmm3[0] += xmm7[0];
    xmm1[0] += xmm5[0];
    xmm1[0] += xmm3[0];
    xmm1[0] += xmm6[0];
    xmm6 = [xmm6[1], xmm6[1]];
    xmm0 = xmm4;
    xmm1[0] += xmm6[0];
    xmm0[0] += xmm1[0];
    xmm0[0]
}

pub fn sin(x: f64) -> f64 {
    let bits = x.to_bits();
    let hi = (bits >> 32) as u32;
    let rax = (hi & 0x7fff_0000).wrapping_sub(808452096);
    if rax <= 281346048 {
        // |x| in [2^-251, 2^17): the one-stage reduction.
        let mut xmm1: V = [f(PI32_INV) * x, 0.0];
        let sign = bits & SIGN_MASK;
        let xmm5: V = [f(ONEHALF[0] | sign), 0.5];
        xmm1 = addpd(xmm1, xmm5);
        let rdx = cvtt32(xmm1[0]);
        xmm1[0] = (rdx as u32 as i32) as f64;
        let mut xmm6: V = vec2(P2);
        let mut xmm3: V = [f(0x3fb9_21fb_5440_0000), 0.0];
        let mut xmm5: V = vec2(SC4);
        let mut xmm4: V = [x, x];
        let mut xmm0: V = [x, 0.0];
        xmm3[0] *= xmm1[0];
        xmm1 = [xmm1[0], xmm1[0]];
        let c = entry(rdx & 63);
        xmm6 = mulpd(xmm6, xmm1);
        xmm1[0] *= f(P3);
        xmm4[0] -= xmm3[0];
        let mut xmm7: V = [f(c[1]), 0.0];
        xmm0[0] -= xmm3[0];
        let mut xmm3: V = [xmm4[0], xmm4[0]];
        xmm4[0] -= xmm6[0];
        xmm0 = [xmm0[0], xmm0[0]];
        let mut xmm2: V = [f(c[0]), f(c[1])];
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = subpd(xmm0, xmm6);
        xmm7[0] *= xmm4[0];
        xmm3[0] -= xmm4[0];
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = mulpd(xmm0, xmm0);
        xmm3[0] -= xmm6[0];
        xmm6 = vec2(SC2);
        xmm1[0] -= xmm3[0];
        xmm3 = [f(c[3]), 0.0];
        xmm2[0] += xmm3[0];
        xmm7[0] -= xmm2[0];
        xmm2[0] *= xmm4[0];
        xmm6 = mulpd(xmm6, xmm0);
        xmm3[0] *= xmm4[0];
        xmm2 = mulpd(xmm2, xmm0);
        xmm0 = mulpd(xmm0, xmm0);
        xmm5 = addpd(xmm5, vec2(SC3));
        xmm4[0] *= f(c[0]);
        xmm6 = addpd(xmm6, vec2(SC1));
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = xmm3;
        xmm3[0] += f(c[1]);
        xmm1 = mulpd(xmm1, xmm7);
        xmm7 = xmm4;
        xmm4[0] += xmm3[0];
        xmm6 = addpd(xmm6, xmm5);
        let mut xmm5: V = [f(c[1]), 0.0];
        xmm5[0] -= xmm3[0];
        xmm3[0] -= xmm4[0];
        xmm1[0] += f(c[2]);
        xmm6 = mulpd(xmm6, xmm2);
        xmm5[0] += xmm0[0];
        xmm3[0] += xmm7[0];
        xmm1[0] += xmm5[0];
        xmm1[0] += xmm3[0];
        xmm1[0] += xmm6[0];
        xmm6 = [xmm6[1], xmm6[1]];
        xmm0 = xmm4;
        xmm1[0] += xmm6[0];
        xmm0[0] += xmm1[0];
        return xmm0[0];
    }
    if (rax as i32) > 281346048 {
        if (hi >> 16) & 0x7ff0 == 0x7ff0 {
            return x * f(NEG_ZERO);
        }
        let (a0, a6, r) = reduce(x);
        return stage2(a0, a6, r, false);
    }
    // |x| below 2^-251 (or 0): sin x = x, except just above the subnormals.
    if rax >> 20 == 3325 {
        return x * f(ALL_ONES);
    }
    x
}

pub fn cos(x: f64) -> f64 {
    let bits = x.to_bits();
    let hi = (bits >> 32) as u32;
    let rax = (hi & 0x7fff_0000).wrapping_sub(808452096);
    if rax <= 281346048 {
        let mut xmm1: V = [f(PI32_INV) * x, 0.0];
        let sign = bits & SIGN_MASK;
        let xmm5: V = [f(ONEHALF[0] | sign), 0.5];
        xmm1 = addpd(xmm1, xmm5);
        let rdx = cvtt32(xmm1[0]);
        xmm1[0] = (rdx as u32 as i32) as f64;
        let mut xmm2: V = vec2(P2);
        let mut xmm3: V = [f(P1), 0.0];
        xmm3[0] *= xmm1[0];
        xmm1 = [xmm1[0], xmm1[0]];
        let mut xmm4: V = [x, 0.0];
        let mut xmm5: V = vec2(SC4);
        let c = entry(w(rdx.wrapping_add(1865232)) & 63);
        xmm2 = mulpd(xmm2, xmm1);
        let mut xmm0: V = [x, 0.0];
        xmm0[0] -= xmm3[0];
        xmm1[0] *= f(P3);
        xmm4[0] -= xmm3[0];
        let mut xmm7: V = [f(c[1]), 0.0];
        xmm0 = [xmm0[0], xmm0[0]];
        xmm3 = xmm4;
        xmm4[0] -= xmm2[0];
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = subpd(xmm0, xmm2);
        let mut xmm6: V = vec2(SC2);
        xmm7[0] *= xmm4[0];
        xmm3[0] -= xmm4[0];
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = mulpd(xmm0, xmm0);
        xmm3[0] -= xmm2[0];
        xmm2 = [f(c[0]), f(c[1])];
        xmm1[0] -= xmm3[0];
        xmm3 = [f(c[3]), 0.0];
        xmm2[0] += xmm3[0];
        xmm7[0] -= xmm2[0];
        xmm2[0] *= xmm4[0];
        xmm6 = mulpd(xmm6, xmm0);
        xmm3[0] *= xmm4[0];
        xmm2 = mulpd(xmm2, xmm0);
        xmm0 = mulpd(xmm0, xmm0);
        xmm5 = addpd(xmm5, vec2(SC3));
        xmm4[0] *= f(c[0]);
        xmm6 = addpd(xmm6, vec2(SC1));
        xmm5 = mulpd(xmm5, xmm0);
        xmm0 = xmm3;
        xmm3[0] += f(c[1]);
        xmm1 = mulpd(xmm1, xmm7);
        xmm7 = xmm4;
        xmm4[0] += xmm3[0];
        xmm6 = addpd(xmm6, xmm5);
        let mut xmm5: V = [f(c[1]), 0.0];
        xmm5[0] -= xmm3[0];
        xmm3[0] -= xmm4[0];
        xmm1[0] += f(c[2]);
        xmm6 = mulpd(xmm6, xmm2);
        xmm0[0] += xmm5[0];
        xmm3[0] += xmm7[0];
        xmm0[0] += xmm1[0];
        xmm0[0] += xmm3[0];
        xmm0[0] += xmm6[0];
        xmm6 = [xmm6[1], xmm6[1]];
        xmm0[0] += xmm6[0];
        xmm0[0] += xmm4[0];
        return xmm0[0];
    }
    if (rax as i32) > 281346048 {
        if (hi >> 16) & 0x7ff0 == 0x7ff0 {
            return x * f(NEG_ZERO);
        }
        let (a0, a6, r) = reduce(x);
        return stage2(a0, a6, r, true);
    }
    // |x| below 2^-251: cos x = 1 - |x|.
    1.0 - f(bits & !SIGN_MASK)
}

// ctable of AMD64MathSinOp/AMD64MathCosOp: 64 entries of 4 doubles (raw bits).
const CTABLE: [u64; 256] = [
    0x0000000000000000, 0x0000000000000000, 0x0000000000000000, 0x3ff0000000000000,
    0xbf73b92e176d6d31, 0x3fb917a6bc29b42c, 0xbc3e2718e0000000, 0x3ff0000000000000,
    0xbf93ad06011469fb, 0x3fc8f8b83c69a60b, 0xbc626d19c0000000, 0x3ff0000000000000,
    0xbfa60bea939d225a, 0x3fd294062ed59f06, 0xbc75d28da0000000, 0x3ff0000000000000,
    0xbfb37ca1866b95cf, 0x3fd87de2a6aea963, 0xbc672cede0000000, 0x3ff0000000000000,
    0xbfbe3a6873fa1279, 0x3fde2b5d3806f63b, 0x3c5e0d8920000000, 0x3ff0000000000000,
    0xbfc592675bc57974, 0x3fe1c73b39ae68c8, 0x3c8b25dd20000000, 0x3ff0000000000000,
    0xbfcd0dfe53aba2fd, 0x3fe44cf325091dd6, 0x3c68076a20000000, 0x3ff0000000000000,
    0x3fca827999fcef32, 0x3fe6a09e667f3bcd, 0xbc8bdd3420000000, 0x3fe0000000000000,
    0x3fc133cc94247758, 0x3fe8bc806b151741, 0xbc82c5e120000000, 0x3fe0000000000000,
    0x3fac73b39ae68c87, 0x3fea9b66290ea1a3, 0x3c39f630e0000000, 0x3fe0000000000000,
    0xbf9d4a2c7f909c4e, 0x3fec38b2f180bdb1, 0xbc76e0b180000000, 0x3fe0000000000000,
    0xbfbe087565455a75, 0x3fed906bcf328d46, 0x3c7457e620000000, 0x3fe0000000000000,
    0x3fa4a03176acf82d, 0x3fee9f4156c62dda, 0x3c8760b1e0000000, 0x3fd0000000000000,
    0xbfac1d1f0e5967d5, 0x3fef6297cff75cb0, 0x3c75621720000000, 0x3fd0000000000000,
    0xbf9ba1650f592f50, 0x3fefd88da3d12526, 0xbc887df640000000, 0x3fc0000000000000,
    0x0000000000000000, 0x3ff0000000000000, 0x0000000000000000, 0x0000000000000000,
    0x3f9ba1650f592f50, 0x3fefd88da3d12526, 0xbc887df640000000, 0xbfc0000000000000,
    0x3fac1d1f0e5967d5, 0x3fef6297cff75cb0, 0x3c75621720000000, 0xbfd0000000000000,
    0xbfa4a03176acf82d, 0x3fee9f4156c62dda, 0x3c8760b1e0000000, 0xbfd0000000000000,
    0x3fbe087565455a75, 0x3fed906bcf328d46, 0x3c7457e620000000, 0xbfe0000000000000,
    0x3f9d4a2c7f909c4e, 0x3fec38b2f180bdb1, 0xbc76e0b180000000, 0xbfe0000000000000,
    0xbfac73b39ae68c87, 0x3fea9b66290ea1a3, 0x3c39f630e0000000, 0xbfe0000000000000,
    0xbfc133cc94247758, 0x3fe8bc806b151741, 0xbc82c5e120000000, 0xbfe0000000000000,
    0xbfca827999fcef32, 0x3fe6a09e667f3bcd, 0xbc8bdd3420000000, 0xbfe0000000000000,
    0x3fcd0dfe53aba2fd, 0x3fe44cf325091dd6, 0x3c68076a20000000, 0xbff0000000000000,
    0x3fc592675bc57974, 0x3fe1c73b39ae68c8, 0x3c8b25dd20000000, 0xbff0000000000000,
    0x3fbe3a6873fa1279, 0x3fde2b5d3806f63b, 0x3c5e0d8920000000, 0xbff0000000000000,
    0x3fb37ca1866b95cf, 0x3fd87de2a6aea963, 0xbc672cede0000000, 0xbff0000000000000,
    0x3fa60bea939d225a, 0x3fd294062ed59f06, 0xbc75d28da0000000, 0xbff0000000000000,
    0x3f93ad06011469fb, 0x3fc8f8b83c69a60b, 0xbc626d19c0000000, 0xbff0000000000000,
    0x3f73b92e176d6d31, 0x3fb917a6bc29b42c, 0xbc3e2718e0000000, 0xbff0000000000000,
    0x0000000000000000, 0x0000000000000000, 0x0000000000000000, 0xbff0000000000000,
    0x3f73b92e176d6d31, 0xbfb917a6bc29b42c, 0x3c3e2718e0000000, 0xbff0000000000000,
    0x3f93ad06011469fb, 0xbfc8f8b83c69a60b, 0x3c626d19c0000000, 0xbff0000000000000,
    0x3fa60bea939d225a, 0xbfd294062ed59f06, 0x3c75d28da0000000, 0xbff0000000000000,
    0x3fb37ca1866b95cf, 0xbfd87de2a6aea963, 0x3c672cede0000000, 0xbff0000000000000,
    0x3fbe3a6873fa1279, 0xbfde2b5d3806f63b, 0xbc5e0d8920000000, 0xbff0000000000000,
    0x3fc592675bc57974, 0xbfe1c73b39ae68c8, 0xbc8b25dd20000000, 0xbff0000000000000,
    0x3fcd0dfe53aba2fd, 0xbfe44cf325091dd6, 0xbc68076a20000000, 0xbff0000000000000,
    0xbfca827999fcef32, 0xbfe6a09e667f3bcd, 0x3c8bdd3420000000, 0xbfe0000000000000,
    0xbfc133cc94247758, 0xbfe8bc806b151741, 0x3c82c5e120000000, 0xbfe0000000000000,
    0xbfac73b39ae68c87, 0xbfea9b66290ea1a3, 0xbc39f630e0000000, 0xbfe0000000000000,
    0x3f9d4a2c7f909c4e, 0xbfec38b2f180bdb1, 0x3c76e0b180000000, 0xbfe0000000000000,
    0x3fbe087565455a75, 0xbfed906bcf328d46, 0xbc7457e620000000, 0xbfe0000000000000,
    0xbfa4a03176acf82d, 0xbfee9f4156c62dda, 0xbc8760b1e0000000, 0xbfd0000000000000,
    0x3fac1d1f0e5967d5, 0xbfef6297cff75cb0, 0xbc75621720000000, 0xbfd0000000000000,
    0x3f9ba1650f592f50, 0xbfefd88da3d12526, 0x3c887df640000000, 0xbfc0000000000000,
    0x0000000000000000, 0xbff0000000000000, 0x0000000000000000, 0x0000000000000000,
    0xbf9ba1650f592f50, 0xbfefd88da3d12526, 0x3c887df640000000, 0x3fc0000000000000,
    0xbfac1d1f0e5967d5, 0xbfef6297cff75cb0, 0xbc75621720000000, 0x3fd0000000000000,
    0x3fa4a03176acf82d, 0xbfee9f4156c62dda, 0xbc8760b1e0000000, 0x3fd0000000000000,
    0xbfbe087565455a75, 0xbfed906bcf328d46, 0xbc7457e620000000, 0x3fe0000000000000,
    0xbf9d4a2c7f909c4e, 0xbfec38b2f180bdb1, 0x3c76e0b180000000, 0x3fe0000000000000,
    0x3fac73b39ae68c87, 0xbfea9b66290ea1a3, 0xbc39f630e0000000, 0x3fe0000000000000,
    0x3fc133cc94247758, 0xbfe8bc806b151741, 0x3c82c5e120000000, 0x3fe0000000000000,
    0x3fca827999fcef32, 0xbfe6a09e667f3bcd, 0x3c8bdd3420000000, 0x3fe0000000000000,
    0xbfcd0dfe53aba2fd, 0xbfe44cf325091dd6, 0xbc68076a20000000, 0x3ff0000000000000,
    0xbfc592675bc57974, 0xbfe1c73b39ae68c8, 0xbc8b25dd20000000, 0x3ff0000000000000,
    0xbfbe3a6873fa1279, 0xbfde2b5d3806f63b, 0xbc5e0d8920000000, 0x3ff0000000000000,
    0xbfb37ca1866b95cf, 0xbfd87de2a6aea963, 0x3c672cede0000000, 0x3ff0000000000000,
    0xbfa60bea939d225a, 0xbfd294062ed59f06, 0x3c75d28da0000000, 0x3ff0000000000000,
    0xbf93ad06011469fb, 0xbfc8f8b83c69a60b, 0x3c626d19c0000000, 0x3ff0000000000000,
    0xbf73b92e176d6d31, 0xbfb917a6bc29b42c, 0x3c3e2718e0000000, 0x3ff0000000000000,
];

// piInvTable: 2/pi in 32-bit words, read at word offsets.
const PI_INV_TABLE: [u32; 41] = [
    0x00000000, 0x00000000, 0xa2f9836e, 0x4e441529, 0xfc2757d1, 0xf534ddc0,
    0xdb629599, 0x3c439041, 0xfe5163ab, 0xdebbc561, 0xb7246e3a, 0x424dd2e0,
    0x06492eea, 0x09d1921c, 0xfe1deb1c, 0xb129a73e, 0xe88235f5, 0x2ebb4484,
    0xe99c7026, 0xb45f7e41, 0x3991d639, 0x835339f4, 0x9c845f8b, 0xbdf9283b,
    0x1ff897ff, 0xde05980f, 0xef2f118b, 0x5a0a6d1f, 0x6d367ecf, 0x27cb09b7,
    0x4f463f66, 0x9e5fea2d, 0x7527bac7, 0xebe5f17b, 0x3d0739f7, 0x8a5292ea,
    0x6bfb5fb1, 0x1f8d5d08, 0x56033046, 0xfc7b6bab, 0xf0cfbc21,
];

const ONEHALF: [u64; 2] = [0x3fe0000000000000, 0x3fe0000000000000];
const P2: [u64; 2] = [0x3d90b4611a600000, 0x3d90b4611a600000];
const SC4: [u64; 2] = [0x3ec71de3a556c734, 0x3efa01a01a01a01a];
const SC2: [u64; 2] = [0x3f81111111111111, 0x3fa5555555555555];
const SC3: [u64; 2] = [0xbf2a01a01a01a01a, 0xbf56c16c16c16c17];
const SC1: [u64; 2] = [0xbfc5555555555555, 0xbfe0000000000000];
const PI4: u64 = 0x3fe921fb40000000;
const PI48: u64 = 0x3e64442d18469899;
const PI32_INV: u64 = 0x40245f306dc9c883;
const SIGN_MASK: u64 = 0x8000000000000000;
const P3: u64 = 0x3b63198a2e037073;
const ALL_ONES: u64 = 0x3fefffffffffffff;
const TWO_POW_55: u64 = 0x4360000000000000;
const TWO_POW_M55: u64 = 0x3c80000000000000;
const P1: u64 = 0x3fb921fb54400000;
const NEG_ZERO: u64 = 0x8000000000000000;
