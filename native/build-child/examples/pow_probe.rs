//! Which `pow` the embedded V8's `Math.pow` is: the probe's inputs
//! (`harness/math-probe.js`'s `inputs.txt`) through V8's own
//! `v8::base::ieee754::legacy::pow` (fdlibm's, in the linked static
//! library) and through this platform's libm `pow`, each compared with the
//! child's `pow.txt`.
//!
//! `cargo run --release --example pow_probe -- <math-probe dir>`

unsafe extern "C" {
    // The C++ function's mangled name (Itanium's, MSVC's on Windows); it
    // takes and returns doubles in the platform's C calling convention, as
    // any non-member C++ function.
    #[cfg_attr(not(windows), link_name = "_ZN2v84base7ieee7546legacy3powEdd")]
    #[cfg_attr(windows, link_name = "?pow@legacy@ieee754@base@v8@@YANNN@Z")]
    fn v8_legacy_pow(x: f64, y: f64) -> f64;
}

// Keep the v8 crate (and so its static library) linked.
use v8 as _;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("a math-probe dir"));
    let bits = |v: f64| format!("{:016x}", v.to_bits());
    let inputs: Vec<f64> = std::fs::read_to_string(dir.join("inputs.txt"))
        .unwrap()
        .lines()
        .map(|h| f64::from_bits(u64::from_str_radix(h, 16).unwrap()))
        .collect();
    let child: Vec<String> = std::fs::read_to_string(dir.join("pow.txt"))
        .unwrap()
        .lines()
        .map(String::from)
        .collect();
    let ys = [0.5, 2.0, 3.0, -1.5, 1.0 / 3.0, 10.0, 0.1];
    let (mut legacy_differ, mut libm_differ) = (0, 0);
    let mut per_y = [[0usize; 2]; 7];
    for (i, &x) in inputs.iter().enumerate() {
        for (j, &y) in ys.iter().enumerate() {
            let want = &child[i * 7 + j];
            // SAFETY: a pure function of two doubles.
            let legacy = unsafe { v8_legacy_pow(x.abs(), y) };
            if bits(legacy) != *want {
                legacy_differ += 1;
                per_y[j][0] += 1;
            }
            if bits(x.abs().powf(y)) != *want {
                libm_differ += 1;
                per_y[j][1] += 1;
            }
        }
    }
    println!(
        "{} {}: of {} Math.pow results, {legacy_differ} differ from V8's legacy::pow, {libm_differ} from the libm pow",
        std::env::consts::OS,
        std::env::consts::ARCH,
        child.len()
    );
    for (j, y) in ys.iter().enumerate() {
        println!(
            "  y = {y}: {} differ from legacy::pow, {} from libm",
            per_y[j][0], per_y[j][1]
        );
    }
}
