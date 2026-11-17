// The process's own memory, as the system charges it, and giving freed
// memory back to the system: what the bound on what the helper keeps is
// measured against (PROTOCOL.md, What is kept). Linux: VmRSS and VmHWM of
// /proc/self/status; macOS: the physical footprint (what Activity Monitor
// shows, GPU memory included: Apple's GPUs share the machine's memory);
// Windows: the working set and its peak.

// Bytes the process holds now.
pub fn resident() -> u64 {
    now_and_peak().0
}

// The most bytes the process has held.
pub fn peak() -> u64 {
    now_and_peak().1
}

#[cfg(target_os = "linux")]
fn now_and_peak() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status is readable");
    let field = |name: &str| -> u64 {
        let line = status.lines().find(|l| l.starts_with(name)).unwrap_or_else(|| panic!("/proc/self/status has {name}"));
        line[name.len()..].trim().trim_end_matches(" kB").trim().parse::<u64>().expect("a count of kB") * 1024
    };
    (field("VmRSS:"), field("VmHWM:"))
}

#[cfg(target_os = "macos")]
fn now_and_peak() -> (u64, u64) {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: proc_pid_rusage writes a rusage_info_v4 for RUSAGE_INFO_V4.
    let r = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V4, (&mut info as *mut libc::rusage_info_v4).cast()) };
    assert_eq!(r, 0, "proc_pid_rusage of the helper itself");
    (info.ri_phys_footprint, info.ri_lifetime_max_phys_footprint)
}

#[cfg(windows)]
fn now_and_peak() -> (u64, u64) {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    // SAFETY: the counters are the size given; the handle is this process's pseudo handle.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) }.expect("GetProcessMemoryInfo of the helper itself");
    (c.WorkingSetSize as u64, c.PeakWorkingSetSize as u64)
}

// What the bound counts of the process (PROTOCOL.md, What is kept): its
// memory as the system charges it, except on macOS the memory Metal holds
// that it gives back by itself: the purgeable memory it caches (marked
// volatile about 1.5 s after its last use, which takes it out of the
// footprint) and graphics memory beyond what the device's live resources
// hold (`allocated`: MTLDevice.currentAllocatedSize). Measured on the M3
// after pagoda2m's shots: footprint 694 MB, of it 228 MB purgeable and 100
// MB of graphics memory over the 235 MB allocated, so 366 MB counted; 1.5 s
// later the footprint is 415 MB, nothing purgeable left, 49 MB over, again
// 366 MB counted.
pub fn held(allocated: Option<u64>) -> u64 {
    #[cfg(target_os = "macos")]
    {
        let v = vm_info();
        let (footprint, purgeable, graphics) = (v.phys_footprint, v.ledger_purgeable_nonvolatile.max(0) as u64, v.ledger_tag_graphics_footprint.max(0) as u64);
        let cached = graphics.saturating_sub(allocated.unwrap_or(graphics));
        footprint.saturating_sub(purgeable + cached)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = allocated;
        resident()
    }
}

// The kernel's task_vm_info (mach/task_info.h, TASK_VM_INFO, revision 7):
// the ledgers held() reads.
#[cfg(target_os = "macos")]
#[repr(C, packed(4))]
#[derive(Default, Clone, Copy)]
struct TaskVmInfo {
    virtual_size: u64,
    region_count: i32,
    page_size: i32,
    resident_size: u64,
    resident_size_peak: u64,
    device: u64,
    device_peak: u64,
    internal: u64,
    internal_peak: u64,
    external: u64,
    external_peak: u64,
    reusable: u64,
    reusable_peak: u64,
    purgeable_volatile_pmap: u64,
    purgeable_volatile_resident: u64,
    purgeable_volatile_virtual: u64,
    compressed: u64,
    compressed_peak: u64,
    compressed_lifetime: u64,
    phys_footprint: u64,
    min_address: u64,
    max_address: u64,
    ledger_phys_footprint_peak: i64,
    ledger_purgeable_nonvolatile: i64,
    ledger_purgeable_novolatile_compressed: i64,
    ledger_purgeable_volatile: i64,
    ledger_purgeable_volatile_compressed: i64,
    ledger_tag_network_nonvolatile: i64,
    ledger_tag_network_nonvolatile_compressed: i64,
    ledger_tag_network_volatile: i64,
    ledger_tag_network_volatile_compressed: i64,
    ledger_tag_media_footprint: i64,
    ledger_tag_media_footprint_compressed: i64,
    ledger_tag_media_nofootprint: i64,
    ledger_tag_media_nofootprint_compressed: i64,
    ledger_tag_graphics_footprint: i64,
    ledger_tag_graphics_footprint_compressed: i64,
    ledger_tag_graphics_nofootprint: i64,
    ledger_tag_graphics_nofootprint_compressed: i64,
    ledger_tag_neural_footprint: i64,
    ledger_tag_neural_footprint_compressed: i64,
    ledger_tag_neural_nofootprint: i64,
    ledger_tag_neural_nofootprint_compressed: i64,
    limit_bytes_remaining: u64,
    decompressions: i32,
    ledger_swapins: i64,
    ledger_tag_neural_nofootprint_total: i64,
    ledger_tag_neural_nofootprint_peak: i64,
}

#[cfg(target_os = "macos")]
fn vm_info() -> TaskVmInfo {
    const TASK_VM_INFO: u32 = 22;
    let mut info = TaskVmInfo::default();
    let mut count = (std::mem::size_of::<TaskVmInfo>() / 4) as u32;
    // The task's own port (mach_task_self() is a macro over it).
    extern "C" {
        static mach_task_self_: libc::mach_port_t;
    }
    // SAFETY: task_info writes at most `count` words of the struct for this task.
    let r = unsafe { libc::task_info(mach_task_self_, TASK_VM_INFO, (&mut info as *mut TaskVmInfo).cast(), &mut count) };
    assert_eq!(r, 0, "task_info(TASK_VM_INFO) of the helper itself");
    info
}

// Freed memory returned to the system: allocators keep what was freed for
// reuse, charged to the process (after a 3 million block world was meshed,
// glibc kept 300 MB the helper had freed on the RTX 5090, mimalloc 400 MB
// on the M3).
pub fn give_back() {
    #[cfg(target_os = "linux")]
    // SAFETY: malloc_trim only releases free memory of glibc's heaps.
    unsafe {
        libc::malloc_trim(0);
    }
    // mimalloc keeps a heap a thread: each thread's freed memory is given
    // back by that thread (meshing and JPEG encoding run on rayon's).
    #[cfg(target_os = "macos")]
    {
        // SAFETY: mi_collect only releases free memory of the calling thread's heap.
        rayon::broadcast(|_| unsafe { libmimalloc_sys::mi_collect(true) });
        unsafe { libmimalloc_sys::mi_collect(true) };
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Memory::{GetProcessHeap, HeapCompact, HEAP_FLAGS};
        // SAFETY: HeapCompact on the process heap Rust's allocator uses.
        unsafe {
            let heap = GetProcessHeap().expect("the process heap");
            HeapCompact(heap, HEAP_FLAGS(0));
        }
    }
}
