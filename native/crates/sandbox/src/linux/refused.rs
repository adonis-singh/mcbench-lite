//! The two calls every program of a box is refused, by one seccomp filter
//! the keeper installs on each program it enters into a box (after
//! `no_new_privs`, before the program's `execve`), so the program and
//! everything it starts carry it.
//!
//! Why. A box's network namespace does not cover `AF_VSOCK`: a vsock socket
//! reaches the hypervisor's host and the other sockets of the machine the
//! box runs in, whatever the box's network is. The boxes of the macOS app
//! run inside the app's Linux VM, whose guest agent talks to the app over
//! vsock, and the boxes of the Windows app run inside WSL's VM, whose host
//! services listen on vsock. Today's Docker refuses the family in its
//! default seccomp profile (`socket` is allowed only when its first argument
//! is not 40), and refuses `io_uring_setup`, whose rings can make a socket
//! of any family without the `socket` call (`IORING_OP_SOCKET`). The filter
//! refuses the same two with the same error, `EPERM`, and allows every other
//! call exactly as before.
//!
//! | Entry point | What the filter does |
//! |---|---|
//! | `socket(AF_VSOCK, ...)`, native and (x86_64) the x32 number | `EPERM` |
//! | `io_uring_setup`, native and (x86_64) the x32 number | `EPERM` |
//! | x86_64, the i386 table (`int 0x80`, a 32-bit program): `socket(AF_VSOCK, ...)` and `io_uring_setup` | `EPERM` |
//! | x86_64, the i386 table: `socketcall(SYS_SOCKET, ...)` | `EPERM`, whatever the family: the family is behind a pointer the filter cannot read. A 32-bit program's sockets made through `socketcall` are refused; one that uses the `socket` call (glibc since 2.23) is not touched |
//! | aarch64, the 32-bit ARM table: `socket(AF_VSOCK, ...)` and `io_uring_setup` | `EPERM` |
//! | `socketpair(AF_VSOCK, ...)` | allowed: the kernel's vsock has no socketpair, and the call fails there |
//!
//! The rules' runner, pasta and the keeper are outside the box and carry no
//! filter.

use std::sync::OnceLock;

/// `struct seccomp_data`: the call's number, the architecture, and the low
/// half of its first argument (both architectures are little-endian).
const NR: u32 = 0;
const ARCH: u32 = 4;
const ARG0: u32 = 16;

/// `BPF_LD | BPF_W | BPF_ABS`, `BPF_JMP | BPF_JEQ | BPF_K`, `BPF_RET | BPF_K`.
const LOAD: u16 = 0x20;
const JUMP_EQ: u16 = 0x15;
const RETURN: u16 = 0x06;

/// `SECCOMP_RET_ALLOW`, and `SECCOMP_RET_ERRNO` with `EPERM`.
const ALLOW: u32 = 0x7fff_0000;
const REFUSE: u32 = 0x0005_0000 | libc::EPERM as u32;

/// `AF_VSOCK`.
const VSOCK: u32 = 40;
/// `io_uring_setup`, the same number in every table named here.
const IO_URING_SETUP: u32 = 425;

