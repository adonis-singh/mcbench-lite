// The mesher natively, for checking it against BlueMap and Minecraft:
//   mcbench-mesh tiles <pack> <blueprint> <min x> <max x> <min z> <max z> <folder>
//     writes <folder>/x<X>/z<Z>.prbm for every tile, as BlueMap lays them out;
//   mcbench-mesh minecraft <pack> <blueprint> <min x> <max x> <min z> <max z> <scene> <quads>
//     writes the world as the mesher reads it, for server/McbenchTerrain.java
//     to draw with Minecraft's renderer (<scene>, in the format that file
//     documents), and every quad of every tile the mesher draws (<quads>,
//     below), for scripts/mesher-minecraft-parity.js;

use mcbench_renderer::mesh;
use mcbench_renderer::pack::Pack;
use mcbench_renderer::tile::Tile;
use mcbench_renderer::world::{Mask, World};
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
        "minecraft" => {
            let pack = Pack::read(&std::fs::read(&args[2]).unwrap()).unwrap();
            let bound = |i: usize| args[i].parse::<i32>().unwrap();
            let mask = Mask { min_x: bound(4), max_x: bound(5), min_z: bound(6), max_z: bound(7) };
            let world = World::read(&pack, &std::fs::read(&args[3]).unwrap(), Mask { ..mask }).unwrap();
            std::fs::write(&args[8], scene(&pack, &world, &mask)).unwrap();
            std::fs::write(&args[9], quads(&pack, &world)).unwrap();
        }
        other => panic!("Unknown command {other}"),
    }
}

// The world as the mesher reads it, over its lit volume (World::volume):
// each cell's state and light as the tile mesher sees them (Draw::see: air
// and full sky light outside the render mask), in McbenchTerrain's scene
// format.
fn scene(pack: &Pack, world: &World, mask: &Mask) -> Vec<u8> {
    let ([min_x, min_y, min_z], [size_x, size_y, size_z]) = world.volume();
    let mut palette: Vec<u32> = Vec::new();
    let mut index = std::collections::HashMap::new();
    let mut cells = Vec::with_capacity((size_x * size_y * size_z) as usize * 2);
    let mut light = Vec::with_capacity((size_x * size_y * size_z) as usize);
    for y in min_y..min_y + size_y {
        for z in min_z..min_z + size_z {
            for x in min_x..min_x + size_x {
                let (mut state, mut sky, block) = world.look([x, y, z]);
                if !mask.contains(x, z) {
                    state = pack.air;
                    sky = 15;
                }
                let at = *index.entry(state).or_insert_with(|| {
                    palette.push(state);
                    palette.len() - 1
                });
                cells.extend_from_slice(&u16::try_from(at).expect("a scene of more than 65,536 states").to_le_bytes());
                light.push((sky << 4 | block) as u8);
            }
        }
    }
    let mut out = b"MCBS".to_vec();
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&(palette.len() as u32).to_le_bytes());
    for &state in &palette {
        let name = pack.name(state);
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }
    for value in [min_x, min_y, min_z, size_x, size_y, size_z] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&cells);
    out.extend_from_slice(&light);
    for value in [mask.min_x, mask.max_x, mask.min_z, mask.max_z] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

// Every quad of every tile the mesher draws of the world (its two faces,
// (a, b, c) and (a, c, d), put back together), little-endian:
//   "MCBQ" u32 1, then quads to the end, each:
//     u8 kind (tile::Source's: 0 model, 1 fluid, 2 block entity), i32 x, y, z
//     (the drawing block), u32 state, u8 tile full (1 when its tile reached
//     BlueMap's face limit), 4 corners, each f32 x, y, z (blocks, from the
//     drawing block's lowest corner), f32 u, v (over the texture), f32
//     ambient occlusion; f32 r, g, b (the tint); u8 sky light, u8 block
//     light; i32 material (the map's texture)
fn quads(pack: &Pack, world: &World) -> Vec<u8> {
    let mut out = b"MCBQ".to_vec();
    out.extend_from_slice(&1u32.to_le_bytes());
    let mut tile = Tile::default();
    for (tx, tz) in mesh::tiles(world) {
        mesh::tile(pack, world, tx, tz, &mut tile);
        let (min_x, min_z) = (tx * 32 + 2, tz * 32 + 2);
        let full = tile.size >= mcbench_renderer::tile::MAX_CAPACITY - 1;
        for face in (0..tile.size).step_by(2) {
            let source = tile.source[face];
            out.push(source.kind);
            for value in source.at {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&source.state.to_le_bytes());
            out.push(full as u8);
            let origin = [(source.at[0] - min_x) as f32, source.at[1] as f32, (source.at[2] - min_z) as f32];
            // Corners a, b, c of the first face and d, the third of the second.
            for (f, v) in [(face, 0), (face, 1), (face, 2), (face + 1, 2)] {
                for axis in 0..3 {
                    out.extend_from_slice(&(tile.position[f * 9 + v * 3 + axis] - origin[axis]).to_le_bytes());
                }
                out.extend_from_slice(&tile.uv[f * 6 + v * 2].to_le_bytes());
                out.extend_from_slice(&tile.uv[f * 6 + v * 2 + 1].to_le_bytes());
                out.extend_from_slice(&tile.ao[f * 3 + v].to_le_bytes());
            }
            for value in &tile.color[face * 3..face * 3 + 3] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.push(tile.sunlight[face]);
            out.push(tile.blocklight[face]);
            out.extend_from_slice(&tile.material[face].to_le_bytes());
        }
    }
    out
}
