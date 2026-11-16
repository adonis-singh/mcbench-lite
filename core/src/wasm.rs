// The WebAssembly module's exports: one world (a Session) in one instance,
// driven from JavaScript through linear memory. The host writes a request's
// bytes into the input buffer, calls an export, and reads the result
// buffers; every result stays valid until the next call.
//
// A request that reaches what the port does not model panics, and this
// target cannot unwind: the call traps, and the module is spent. The panic
// hook has kept the reason (mcb_failure_ptr and mcb_failure_len, worded as
// the native program words its failure replies), and the host discards the
// instance; a fresh placement starts in a new one.
//
// Return values: 0 or more is success, -1 a failure Minecraft reported
// (mcb_message_ptr and mcb_message_len), as the native program's failure
// replies that are not refusals.

use crate::session::Session;
use std::cell::RefCell;

#[derive(Default)]
struct Buffers {
    input: Vec<u8>,
    data: Vec<u8>,
    entities: Vec<u8>,
    // The input each palette entry the blueprint or patch in the data buffer
    // adds takes, a byte each (Stream::inputs: 0 none, 1 toggle, 2 press, 3 use).
    inputs: Vec<u8>,
    message: Vec<u8>,
    // The player's view (28 f64, little endian: the host's own byte order),
    // or a standing spot's feet (3 f64).
    view: Vec<u8>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static BUFFERS: RefCell<Buffers> = RefCell::new(Buffers::default());
    static FAILURE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    // The tables' bytes (mcb_tables_buffer) and where loot.json's begin, until mcb_init takes them.
    static TABLES: RefCell<(Vec<u8>, usize)> = const { RefCell::new((Vec::new(), 0)) };
}

fn pointer(bytes: &[u8]) -> *const u8 {
    bytes.as_ptr()
}

// A buffer of `blocks_len + loot_len` bytes for Minecraft's tables, which
// the host writes before mcb_init: blocks.txt's bytes, then loot.json's
// (crate::tables). Once per instance.
#[no_mangle]
pub extern "C" fn mcb_tables_buffer(blocks_len: usize, loot_len: usize) -> *mut u8 {
    TABLES.with(|tables| {
        let mut tables = tables.borrow_mut();
        *tables = (vec![0; blocks_len + loot_len], blocks_len);
        tables.0.as_mut_ptr()
    })
}

// Reads the registries from the tables the host wrote (mcb_tables_buffer)
// and makes the empty world. Once per instance.
#[no_mangle]
pub extern "C" fn mcb_init() {
    std::panic::set_hook(Box::new(|info| {
        FAILURE.with(|failure| *failure.borrow_mut() = crate::failure(info.payload()).into_bytes());
    }));
    let (mut bytes, blocks_len) = TABLES.with(|tables| std::mem::take(&mut *tables.borrow_mut()));
    let loot = bytes.split_off(blocks_len);
    crate::tables::set(crate::tables::Text {
        blocks: String::from_utf8(bytes).expect("blocks.txt is not UTF-8"),
        loot: String::from_utf8(loot).expect("loot.json is not UTF-8"),
    });
    let (reg, blocks) = crate::boot();
    SESSION.with(|session| *session.borrow_mut() = Some(Session::new(reg, blocks)));
}

// A buffer of `len` bytes for the next request's text; the host writes it.
#[no_mangle]
pub extern "C" fn mcb_input_buffer(len: usize) -> *mut u8 {
    BUFFERS.with(|buffers| {
        let input = &mut buffers.borrow_mut().input;
        input.clear();
        input.resize(len, 0);
        input.as_mut_ptr()
    })
}

fn with_session<T>(run: impl FnOnce(&mut Session, &mut Buffers) -> Result<T, String>) -> Result<T, String> {
    SESSION.with(|session| {
        BUFFERS.with(|buffers| {
            let mut session = session.borrow_mut();
            run(session.as_mut().expect("mcb_init was not called"), &mut buffers.borrow_mut())
        })
    })
}