/// One step of the filter, with its jumps by name.
enum Step {
    /// Loads the 32-bit word at this offset of `seccomp_data`.
    Load(u32),
    /// Compares what was loaded with the value: goes to the first name when
    /// equal, to the second when not. [`NEXT`] is the step after.
    Equal(u32, &'static str, &'static str),
    /// A name for the step after it.
    Named(&'static str),
    /// Ends with this action.
    Return(u32),
}

const NEXT: &str = "";

/// The filter's steps for this architecture.
#[cfg(target_arch = "x86_64")]
fn steps() -> Vec<Step> {
    use Step::*;
    /// `AUDIT_ARCH_X86_64`, `AUDIT_ARCH_I386`, and the x32 calls' bit.
    const X86_64: u32 = 0xc000_003e;
    const I386: u32 = 0x4000_0003;
    const X32: u32 = 0x4000_0000;
    vec![
        Load(ARCH),
        Equal(X86_64, NEXT, "the i386 table"),
        Load(NR),
        Equal(libc::SYS_socket as u32, "the family", NEXT),
        Equal(libc::SYS_socket as u32 | X32, "the family", NEXT),
        Equal(IO_URING_SETUP, "refuse", NEXT),
        Equal(IO_URING_SETUP | X32, "refuse", "allow"),
        Named("the i386 table"),
        // What was loaded is still the architecture.
        Equal(I386, NEXT, "allow"),
        Load(NR),
        // i386's socket is 359, its socketcall 102.
        Equal(359, "the family", NEXT),
        Equal(IO_URING_SETUP, "refuse", NEXT),
        Equal(102, NEXT, "allow"),
        // socketcall's first argument is the call: SYS_SOCKET is 1.
        Load(ARG0),
        Equal(1, "refuse", "allow"),
        Named("the family"),
        Load(ARG0),
        Equal(VSOCK, "refuse", "allow"),
        Named("allow"),
        Return(ALLOW),
        Named("refuse"),
        Return(REFUSE),
    ]
}

/// The filter's steps for this architecture.
#[cfg(target_arch = "aarch64")]
fn steps() -> Vec<Step> {
    use Step::*;
    /// `AUDIT_ARCH_AARCH64` and `AUDIT_ARCH_ARM`.
    const AARCH64: u32 = 0xc000_00b7;
    const ARM: u32 = 0x4000_0028;
    vec![
        Load(ARCH),
        Equal(AARCH64, NEXT, "the ARM table"),
        Load(NR),
        Equal(libc::SYS_socket as u32, "the family", NEXT),
        Equal(IO_URING_SETUP, "refuse", "allow"),
        Named("the ARM table"),
        // What was loaded is still the architecture.
        Equal(ARM, NEXT, "allow"),
        Load(NR),
        // 32-bit ARM's socket is 281.
        Equal(281, "the family", NEXT),
        Equal(IO_URING_SETUP, "refuse", "allow"),
        Named("the family"),
        Load(ARG0),
        Equal(VSOCK, "refuse", "allow"),
        Named("allow"),
        Return(ALLOW),
        Named("refuse"),
        Return(REFUSE),
    ]
}

/// The filter as the kernel takes it: every jump a count of instructions
/// forward (classic BPF has no other).
fn program() -> Vec<libc::sock_filter> {
    let steps = steps();
    // Where each name's instruction is, counting instructions only.
    let mut names: Vec<(&str, usize)> = Vec::new();
    let mut at = 0;
    for step in &steps {
        match step {
            Step::Named(name) => names.push((*name, at)),
            _ => at += 1,
        }
    }
    let mut program = Vec::new();
    for step in &steps {
        let here = program.len();
        let forward = |name: &str| -> u8 {
            if name == NEXT {
                return 0;
            }
            let to = names
                .iter()
                .find(|(known, _)| *known == name)
                .unwrap_or_else(|| panic!("the filter names no step {name}"))
                .1;
            assert!(to > here, "the filter jumps back to {name}");
            u8::try_from(to - here - 1).expect("a jump of under 256 instructions")
        };
        match step {
            Step::Load(offset) => program.push(libc::sock_filter {
                code: LOAD,
                jt: 0,
                jf: 0,
                k: *offset,
            }),
            Step::Equal(value, equal, different) => program.push(libc::sock_filter {
                code: JUMP_EQ,
                jt: forward(equal),
                jf: forward(different),
                k: *value,
            }),
            Step::Return(action) => program.push(libc::sock_filter {
                code: RETURN,
                jt: 0,
                jf: 0,
                k: *action,
            }),
            Step::Named(_) => {}
        }
    }
    program
}

static PROGRAM: OnceLock<Vec<libc::sock_filter>> = OnceLock::new();

/// Builds the filter: the keeper calls it once before it enters anything,
/// so [`install`] in a forked child only reads it.
pub fn prepare() {
    PROGRAM.get_or_init(program);
}

/// Installs the filter on the calling thread, which has set `no_new_privs`.
/// False when it was not prepared or the kernel refused it (`errno` says
/// why).
///
/// # Safety
///
/// Async-signal-safe: one read of the prepared filter and one `seccomp`
/// call. For a forked child before its `execve`.
pub unsafe fn install() -> bool {
    let Some(program) = PROGRAM.get() else {
        return false;
    };
    let filter = libc::sock_fprog {
        len: program.len() as libc::c_ushort,
        filter: program.as_ptr() as *mut libc::sock_filter,
    };
    // SECCOMP_SET_MODE_FILTER is 1; no flags.
    // SAFETY: `filter` points at the prepared program, which lives for the
    // process's life, and the kernel copies it.
    unsafe { libc::syscall(libc::SYS_seccomp, 1u32, 0u32, &filter) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the filter answers for a call, run as the kernel runs it.
    fn answer(arch: u32, nr: u32, arg0: u32) -> u32 {
        let program = program();
        let mut loaded = 0u32;
        let mut at = 0usize;
        loop {
            let step = &program[at];
            at += 1;
            match step.code {
                LOAD => {
                    loaded = match step.k {
                        NR => nr,
                        ARCH => arch,
                        ARG0 => arg0,
                        other => panic!("the filter loads offset {other}"),
                    }
                }
                JUMP_EQ => {
                    let forward = if loaded == step.k { step.jt } else { step.jf };
                    at += forward as usize;
                }
                RETURN => return step.k,
                other => panic!("the filter has an instruction {other:#x}"),
            }
        }
    }

    const INET: u32 = libc::AF_INET as u32;
    const UNIX: u32 = libc::AF_UNIX as u32;

    #[test]
    fn the_filter_fits_one_seccomp_program() {
        let program = program();
        assert!(program.len() < 64, "{} instructions", program.len());
        assert_eq!(program.last().unwrap().code, RETURN);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_refuses_a_vsock_socket_and_io_uring_by_every_table_and_nothing_else() {
        const X86_64: u32 = 0xc000_003e;
        const I386: u32 = 0x4000_0003;
        const X32: u32 = 0x4000_0000;
        let socket = libc::SYS_socket as u32;
        for (what, arch, nr, arg0, refused) in [
            ("socket(AF_VSOCK)", X86_64, socket, VSOCK, true),
            ("socket(AF_INET)", X86_64, socket, INET, false),
            ("socket(AF_UNIX)", X86_64, socket, UNIX, false),
            ("x32 socket(AF_VSOCK)", X86_64, socket | X32, VSOCK, true),
            ("x32 socket(AF_INET)", X86_64, socket | X32, INET, false),
            ("io_uring_setup", X86_64, IO_URING_SETUP, 8, true),
            ("x32 io_uring_setup", X86_64, IO_URING_SETUP | X32, 8, true),
            (
                "socketpair(AF_VSOCK)",
                X86_64,
                libc::SYS_socketpair as u32,
                VSOCK,
                false,
            ),
            (
                "connect on descriptor 40",
                X86_64,
                libc::SYS_connect as u32,
                40,
                false,
            ),
            (
                "read on descriptor 40",
                X86_64,
                libc::SYS_read as u32,
                40,
                false,
            ),
            ("i386 socket(AF_VSOCK)", I386, 359, VSOCK, true),
            ("i386 socket(AF_INET)", I386, 359, INET, false),
            ("i386 socketcall(SYS_SOCKET)", I386, 102, 1, true),
            ("i386 socketcall(SYS_CONNECT)", I386, 102, 3, false),
            ("i386 io_uring_setup", I386, IO_URING_SETUP, 8, true),
            // i386's call 41 is dup: its first argument is a descriptor.
            ("i386 dup(40)", I386, 41, 40, false),
            ("another architecture", 0xc000_00b7, socket, VSOCK, false),
        ] {
            let expected = if refused { REFUSE } else { ALLOW };
            assert_eq!(answer(arch, nr, arg0), expected, "{what}");
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_refuses_a_vsock_socket_and_io_uring_by_every_table_and_nothing_else() {
        const AARCH64: u32 = 0xc000_00b7;
        const ARM: u32 = 0x4000_0028;
        let socket = libc::SYS_socket as u32;
        for (what, arch, nr, arg0, refused) in [
            ("socket(AF_VSOCK)", AARCH64, socket, VSOCK, true),
            ("socket(AF_INET)", AARCH64, socket, INET, false),
            ("socket(AF_UNIX)", AARCH64, socket, UNIX, false),
            ("io_uring_setup", AARCH64, IO_URING_SETUP, 8, true),
            (
                "socketpair(AF_VSOCK)",
                AARCH64,
                libc::SYS_socketpair as u32,
                VSOCK,
                false,
            ),
            (
                "connect on descriptor 40",
                AARCH64,
                libc::SYS_connect as u32,
                40,
                false,
            ),
            ("ARM socket(AF_VSOCK)", ARM, 281, VSOCK, true),
            ("ARM socket(AF_INET)", ARM, 281, INET, false),
            ("ARM io_uring_setup", ARM, IO_URING_SETUP, 8, true),
            ("another architecture", 0xc000_003e, socket, VSOCK, false),
        ] {
            let expected = if refused { REFUSE } else { ALLOW };
            assert_eq!(answer(arch, nr, arg0), expected, "{what}");
        }
    }
}
