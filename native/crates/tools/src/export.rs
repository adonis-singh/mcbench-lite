//! Schematic export: `server/schematic.js`. A chat's schematic is its build
//! as the app shows it at tick 0, the world the engine holds once it has
//! placed the build with Minecraft's own block updates (the settle at 0
//! ticks), written as a Sponge Schematic v3 (`.schem`) that WorldEdit, FAWE
//! and Litematica load. World export (`web/world-export.mjs`, the region
//! writer) is the page's today, not the server's, and is not here.

use mcbench_world::World;

use crate::settle::Settler;

/// `MAX_VOLUME`: Data holds one palette index per cell of the bounding box.
pub const MAX_VOLUME: u64 = 1 << 27;

const SHORT: u8 = 2;
const INT: u8 = 3;
const BYTE_ARRAY: u8 = 7;
const COMPOUND: u8 = 10;
const INT_ARRAY: u8 = 11;

/// A named tag's header: its type and its name.
fn named(out: &mut Vec<u8>, name: &str, kind: u8) {
    out.push(kind);
    out.extend_from_slice(&(name.len() as u16).to_be_bytes());
    out.extend_from_slice(name.as_bytes());
}

/// `writeSchematic(world)`: the gzipped NBT of `world`.
pub fn schematic(world: &World) -> Result<Vec<u8>, String> {
    let bounds = world.bounds().ok_or("The build is empty")?;
    let [width, height, length] =
        [0, 1, 2].map(|axis| (bounds.max[axis] - bounds.min[axis] + 1) as u64);
    let volume = width * height * length;
    if volume > MAX_VOLUME {
        return Err(format!(
            "The build spans {width}×{height}×{length} blocks, more than a schematic export holds ({MAX_VOLUME} cells)"
        ));
    }
    let mut palette: indexmap::IndexMap<String, u32> = indexmap::IndexMap::new();
    palette.insert("minecraft:air".into(), 0);
    let mut local: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();
    let mut cells = vec![0u32; volume as usize];
    let [ox, oy, oz] = bounds.min;
    let (w, l) = (width as i64, length as i64);
    let mut failed = None;
    for (key, chunk) in world.chunks() {
        let [cx, cy, cz] = mcbench_world::coords_of(key);
        chunk.for_each(|index, value| {
            let index_of = match local.get(&value) {
                Some(&index) => index,
                None => {
                    let spec = match mcbench_world::live_placement::block_spec(
                        world.palette().entry(value as usize),
                    ) {
                        Ok(spec) => spec,
                        Err(error) => {
                            failed.get_or_insert(error);
                            return;
                        }
                    };
                    let next = palette.len() as u32;
                    let at = *palette.entry(spec).or_insert(next);
                    local.insert(value, at);
                    at
                }
            };
            let x = (cx * 16 + (index & 15) as i32 - ox) as i64;
            let y = (cy * 16 + (index >> 8) as i32 - oy) as i64;
            let z = (cz * 16 + ((index >> 4) & 15) as i32 - oz) as i64;
            cells[(x + z * w + y * w * l) as usize] = index_of;
        });
    }
    if let Some(error) = failed {
        return Err(error);
    }
    // Indexes are unsigned LEB128 varints, x fastest, then z, then y.
    let mut data = Vec::with_capacity(volume as usize * if palette.len() > 128 { 3 } else { 1 });
    for &cell in &cells {
        let mut value = cell;
        while value >= 0x80 {
            data.push((value & 0x7f) as u8 | 0x80);
            value >>= 7;
        }
        data.push(value as u8);
    }
    let mut nbt = vec![COMPOUND, 0, 0];
    named(&mut nbt, "Schematic", COMPOUND);
    named(&mut nbt, "Version", INT);
    nbt.extend_from_slice(&3i32.to_be_bytes());
    named(&mut nbt, "DataVersion", INT);
    nbt.extend_from_slice(&(mcbench_engine::DATA_VERSION as i32).to_be_bytes());
    for (name, value) in [("Width", width), ("Height", height), ("Length", length)] {
        named(&mut nbt, name, SHORT);
        nbt.extend_from_slice(&(value as u16).to_be_bytes());
    }
    named(&mut nbt, "Offset", INT_ARRAY);
    nbt.extend_from_slice(&3i32.to_be_bytes());
    nbt.extend_from_slice(&[0; 12]);
    named(&mut nbt, "Blocks", COMPOUND);
    named(&mut nbt, "Palette", COMPOUND);
    for (spec, index) in &palette {
        named(&mut nbt, spec, INT);
        nbt.extend_from_slice(&(*index as i32).to_be_bytes());
    }
    nbt.push(0);
    named(&mut nbt, "Data", BYTE_ARRAY);
    nbt.extend_from_slice(&(data.len() as i32).to_be_bytes());
    nbt.extend_from_slice(&data);
    // Blocks, Schematic, the root.
    nbt.extend_from_slice(&[0, 0, 0]);
    gzip(&nbt)
}

/// `zlib.gzipSync(bytes)`: deflate at zlib's default level with the header
/// zlib writes (no name, no time, the operating system's code).
fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut encoder = flate2::GzBuilder::new()
        .operating_system(GZIP_OS)
        .write(Vec::new(), flate2::Compression::new(6));
    encoder
        .write_all(bytes)
        .map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

/// The operating system byte zlib's `deflateInit2` writes (`OS_CODE`):
/// 19 on macOS, 10 on Windows, 3 elsewhere.
#[cfg(target_os = "macos")]
const GZIP_OS: u8 = 19;
#[cfg(windows)]
const GZIP_OS: u8 = 10;
#[cfg(not(any(target_os = "macos", windows)))]
const GZIP_OS: u8 = 3;

/// `buildSchematic(conversationId, world)`: the schematic of the chat's
/// current world at tick 0; the settle is kept with the world, so asking
/// again writes the kept one. The error of a build Minecraft keeps none of
/// is a refusal (HTTP 400 today).
pub async fn build_schematic(
    settler: &Settler,
    conversation: &str,
    world: &mut World,
) -> Result<Vec<u8>, String> {
    let settles = settler.settle(conversation, world, &[0], None).await?;
    let placed = &settles[&0].value.world;
    if placed.size() == 0 {
        return Err(
            "Minecraft keeps none of the blocks of this build: there is nothing to export".into(),
        );
    }
    schematic(placed)
}