fn finish<T>(result: Result<T, String>, buffers_ok: impl FnOnce(T) -> i32) -> i32 {
    match result {
        Ok(value) => buffers_ok(value),
        Err(message) => {
            BUFFERS.with(|buffers| buffers.borrow_mut().message = message.into_bytes());
            -1
        }
    }
}

// PLACE: the commands (the input buffer's first `len` bytes, text) on an
// empty world if `fresh` is not 0, else on the world as it is. The blueprint
// is the data buffer, the inputs of its palette the inputs buffer.
#[no_mangle]
pub extern "C" fn mcb_place(len: usize, fresh: i32) -> i32 {
    let result = with_session(|session, buffers| {
        let commands = std::str::from_utf8(&buffers.input[..len]).expect("the commands are not UTF-8");
        let blueprint = session.place(commands, fresh != 0)?;
        buffers.data = blueprint;
        buffers.inputs = session.base_inputs();
        Ok(())
    });
    finish(result, |()| 0)
}

// An edit: the commands (the input buffer's first `len` bytes) on the world
// as it is (Session::edit). The data buffer holds the patch, the entities
// buffer the entities' JSON, the inputs buffer the inputs of the palette
// entries the patch adds.
#[no_mangle]
pub extern "C" fn mcb_edit(len: usize) -> i32 {
    let result = with_session(|session, buffers| {
        let commands = std::str::from_utf8(&buffers.input[..len]).expect("the commands are not UTF-8");
        let ticked = session.edit(commands)?;
        buffers.data = ticked.changes;
        buffers.entities = ticked.entities;
        buffers.inputs = ticked.inputs;
        Ok(())
    });
    finish(result, |()| 0)
}

// TICK: `ticks` ticks. The data buffer holds the patch, the entities buffer
// the entities' JSON, the inputs buffer the inputs of the palette entries
// the patch adds.
#[no_mangle]
pub extern "C" fn mcb_tick(ticks: i32) -> i32 {
    let result = with_session(|session, buffers| {
        let ticked = session.tick(ticks, true)?;
        buffers.data = ticked.changes;
        buffers.entities = ticked.entities;
        buffers.inputs = ticked.inputs;
        Ok(())
    });
    finish(result, |()| 0)
}

// INPUT: the kind ("toggle" a lever, "press" a button, "use" a door,
// trapdoor, fence gate, repeater, comparator, note block, daylight detector,
// redstone dust or decorated pot; the input buffer's first `len` bytes) at x,
// y, z, by a player looking with `yaw` degrees.
#[no_mangle]
pub extern "C" fn mcb_input(len: usize, x: i32, y: i32, z: i32, yaw: f32) -> i32 {
    let result = with_session(|session, buffers| {
        let kind = std::str::from_utf8(&buffers.input[..len]).expect("the input kind is not UTF-8");
        session.input(kind, x, y, z, yaw)
    });
    finish(result, |()| 0)
}

// A click at x, y, z by a player looking with `yaw` degrees
// (Session::click): returns the input the block took, 1 toggle, 2 press,
// 3 use, or 0 for none (nothing changed).
#[no_mangle]
pub extern "C" fn mcb_click(x: i32, y: i32, z: i32, yaw: f32) -> i32 {
    let result = with_session(|session, _| session.click(x, y, z, yaw));
    finish(result, |taken| taken.map_or(0, |kind| kind as i32))
}

fn view_bytes(view: [f64; 28]) -> Vec<u8> {
    view.iter().flat_map(|v| v.to_le_bytes()).collect()
}

// Puts a creative player in the world at x, y, z looking as yaw and pitch
// say, flying or not; the view buffer holds it.
#[no_mangle]
pub extern "C" fn mcb_player_spawn(x: f64, y: f64, z: f64, yaw: f32, pitch: f32, flying: i32) -> i32 {
    let result = with_session(|session, buffers| {
        session.spawn_player([x, y, z], yaw, pitch, flying != 0);
        buffers.view = view_bytes(session.player_view());
        Ok(())
    });
    finish(result, |()| 0)
}

