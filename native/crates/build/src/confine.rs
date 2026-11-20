//! Linux: the child's seccomp filter, the layer Node's permission model is
//! today (no child processes, no network). bubblewrap's namespaces already
//! hide the host's files, network and processes; this refuses the system
//! calls that would start a program or open a socket in them, so native code
//! that took over the child can do neither. The child installs it on all its
//! threads once V8 is initialised (its worker threads exist then) and before
//! any JavaScript runs.
//!
//! Refused with EPERM: `execve`, `execveat`, `fork`, `vfork`, `clone`
//! without `CLONE_THREAD` (a new process; threads stay allowed), `socket`,
//! `socketpair`, `connect`, `bind`, `listen`, `accept`, `accept4`,
//! `ptrace`, `process_vm_readv`, `process_vm_writev`, the `io_uring` calls
//! (which open sockets and files without these calls), `unshare`, `setns`
//! and `mount`. `clone3` returns ENOSYS: its flags are in memory a filter
//! cannot read, and glibc's `pthread_create` then makes the thread with
//! `clone`, which the filter can check. Everything else is allowed: V8's JIT
//! (`mmap`, `mprotect` with execute), its threads, futexes and the frames on
//! stdin and stdout. On x86-64 the x32 numbers of the refused calls are
//! refused too.

use std::collections::BTreeMap;

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

/// The refused calls, each refused whatever its arguments.
fn refused() -> Vec<libc::c_long> {
    let mut calls = vec![
        libc::SYS_execve,
        libc::SYS_execveat,
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_ptrace,
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_mount,
    ];
    #[cfg(target_arch = "x86_64")]
    {
        calls.extend([libc::SYS_fork, libc::SYS_vfork]);
        // The x32 ABI: the same calls with bit 30 set, and x32's own execve
        // and execveat.
        const X32: libc::c_long = 0x4000_0000;
        let x32: Vec<libc::c_long> = calls
            .iter()
            .map(|call| call | X32)
            .chain([520 | X32, 545 | X32])
            .collect();
        calls.extend(x32);
    }
    calls
}

fn filter(rules: BTreeMap<i64, Vec<SeccompRule>>, errno: i32) -> BpfProgram {
    let arch = std::env::consts::ARCH
        .try_into()
        .expect("seccompiler knows this architecture");
    SeccompFilter::new(
        rules,
        SeccompAction::Allow,
        SeccompAction::Errno(errno as u32),
        arch,
    )
    .and_then(BpfProgram::try_from)
    .expect("the child's seccomp filter compiles")
}

/// Installs the filter on every thread of the process (`SECCOMP_FILTER_FLAG_TSYNC`).
pub fn install() {
    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = refused()
        .into_iter()
        .map(|call| (call, Vec::new()))
        .collect();
    // clone(flags, ...) without CLONE_THREAD starts a process.
    let no_thread = SeccompCondition::new(
        0,
        SeccompCmpArgLen::Qword,
        SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64),
        0,
    )
    .expect("a clone condition");
    rules.insert(
        libc::SYS_clone,
        vec![SeccompRule::new(vec![no_thread]).expect("a clone rule")],
    );
    let refuse = filter(rules, libc::EPERM);
    let clone3 = filter(
        BTreeMap::from([(libc::SYS_clone3, Vec::new())]),
        libc::ENOSYS,
    );
    seccompiler::apply_filter_all_threads(&clone3).expect("the child's clone3 filter installs");
    seccompiler::apply_filter_all_threads(&refuse).expect("the child's seccomp filter installs");
}
