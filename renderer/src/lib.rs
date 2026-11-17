// BlueMap 5.22's hires tile mesher for blueprints, byte for byte: given the
// baked pack (server/McbenchBake.java) and a blueprint (McbenchEngine.write),
// it draws every tile BlueMap draws for that blueprint and writes each as
// the .prbm bytes BlueMap's PRBMWriter writes.
//
// The WebAssembly interface is plain functions over linear memory:
//   alloc(len) -> ptr                 memory for the caller to fill
//   load_pack(ptr, len) -> status     takes the pack (0 ok, -1 error)
//   load_start(min_x, max_x, min_z, max_z) -> status
//       starts reading a blueprint, with its render mask (inclusive block
//       bounds), as the world to draw
//   load_take(ptr, len) -> status     the blueprint's next bytes, any length
//       (world.rs Load: only a part of a section waits for the next ones)
//   load_finish(compare) -> status
//       lights the blueprint and makes it the world to draw, replacing the
//       last one. The output is u32 n, then n tiles (i32 x, i32 z), then u32
//       m and the m of those tiles that can draw differently from the
//       replaced world's (mesh::changed); without `compare`, or with no
//       world before, every tile.
//   patch(ptr, len) -> status
//       applies a patch (world.rs World::patch) to the world: the palette
//       entries it appends, the bounds and built column count after it, and
//       the blocks that changed; the world grows and shrinks with them. The
//       output is as load_finish's: the tiles after it, then those that can
//       draw differently.
//   draw(x, z) -> status              the output is tile x, z's .prbm bytes
//   draw_indexed(x, z) -> status      the output is tile x, z indexed for the
//       GPU (prbm::write_indexed): the same triangles, groups and corner
//       bytes, a quad's repeated corners stored once
//   lone(ptr, len, culled) -> status  the output is the faces the tile
//       mesher draws of the models of the state named (UTF-8, as Minecraft
//       names it, "id[name=value,...]") alone in air, those with a
//       cullface toward `culled` hidden (mesh::lone): its models' faces,
//       then what Minecraft's block entity renderer draws of it, each list
//       u32 n, then n triangles, each f32 x9 corner positions (blocks), f32
//       x6 uvs, f32 x3 colour and i32 material (the map's texture); then
//       i32 x3, the step to the block's other half (mesh::partner); needs
//       only the pack
//   snapshot(x, z) -> status          the output is what tile x, z reads of
//       the world (World::snapshot), for another module to draw it
//   draw_snapshot(ptr, len) -> status the output is a snapshot's tile,
//       indexed as draw_indexed draws it; needs only the pack
//   output_ptr(), output_len()        the last output
//   error_ptr(), error_len()          the last error, UTF-8
// Everything given is freed by the call that takes it. A failed load keeps
// the world it would have replaced; a failed patch changes nothing.

pub mod mesh;
pub mod pack;
pub mod prbm;
pub mod tile;
pub mod trig;
pub mod world;
// The WebAssembly interface above. Only the WebAssembly module has it: a
// native program links the crate as a library, and two builds of it in one
// program (renderer/parity, the mesher against a reference build of itself)
// would export the same names twice.
#[cfg(target_arch = "wasm32")]
mod wasm;

use pack::Pack;
use tile::Tile;
use world::{Mask, World};

// Every tile of a blueprint: (x, z, .prbm bytes), in tile order.
pub fn render(pack: &Pack, blueprint: &[u8], mask: Mask) -> Result<Vec<(i32, i32, Vec<u8>)>, String> {
    let world = World::read(pack, blueprint, mask)?;
    let mut tile = Tile::default();
    Ok(mesh::tiles(&world)
        .into_iter()
        .map(|(x, z)| {
            mesh::tile(pack, &world, x, z, &mut tile);
            let mut bytes = Vec::new();
            prbm::write(&tile, &mut bytes);
            (x, z, bytes)
        })
        .collect())
}