// Where a player can stand on top of the columns of the box min x, y, z to
// max x, y, z nearest to the feet position near x, y, z
// (Session::standing_spot): returns 1 with the feet in the view buffer (3
// f64), or 0 when no column has a spot.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn mcb_standing_spot(min_x: i32, min_y: i32, min_z: i32, max_x: i32, max_y: i32, max_z: i32, near_x: f64, near_y: f64, near_z: f64) -> i32 {
    let result = with_session(|session, buffers| {
        let spot = session.standing_spot([min_x, min_y, min_z], [max_x, max_y, max_z], [near_x, near_y, near_z]);
        buffers.view = spot.map_or_else(Vec::new, |feet| feet.iter().flat_map(|v| v.to_le_bytes()).collect());
        Ok(spot.is_some())
    });
    finish(result, i32::from)
}

#[no_mangle]
pub extern "C" fn mcb_player_remove() -> i32 {
    let result = with_session(|session, _| {
        session.remove_player();
        Ok(())
    });
    finish(result, |()| 0)
}

// One tick with the player (Session::player_tick): the keys as flags (1
// forward, 2 backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint) and the
// look. Returns as mcb_tick does; the view buffer holds the player after it.
#[no_mangle]
pub extern "C" fn mcb_player_tick(keys: u32, yaw: f32, pitch: f32) -> i32 {
    let result = with_session(|session, buffers| {
        let (ticked, view) = session.player_tick(keys, yaw, pitch)?;
        buffers.data = ticked.changes;
        buffers.entities = ticked.entities;
        buffers.inputs = ticked.inputs;
        buffers.view = view_bytes(view);
        Ok(())
    });
    finish(result, |()| 0)
}

#[no_mangle]
pub extern "C" fn mcb_view_ptr() -> *const u8 {
    BUFFERS.with(|buffers| pointer(&buffers.borrow().view))
}

#[no_mangle]
pub extern "C" fn mcb_view_len() -> usize {
    BUFFERS.with(|buffers| buffers.borrow().view.len())
}

// STATE: what the world holds besides its blocks, as text in the data buffer.
#[no_mangle]
pub extern "C" fn mcb_state() -> i32 {
    let result = with_session(|session, buffers| {
        buffers.data = session.state().into_bytes();
        Ok(())
    });
    finish(result, |()| 0)
}

// EXPORT: the world's chunks as a save holds them (Session::export), in the
// data buffer.
#[no_mangle]
pub extern "C" fn mcb_export() -> i32 {
    let result = with_session(|session, buffers| {
        buffers.data = session.export();
        Ok(())
    });
    finish(result, |()| 0)
}

#[no_mangle]
pub extern "C" fn mcb_data_ptr() -> *const u8 {
    BUFFERS.with(|buffers| pointer(&buffers.borrow().data))
}

#[no_mangle]
pub extern "C" fn mcb_data_len() -> usize {
    BUFFERS.with(|buffers| buffers.borrow().data.len())
}

#[no_mangle]
pub extern "C" fn mcb_entities_ptr() -> *const u8 {
    BUFFERS.with(|buffers| pointer(&buffers.borrow().entities))
}

#[no_mangle]
pub extern "C" fn mcb_entities_len() -> usize {
    BUFFERS.with(|buffers| buffers.borrow().entities.len())
}

#[no_mangle]
pub extern "C" fn mcb_inputs_ptr() -> *const u8 {
    BUFFERS.with(|buffers| pointer(&buffers.borrow().inputs))
}

#[no_mangle]
pub extern "C" fn mcb_inputs_len() -> usize {
    BUFFERS.with(|buffers| buffers.borrow().inputs.len())
}

#[no_mangle]
pub extern "C" fn mcb_message_ptr() -> *const u8 {
    BUFFERS.with(|buffers| pointer(&buffers.borrow().message))
}

#[no_mangle]
pub extern "C" fn mcb_message_len() -> usize {
    BUFFERS.with(|buffers| buffers.borrow().message.len())
}

// After a trap: why the request panicked ("unported: ..." for a refusal).
#[no_mangle]
pub extern "C" fn mcb_failure_ptr() -> *const u8 {
    FAILURE.with(|failure| pointer(&failure.borrow()))
}

#[no_mangle]
pub extern "C" fn mcb_failure_len() -> usize {
    FAILURE.with(|failure| failure.borrow().len())
}
