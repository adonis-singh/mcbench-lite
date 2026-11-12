// The mesher natively, for checking it against BlueMap:
//   mcbench-mesh tiles <pack> <blueprint> <min x> <max x> <min z> <max z> <folder>
//     writes <folder>/x<X>/z<Z>.prbm for every tile, as BlueMap lays them out;
//   mcbench-mesh flow <table>
//     checks flow_angle against McbenchBake's table of BlueMap's own angles.

use mcbench_renderer::mesh::{base_height, flow_angle};
use mcbench_renderer::pack::Pack;
use mcbench_renderer::world::Mask;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args[1].as_str() {
        "tiles" => {
            let pack = Pack::read(&std::fs::read(&args[2]).unwrap()).unwrap();
            let blueprint = std::fs::read(&args[3]).unwrap();
            let bound = |i: usize| args[i].parse::<i32>().unwrap();
            let started = Instant::now();
            let tiles = mcbench_renderer::render(&pack, &blueprint, Mask { min_x: bound(4), max_x: bound(5), min_z: bound(6), max_z: bound(7) }).unwrap();
            eprintln!("{} tiles in {} ms", tiles.len(), started.elapsed().as_millis());
            for (x, z, bytes) in tiles {
                let folder = format!("{}/x{x}", args[8]);
                std::fs::create_dir_all(&folder).unwrap();
                std::fs::write(format!("{folder}/z{z}.prbm"), bytes).unwrap();
            }
        }
        "flow" => {
            let table = std::fs::read(&args[2]).unwrap();
            let mut wrong = 0;
            for own in 0..16 {
                for code in 0..17 * 17 * 17 * 17 {
                    let at = (own * 17 * 17 * 17 * 17 + code) * 2;
                    let expected = i16::from_le_bytes([table[at], table[at + 1]]) as i32;
                    let height = base_height(own as i32) * (1.0 / 16.0);
                    let digit = |side: usize| code / 17usize.pow(3 - side as u32) % 17;
                    let compared = [0, 1, 2, 3].map(|side| match digit(side) {
                        0 => 0.0,
                        level => base_height(level as i32 - 1) * (1.0 / 16.0) - height,
                    });
                    if flow_angle(height, compared) != expected {
                        wrong += 1;
                    }
                }
            }
            println!("flow angles: {wrong} of {} differ", 16 * 17 * 17 * 17 * 17);
            std::process::exit(if wrong == 0 { 0 } else { 1 });
        }
        other => panic!("Unknown command {other}"),
    }
}
