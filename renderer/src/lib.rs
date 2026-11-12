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

use pack::Pack;
use tile::Tile;
use world::{Load, Mask, World};

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

static mut PACK: Option<Pack> = None;
static mut WORLD: Option<World> = None;
static mut LOAD: Option<Load> = None;
static mut TILE: Tile = Tile::new();
static mut OUTPUT: Vec<u8> = Vec::new();
static mut ERROR: Vec<u8> = Vec::new();

unsafe fn take(ptr: *mut u8, len: usize) -> Vec<u8> {
    Vec::from_raw_parts(ptr, len, len)
}

unsafe fn fail(error: String) -> i32 {
    *std::ptr::addr_of_mut!(ERROR) = error.into_bytes();
    -1
}

unsafe fn pack() -> Result<&'static Pack, String> {
    (*std::ptr::addr_of!(PACK)).as_ref().ok_or_else(|| "No pack is loaded".to_string())
}

// The output: each list u32-counted, its tiles as (i32 x, i32 z).
unsafe fn output_lists(lists: [&[(i32, i32)]; 2]) {
    let output = &mut *std::ptr::addr_of_mut!(OUTPUT);
    output.clear();
    for list in lists {
        output.extend_from_slice(&(list.len() as u32).to_le_bytes());
        for (x, z) in list {
            output.extend_from_slice(&x.to_le_bytes());
            output.extend_from_slice(&z.to_le_bytes());
        }
    }
}

/// # Safety
/// Returns memory the caller fills and hands back to one of the calls below.
#[no_mangle]
pub unsafe extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut bytes = Vec::<u8>::with_capacity(len);
    bytes.set_len(len);
    std::mem::ManuallyDrop::new(bytes).as_mut_ptr()
}

/// # Safety
/// `ptr`, `len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn load_pack(ptr: *mut u8, len: usize) -> i32 {
    let bytes = take(ptr, len);
    match Pack::read(&bytes) {
        Ok(pack) => {
            *std::ptr::addr_of_mut!(PACK) = Some(pack);
            *std::ptr::addr_of_mut!(WORLD) = None;
            *std::ptr::addr_of_mut!(LOAD) = None;
            0
        }
        Err(error) => fail(error),
    }
}

/// # Safety
/// Changes the module's state.
#[no_mangle]
pub unsafe extern "C" fn load_start(min_x: i32, max_x: i32, min_z: i32, max_z: i32) -> i32 {
    *std::ptr::addr_of_mut!(LOAD) = Some(Load::new(Mask { min_x, max_x, min_z, max_z }));
    0
}

/// # Safety
/// `ptr`, `len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn load_take(ptr: *mut u8, len: usize) -> i32 {
    let bytes = take(ptr, len);
    let pack = match pack() {
        Ok(pack) => pack,
        Err(error) => return fail(error),
    };
    let slot = &mut *std::ptr::addr_of_mut!(LOAD);
    let Some(load) = slot.as_mut() else {
        return fail("No blueprint is being read".into());
    };
    match load.take(pack, &bytes) {
        Ok(()) => 0,
        Err(error) => {
            *slot = None;
            fail(error)
        }
    }
}

/// # Safety
/// Changes the module's state.
#[no_mangle]
pub unsafe extern "C" fn load_finish(compare: i32) -> i32 {
    let pack = match pack() {
        Ok(pack) => pack,
        Err(error) => return fail(error),
    };
    let Some(load) = (*std::ptr::addr_of_mut!(LOAD)).take() else {
        return fail("No blueprint is being read".into());
    };
    let world = match load.finish(pack) {
        Ok(world) => world,
        Err(error) => return fail(error),
    };
    let slot = &mut *std::ptr::addr_of_mut!(WORLD);
    let tiles = mesh::tiles(&world);
    let changed = match slot.as_ref() {
        Some(before) if compare != 0 => mesh::changed(before, &world),
        _ => tiles.clone(),
    };
    *slot = Some(world);
    output_lists([&tiles, &changed]);
    0
}

/// # Safety
/// `ptr`, `len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn patch(ptr: *mut u8, len: usize) -> i32 {
    let patch = take(ptr, len);
    let (Ok(pack), Some(world)) = (pack(), (*std::ptr::addr_of_mut!(WORLD)).as_mut()) else {
        return fail("No world is loaded".into());
    };
    match world.patch(pack, &patch) {
        Ok(changed) => {
            output_lists([&mesh::tiles(world), &changed]);
            0
        }
        Err(error) => fail(error),
    }
}

/// # Safety
/// Reads the module's state.
#[no_mangle]
pub unsafe extern "C" fn draw(x: i32, z: i32) -> i32 {
    draw_with(x, z, prbm::write)
}

/// # Safety
/// Reads the module's state.
#[no_mangle]
pub unsafe extern "C" fn draw_indexed(x: i32, z: i32) -> i32 {
    draw_with(x, z, prbm::write_indexed)
}

unsafe fn draw_with(x: i32, z: i32, write: fn(&Tile, &mut Vec<u8>)) -> i32 {
    let (Ok(pack), Some(world)) = (pack(), (*std::ptr::addr_of!(WORLD)).as_ref()) else {
        return fail("No world is loaded".into());
    };
    let tile = &mut *std::ptr::addr_of_mut!(TILE);
    mesh::tile(pack, world, x, z, tile);
    write(tile, &mut *std::ptr::addr_of_mut!(OUTPUT));
    0
}

/// # Safety
/// Reads the module's state.
#[no_mangle]
pub unsafe extern "C" fn snapshot(x: i32, z: i32) -> i32 {
    let Some(world) = (*std::ptr::addr_of!(WORLD)).as_ref() else {
        return fail("No world is loaded".into());
    };
    world.snapshot(x, z, &mut *std::ptr::addr_of_mut!(OUTPUT));
    0
}

/// # Safety
/// `ptr`, `len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn draw_snapshot(ptr: *mut u8, len: usize) -> i32 {
    let bytes = take(ptr, len);
    let pack = match pack() {
        Ok(pack) => pack,
        Err(error) => return fail(error),
    };
    let (world, x, z) = match World::from_snapshot(&bytes) {
        Ok(read) => read,
        Err(error) => return fail(error),
    };
    drop(bytes);
    let tile = &mut *std::ptr::addr_of_mut!(TILE);
    mesh::tile(pack, &world, x, z, tile);
    prbm::write_indexed(tile, &mut *std::ptr::addr_of_mut!(OUTPUT));
    0
}

/// # Safety
/// Reads the module's output buffer.
#[no_mangle]
pub unsafe extern "C" fn output_ptr() -> *const u8 {
    (*std::ptr::addr_of!(OUTPUT)).as_ptr()
}

/// # Safety
/// Reads the module's output buffer.
#[no_mangle]
pub unsafe extern "C" fn output_len() -> usize {
    (*std::ptr::addr_of!(OUTPUT)).len()
}

/// # Safety
/// Reads the module's error buffer.
#[no_mangle]
pub unsafe extern "C" fn error_ptr() -> *const u8 {
    (*std::ptr::addr_of!(ERROR)).as_ptr()
}

/// # Safety
/// Reads the module's error buffer.
#[no_mangle]
pub unsafe extern "C" fn error_len() -> usize {
    (*std::ptr::addr_of!(ERROR)).len()
}
