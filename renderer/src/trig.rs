// flow-math 1.0.3 TrigMath.acos, the double polynomial BlueMap's liquid flow
// angle goes through (not libm).

use std::f64::consts::PI;

pub const RAD_TO_DEG: f64 = 180.0 / PI;
const HALF_PI: f64 = PI / 2.0;
const SQ2P1: f64 = 2.414213562373095048802;
const SQ2M1: f64 = 0.414213562373095048802;
const P4: f64 = 0.161536412982230228262E2;
const P3: f64 = 0.26842548195503973794141E3;
const P2: f64 = 0.11530293515404850115428136E4;
const P1: f64 = 0.178040631643319697105464587E4;
const P0: f64 = 0.89678597403663861959987488E3;
const Q4: f64 = 0.5895697050844462222791E2;
const Q3: f64 = 0.536265374031215315104235E3;
const Q2: f64 = 0.16667838148816337184521798E4;
const Q1: f64 = 0.207933497444540981287275926E4;
const Q0: f64 = 0.89678597403663861962481162E3;

pub fn acos(value: f64) -> f64 {
    if value > 1.0 || value < -1.0 {
        f64::NAN
    } else {
        HALF_PI - asin(value)
    }
}

fn asin(value: f64) -> f64 {
    if value > 1.0 {
        f64::NAN
    } else if value < 0.0 {
        -asin(-value)
    } else {
        let temp = (1.0 - value * value).sqrt();
        if value > 0.7 {
            HALF_PI - msatan(temp / value)
        } else {
            msatan(value / temp)
        }
    }
}

fn mxatan(arg: f64) -> f64 {
    let argsq = arg * arg;
    let mut value = (((P4 * argsq + P3) * argsq + P2) * argsq + P1) * argsq + P0;
    value /= ((((argsq + Q4) * argsq + Q3) * argsq + Q2) * argsq + Q1) * argsq + Q0;
    value * arg
}

fn msatan(arg: f64) -> f64 {
    if arg < SQ2M1 {
        return mxatan(arg);
    }
    if arg > SQ2P1 {
        return HALF_PI - mxatan(1.0 / arg);
    }
    HALF_PI / 2.0 + mxatan((arg - 1.0) / (arg + 1.0))
}
