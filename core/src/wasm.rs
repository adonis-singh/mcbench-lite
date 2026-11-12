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
    message: Vec<u8>,
    // The player's view (28 f64, little endian: the host's own byte order).
    view: Vec<u8>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static BUFFERS: RefCell<Buffers> = RefCell::new(Buffers::default());
    static FAILURE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn pointer(bytes: &[u8]) -> *const u8 {
    bytes.as_ptr()
}

// Reads the registries and makes the empty world. Once per instance.
#[no_mangle]
pub extern "C" fn mcb_init() {
    std::panic::set_hook(Box::new(|info| {
        FAILURE.with(|failure| *failure.borrow_mut() = crate::failure(info.payload()).into_bytes());
    }));
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
// is the data buffer.
#[no_mangle]
pub extern "C" fn mcb_place(len: usize, fresh: i32) -> i32 {
    let result = with_session(|session, buffers| {
        let commands = std::str::from_utf8(&buffers.input[..len]).expect("the commands are not UTF-8");
        let blueprint = session.place(commands, fresh != 0)?;
        buffers.data = blueprint;
        Ok(())
    });
    finish(result, |()| 0)
}

// An edit: the commands (the input buffer's first `len` bytes) on the world
// as it is (Session::edit). The data buffer holds the patch, the entities
// buffer the entities' JSON.
#[no_mangle]
pub extern "C" fn mcb_edit(len: usize) -> i32 {
    let result = with_session(|session, buffers| {
        let commands = std::str::from_utf8(&buffers.input[..len]).expect("the commands are not UTF-8");
        let ticked = session.edit(commands)?;
        buffers.data = ticked.changes;
        buffers.entities = ticked.entities;
        Ok(())
    });
    finish(result, |()| 0)
}

// TICK: `ticks` ticks. The data buffer holds the patch, the entities buffer
// the entities' JSON.
#[no_mangle]
pub extern "C" fn mcb_tick(ticks: i32) -> i32 {
    let result = with_session(|session, buffers| {
        let ticked = session.tick(ticks, true)?;
        buffers.data = ticked.changes;
        buffers.entities = ticked.entities;
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
