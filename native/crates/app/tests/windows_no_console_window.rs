//! Windows: no console window opens for the app. The shipped program is a
//! window program (`main.rs`'s `windows_subsystem`); that each child it
//! starts opens none either is `spawns_show_no_console_window.rs`.
#![cfg(windows)]

/// `IMAGE_SUBSYSTEM_WINDOWS_GUI` (`winnt.h`).
const WINDOW_SUBSYSTEM: u16 = 2;

#[test]
fn the_built_program_is_a_window_program_so_windows_opens_no_console_for_it() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_mcbench-lite")).expect("the built program");
    let at = |offset: usize, len: usize| &bytes[offset..offset + len];
    assert_eq!(at(0, 2), b"MZ", "a DOS header");
    let pe = u32::from_le_bytes(at(0x3c, 4).try_into().unwrap()) as usize;
    assert_eq!(at(pe, 4), b"PE\0\0", "a PE signature");
    // The optional header follows the 4-byte signature and the 20-byte COFF
    // header; its Subsystem field is at offset 68 in PE32 and PE32+ alike.
    let subsystem = u16::from_le_bytes(at(pe + 24 + 68, 2).try_into().unwrap());
    assert_eq!(
        subsystem, WINDOW_SUBSYSTEM,
        "the subsystem of mcbench-lite.exe (2 window, 3 console)"
    );
}
