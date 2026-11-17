// The WebAssembly interface (lib.rs describes it).

use crate::pack::Pack;
use crate::tile::Tile;
use crate::world::{Load, Mask, World};
use crate::{mesh, prbm};

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
///
/// The light an entity is drawn in (web/entity-draw.mjs): the block and sky
/// light the world holds at each of the cells given (x, y, z as i32, little
/// endian, three each), written to the output as a byte each, block light in
/// the low four bits, sky light in the high four.
#[no_mangle]
pub unsafe extern "C" fn light(ptr: *mut u8, len: usize) -> i32 {
    let cells = take(ptr, len);
    let Some(world) = (*std::ptr::addr_of!(WORLD)).as_ref() else {
        return fail("No world is loaded".into());
    };
    if cells.len() % 12 != 0 {
        return fail(format!("{} bytes are not cells of three i32", cells.len()));
    }
    let out = &mut *std::ptr::addr_of_mut!(OUTPUT);
    out.clear();
    for cell in cells.chunks_exact(12) {
        let at = |i: usize| i32::from_le_bytes(cell[i * 4..i * 4 + 4].try_into().unwrap());
        let (sky, block) = world.light([at(0), at(1), at(2)]);
        out.push((block | sky << 4) as u8);
    }
    0
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
/// `ptr`, `len` must come from `alloc`.
#[no_mangle]
pub unsafe extern "C" fn lone(ptr: *mut u8, len: usize, culled: u32) -> i32 {
    let name = take(ptr, len);
    let pack = match pack() {
        Ok(pack) => pack,
        Err(error) => return fail(error),
    };
    let state = match std::str::from_utf8(&name).map_err(|_| "A block state's name is not UTF-8".to_string()).and_then(|name| pack.named(name)) {
        Ok(state) => state,
        Err(error) => return fail(error),
    };
    if culled > 63 {
        return fail(format!("Not a set of directions: {culled}"));
    }
    let tile = &mut *std::ptr::addr_of_mut!(TILE);
    let mut entities = Tile::new();
    mesh::lone(pack, state, culled as u8, tile, &mut entities);
    let out = &mut *std::ptr::addr_of_mut!(OUTPUT);
    out.clear();
    for drawn in [&*tile, &entities] {
        out.extend_from_slice(&(drawn.size as u32).to_le_bytes());
        for face in 0..drawn.size {
            let floats = drawn.position[face * 9..face * 9 + 9].iter().chain(&drawn.uv[face * 6..face * 6 + 6]).chain(&drawn.color[face * 3..face * 3 + 3]);
            for value in floats {
                out.extend_from_slice(&value.to_le_bytes());
            }
            out.extend_from_slice(&drawn.material[face].to_le_bytes());
        }
    }
    for step in mesh::partner(pack, state) {
        out.extend_from_slice(&step.to_le_bytes());
    }
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
