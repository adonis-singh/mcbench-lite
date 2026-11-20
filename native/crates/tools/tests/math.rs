//! `see_build`'s arithmetic against V8's (`tests/node/math.js`): fdlibm's
//! `tan`, `asin` and `atan2` (the `libm` crate) and V8's `Math.hypot`
//! (`mcbench_tools::shots::hypot`) answer every case as V8 does, and
//! fdlibm's `sin` and `cos` every named view's angle; on other angles V8's
//! `sin` and `cos` (a port of glibc's) and fdlibm's differ in the last
//! place for about 1% of arguments, which is counted and printed.
//!
//! `MCBENCH_TOOLS_MATH=<the file math.js wrote>`; ignored without it.

fn of(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).unwrap())
}

#[test]
#[ignore = "needs MCBENCH_TOOLS_MATH=<the file tests/node/math.js wrote>"]
fn the_cameras_arithmetic_is_v8s() {
    let file = std::env::var("MCBENCH_TOOLS_MATH").expect("MCBENCH_TOOLS_MATH");
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{file}: {e}"));
    let mut counts = std::collections::BTreeMap::<&str, (usize, usize)>::new();
    for line in text.lines() {
        let mut parts = line.split(' ');
        let (name, inputs, answer) = (
            parts.next().unwrap(),
            parts.next().unwrap(),
            of(parts.next().unwrap()),
        );
        let x: Vec<f64> = inputs.split(',').map(of).collect();
        let ours = match name {
            "sin" | "view-sin" => libm::sin(x[0]),
            "cos" | "view-cos" => libm::cos(x[0]),
            "tan" | "view-tan" => libm::tan(x[0]),
            "asin" => libm::asin(x[0]),
            "atan2" => libm::atan2(x[0], x[1]),
            "hypot" => mcbench_tools::shots::hypot(&x),
            other => panic!("no case {other}"),
        };
        let count = counts.entry(name).or_default();
        count.0 += 1;
        if ours.to_bits() != answer.to_bits() {
            count.1 += 1;
        }
    }
    for (name, (cases, differ)) in &counts {
        println!("{name}: {cases} cases, {differ} differ from V8 in the last place");
        if !matches!(*name, "sin" | "cos") {
            assert_eq!(*differ, 0, "{name}");
        }
    }
}
