//! macOS: bubblewrap inside the app's Virtualization.framework VM, chat
//! folders shared by virtiofs; the app is ad-hoc signed with the one
//! entitlement `com.apple.security.virtualization`. The VM runs the guest
//! agent (`mcbench-sandbox-agent guest`, the guest base's `/init` starts
//! it), and this module is the lifecycle over [`GuestClient`] talking to it
//! over a vsock connection.
//!
//! The VM, made from Rust through the objc2 family's bindings of the
//! framework (`objc2-virtualization` 0.3.2):
//!
//! | Device | What |
//! |---|---|
//! | boot | `VZLinuxBootLoader`: the guest base's kernel (Debian 13's cloud kernel) and initramfs (`guest/build.py`), the host's choices on the command line (`guest/init` lists them) |
//! | CPUs, memory | [`CPUS`] and [`MEMORY`] |
//! | virtiofs `sandbox` | the data folder's `sandbox` folder (the chats' `/work`), read and write, at `/mnt/sandbox` |
//! | virtiofs `rootfs` | the root filesystem's folder, read only, at `/mnt/rootfs` |
//! | virtio block | `<sandbox>/.vm/disk-<n>.img`, a sparse file of [`DISK_BYTES`]: the agent's own folder (the unpacked root filesystem); `n` is the first whose `disk-<n>.lock` this process can lock, so two VMs on one sandbox folder never share a disk |
//! | virtio network | the NAT attachment (`VZNATNetworkDeviceAttachment`) |
//! | virtio socket | the agent listens on [`PORT`]; the host connects once |
//! | virtio console | the guest's console (kernel and init messages) written to `<sandbox>/.vm/console-<n>.log` |
//! | virtio entropy | the host's random source |
//!
//! There is no graphics, keyboard or pointing device, and no
//! `VZVirtualMachineView`: nothing of the VM can appear on screen. The
//! framework runs the VM in an XPC service process of its own
//! (`com.apple.Virtualization.VirtualMachine`), which ends with its client:
//! the app dying stops the VM. [`crate::Sandbox::shutdown`] and dropping
//! the [`VirtualMachine`] stop it and return when the framework has stopped
//! it and let go of its disk image (the boxes are removed first by
//! the agent's `shutdown`; the disk is written only while the root
//! filesystem is unpacked, so a VM stopped at any other time leaves it
//! whole).
//!
//! The guest base is [`crate::Config`]'s `guest`, from setup
//! (`mcbench_setup::shell::files`).
//!
//! The process that boots the VM must carry the entitlement: tests are
//! signed by `macos/run-signed.sh` (cargo's runner), the app by
//! `codesign --sign - --force --entitlements macos/vm.entitlements` (the
//! release pipeline's one ad-hoc signature of the joined binary, and by hand
//! in development). Without it the framework refuses the configuration, and
//! every call answers [`VM_UNAVAILABLE`] with the framework's words, which
//! name the missing entitlement.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::agent::{GuestClient, WorkMap};
use crate::lifecycle::Sandboxes;
use crate::{BashCommand, BashRun, GuestBase, Sandbox, SandboxError, Workspace};

/// The VM's virtual CPUs (a box's `cpu.max` is two CPUs, today's `--cpus 2`).
pub const CPUS: usize = 2;
/// The VM's memory (a box's `memory.max` is 512 MiB, today's `--memory
/// 512m`; the framework gives the guest pages as it touches them).
pub const MEMORY: u64 = 2 << 30;
/// The agent's disk, a sparse file: what is written takes space.
pub const DISK_BYTES: u64 = 8 << 30;
/// The vsock port the guest agent listens on.
pub const PORT: u32 = 1024;
/// The agent's user in the guest, and so the boxes' user. Apple's virtiofs
/// shows every shared file as owned by the user who looks, so the host
/// user's ids need not reach the guest; these are a Linux desktop's first
/// user's, which `id` shows in a box on such a host.
pub const GUEST_USER: (u32, u32) = (1000, 1000);
/// Where the share of the data folder's `sandbox` folder is in the guest.
pub const GUEST_SANDBOX: &str = "/mnt/sandbox";

/// The sentence a model reads when the VM cannot be started (the
/// unavailable form of the Linux sandbox's missing pieces), with the
/// framework's or the step's own words after it.
pub const VM_UNAVAILABLE: &str =
    "The model's bash shell runs in a Linux virtual machine, and it could not be started";

/// The macOS sandbox.
pub struct VirtualMachine {
    /// The lifecycle over the guest.
    pub guest: Sandboxes<GuestClient>,
    machine: Arc<Mutex<Option<Machine>>>,
}

impl VirtualMachine {
    /// The sandbox over the guest agent's byte stream (`reader` from it,
    /// `writer` to it); `map` gives each `/work`'s path inside the VM.
    pub fn over<R, W>(
        reader: R,
        writer: W,
        map: WorkMap,
        budget: std::sync::Arc<dyn crate::Budget>,
    ) -> Self
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        VirtualMachine {
            guest: Sandboxes::new(GuestClient::new(reader, writer, map), budget),
            machine: Arc::default(),
        }
    }

    /// Boots the VM from the configuration's guest base, with its root
    /// filesystem and the agent, in the background (on the current tokio
    /// runtime), and answers at once: calls wait for the agent, and when the
    /// VM cannot start every call answers [`SandboxError::Unavailable`] with
    /// [`VM_UNAVAILABLE`] and the reason. Then [`VirtualMachine::over`] its
    /// stream.
    #[cfg(not(target_os = "linux"))]
    pub fn start(config: crate::Config) -> Self {
        let guest = config.guest.clone();
        Self::boot(config.root, config.rootfs, config.budget, guest)
    }

    #[cfg_attr(target_os = "linux", allow(dead_code))]
    fn boot(
        root: PathBuf,
        rootfs: (PathBuf, String),
        budget: Arc<dyn crate::Budget>,
        guest: GuestBase,
    ) -> Self {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        let (reader, writer) = tokio::io::split(ours);
        let shared = root.clone();
        let map: WorkMap = Arc::new(move |workspace: &Workspace| {
            let inside = workspace.host.strip_prefix(&shared).map_err(|_| {
                SandboxError::Failed(format!(
                    "the chat's /work {} is not in the sandbox folder {}",
                    workspace.host.display(),
                    shared.display()
                ))
            })?;
            Ok(Path::new(GUEST_SANDBOX).join(inside))
        });
        let sandbox = Self::over(reader, writer, map, budget);
        let slot = sandbox.machine.clone();
        tokio::spawn(async move {
            let booted = tokio::task::spawn_blocking(move || Machine::boot(&root, &rootfs, &guest))
                .await
                .expect("the boot task");
            match booted {
                Ok((machine, connection)) => {
                    *slot.lock().expect("the machine") = Some(machine);
                    let mut stream = connection_stream(connection);
                    let mut theirs = theirs;
                    let _ = tokio::io::copy_bidirectional(&mut theirs, &mut stream).await;
                    let machine = slot.lock().expect("the machine").take();
                    if let Some(machine) = machine {
                        let _ = tokio::task::spawn_blocking(move || machine.stop()).await;
                    }
                }
                Err(reason) => {
                    answer_unavailable(theirs, format!("{VM_UNAVAILABLE}: {reason}")).await
                }
            }
        });
        sandbox
    }

    /// Stops the VM (the boxes' processes end with it) and returns when
    /// the framework has stopped it and let go of its disk.
    async fn stop(&self) {
        let machine = self.machine.lock().expect("the machine").take();
        if let Some(machine) = machine {
            tokio::task::spawn_blocking(move || machine.stop())
                .await
                .expect("the stop task");
        }
    }
}

impl Drop for VirtualMachine {
    /// Stops the VM and waits for it (blocking the dropping thread, up to
    /// the framework's stop and the image's release: tenths of a second).
    fn drop(&mut self) {
        let machine = self.machine.lock().expect("the machine").take();
        if let Some(machine) = machine {
            machine.stop();
        }
    }
}

/// Answers every call read from `stream` with `{"unavailable": message}`,
/// until the host's side closes it.
pub(crate) async fn answer_unavailable(stream: tokio::io::DuplexStream, message: String) {
    let (mut reader, mut writer) = tokio::io::split(stream);
    while let Ok(Some((kind, body))) = crate::agent::read_frame(&mut reader).await {
        if kind != crate::agent::kind::CALL {
            continue;
        }
        let Ok(call) = serde_json::from_slice::<serde_json::Value>(&body) else {
            break;
        };
        let answer = serde_json::json!({ "id": call["id"], "unavailable": message });
        let body = serde_json::to_vec(&answer).expect("an answer serialises");
        if crate::agent::write_frame(&mut writer, crate::agent::kind::ANSWER, &body)
            .await
            .is_err()
        {
            break;
        }
    }
}

#[async_trait]
impl Sandbox for VirtualMachine {
    async fn prepare(&self) -> Result<(), SandboxError> {
        Sandbox::prepare(&self.guest).await
    }
    async fn exec(
        &self,
        workspace: &Workspace,
        command: &BashCommand,
        cancel: &CancellationToken,
    ) -> Result<BashRun, SandboxError> {
        Sandbox::exec(&self.guest, workspace, command, cancel).await
    }
    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError> {
        Sandbox::view_image(&self.guest, workspace, path).await
    }
    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError> {
        Sandbox::remove_work_files(&self.guest, workspace, paths).await
    }
    async fn reset(&self, chat: &str) {
        Sandbox::reset(&self.guest, chat).await
    }
    async fn destroy(&self, workspace: &Workspace) -> Result<(), SandboxError> {
        Sandbox::destroy(&self.guest, workspace).await
    }
    /// The boxes removed by the agent, then the VM stopped; it returns when
    /// the VM has stopped and let go of its disk.
    async fn shutdown(&self) {
        Sandbox::shutdown(&self.guest).await;
        self.stop().await;
    }
    fn network_access(&self) -> bool {
        Sandbox::network_access(&self.guest)
    }
    fn disable_network(&self) {
        Sandbox::disable_network(&self.guest)
    }
}

#[cfg(target_os = "macos")]
mod vz {
    //! The framework's objects, touched only on the VM's serial queue.

    use std::os::fd::{FromRawFd, OwnedFd};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchRetained};
    use objc2::AllocAnyThread;
    use objc2::rc::Retained;
    use objc2_foundation::{NSArray, NSError, NSFileHandle, NSString, NSURL};
    use objc2_virtualization::*;

    /// A value that is used only inside closures run on the VM's serial
    /// queue.
    struct OnQueue<T>(T);
    // SAFETY: the framework's objects are not thread-safe, but every use of
    // the wrapped value is inside a closure run on the one serial queue the
    // VM was made with (the framework's own rule for VZVirtualMachine), so
    // they are never touched by two threads at once.
    unsafe impl<T> Send for OnQueue<T> {}

    /// The running VM: the framework's object and its queue.
    pub struct Vm {
        vm: OnQueue<Retained<VZVirtualMachine>>,
        queue: DispatchRetained<DispatchQueue>,
    }

    /// What the VM is made from.
    pub struct Parts<'a> {
        pub kernel: &'a Path,
        pub initramfs: &'a Path,
        pub command_line: String,
        pub sandbox: &'a Path,
        pub rootfs_folder: &'a Path,
        pub disk: &'a Path,
        pub console: &'a Path,
        pub cpus: usize,
        pub memory: u64,
    }

    fn described(error: &NSError) -> String {
        error.localizedDescription().to_string()
    }

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    /// Makes and validates the configuration, makes the VM on a new serial
    /// queue and starts it; answers once it runs.
    pub fn start(parts: Parts<'_>) -> Result<Vm, String> {
        // SAFETY: isSupported is a class property readable anywhere.
        if !unsafe { VZVirtualMachine::isSupported() } {
            return Err("this Mac does not support virtualization".to_string());
        }
        let console = std::fs::File::create(parts.console)
            .map_err(|e| format!("{}: {e}", parts.console.display()))?;
        let queue = DispatchQueue::new("mcbench.sandbox.vm", None);
        let (made_tx, made_rx) = mpsc::channel();
        let configure = OnQueue((
            parts.kernel.to_path_buf(),
            parts.initramfs.to_path_buf(),
            parts.command_line.clone(),
            parts.sandbox.to_path_buf(),
            parts.rootfs_folder.to_path_buf(),
            parts.disk.to_path_buf(),
            console,
            parts.cpus,
            parts.memory,
            made_tx,
            queue.clone(),
        ));
        queue.exec_async(move || {
            let configure = configure;
            let (
                kernel,
                initramfs,
                command_line,
                sandbox,
                rootfs,
                disk,
                console,
                cpus,
                memory,
                made,
                queue,
            ) = configure.0;
            // SAFETY: framework calls with valid arguments, on the VM's queue.
            let result = unsafe {
                make(
                    &kernel,
                    &initramfs,
                    &command_line,
                    &sandbox,
                    &rootfs,
                    &disk,
                    console,
                    cpus,
                    memory,
                    &queue,
                )
            };
            let _ = made.send(result.map(OnQueue));
        });
        let vm = made_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "the VM was not made within 30 s".to_string())??;
        let vm = Vm { vm, queue };
        let (started_tx, started_rx) = mpsc::channel::<Result<(), String>>();
        vm.on_queue(move |vm| {
            let block = RcBlock::new(move |error: *mut NSError| {
                // SAFETY: the framework passes nil or a valid NSError.
                let result = match unsafe { error.as_ref() } {
                    None => Ok(()),
                    Some(error) => Err(described(error)),
                };
                let _ = started_tx.send(result);
            });
            // SAFETY: start on the VM's queue with a block that lives as long
            // as the framework retains it.
            unsafe { vm.startWithCompletionHandler(&block) };
        });
        started_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| "the VM did not start within 30 s".to_string())??;
        Ok(vm)
    }

    /// The configuration and the VM.
    ///
    /// # Safety
    /// On `queue`, which the VM is made with.
    #[allow(clippy::too_many_arguments)]
    unsafe fn make(
        kernel: &Path,
        initramfs: &Path,
        command_line: &str,
        sandbox: &Path,
        rootfs: &Path,
        disk: &Path,
        console: std::fs::File,
        cpus: usize,
        memory: u64,
        queue: &DispatchQueue,
    ) -> Result<Retained<VZVirtualMachine>, String> {
        // SAFETY: framework initialisers and setters with valid objects.
        unsafe {
            let boot =
                VZLinuxBootLoader::initWithKernelURL(VZLinuxBootLoader::alloc(), &url(kernel));
            boot.setInitialRamdiskURL(Some(&url(initramfs)));
            boot.setCommandLine(&NSString::from_str(command_line));

            let config = VZVirtualMachineConfiguration::new();
            config.setBootLoader(Some(&boot));
            config.setPlatform(&VZGenericPlatformConfiguration::new());
            config.setCPUCount(cpus);
            config.setMemorySize(memory);

            let share = |tag: &str, folder: &Path, read_only: bool| {
                let device = VZVirtioFileSystemDeviceConfiguration::initWithTag(
                    VZVirtioFileSystemDeviceConfiguration::alloc(),
                    &NSString::from_str(tag),
                );
                let directory = VZSharedDirectory::initWithURL_readOnly(
                    VZSharedDirectory::alloc(),
                    &url(folder),
                    read_only,
                );
                let single = VZSingleDirectoryShare::initWithDirectory(
                    VZSingleDirectoryShare::alloc(),
                    &directory,
                );
                device.setShare(Some(&single));
                Retained::into_super(device)
            };
            config.setDirectorySharingDevices(&NSArray::from_retained_slice(&[
                share("sandbox", sandbox, false),
                share("rootfs", rootfs, true),
            ]));

            let attachment = VZDiskImageStorageDeviceAttachment::initWithURL_readOnly_error(
                VZDiskImageStorageDeviceAttachment::alloc(),
                &url(disk),
                false,
            )
            .map_err(|e| format!("the disk {}: {}", disk.display(), described(&e)))?;
            let block = VZVirtioBlockDeviceConfiguration::initWithAttachment(
                VZVirtioBlockDeviceConfiguration::alloc(),
                &attachment,
            );
            config.setStorageDevices(&NSArray::from_retained_slice(&[Retained::into_super(
                block,
            )]));

            let network = VZVirtioNetworkDeviceConfiguration::new();
            network.setAttachment(Some(&VZNATNetworkDeviceAttachment::new()));
            config.setNetworkDevices(&NSArray::from_retained_slice(&[Retained::into_super(
                network,
            )]));

            config.setSocketDevices(&NSArray::from_retained_slice(&[Retained::into_super(
                VZVirtioSocketDeviceConfiguration::new(),
            )]));

            let handle = NSFileHandle::initWithFileDescriptor_closeOnDealloc(
                NSFileHandle::alloc(),
                std::os::fd::IntoRawFd::into_raw_fd(console),
                true,
            );
            let serial = VZVirtioConsoleDeviceSerialPortConfiguration::new();
            serial.setAttachment(Some(&VZFileHandleSerialPortAttachment::initWithFileHandleForReading_fileHandleForWriting(
                VZFileHandleSerialPortAttachment::alloc(),
                None,
                Some(&handle),
            )));
            config.setSerialPorts(&NSArray::from_retained_slice(&[Retained::into_super(
                serial,
            )]));

            config.setEntropyDevices(&NSArray::from_retained_slice(&[Retained::into_super(
                VZVirtioEntropyDeviceConfiguration::new(),
            )]));

            config.validateWithError().map_err(|e| described(&e))?;
            Ok(VZVirtualMachine::initWithConfiguration_queue(
                VZVirtualMachine::alloc(),
                &config,
                queue,
            ))
        }
    }

    impl Vm {
        fn on_queue<F: FnOnce(&VZVirtualMachine) + Send + 'static>(&self, work: F) {
            let vm = OnQueue(self.vm.0.clone());
            self.queue.exec_async(move || {
                let vm = vm;
                work(&vm.0);
            });
        }

        /// Connects to the guest's `port`, trying until it listens or
        /// `within` has passed; answers the connection's own descriptor.
        pub fn connect(&self, port: u32, within: Duration) -> Result<OwnedFd, String> {
            let deadline = Instant::now() + within;
            loop {
                let (tx, rx) = mpsc::channel::<Result<OwnedFd, String>>();
                self.on_queue(move |vm| {
                    // SAFETY: socketDevices on the VM's queue; the configuration
                    // has one virtio socket device.
                    let devices = unsafe { vm.socketDevices() };
                    let device = devices.firstObject().expect("one socket device");
                    let device: Retained<VZVirtioSocketDevice> =
                        Retained::downcast(device).expect("a virtio socket device");
                    let block = RcBlock::new(
                        move |connection: *mut VZVirtioSocketConnection, error: *mut NSError| {
                            // SAFETY: the framework passes a valid connection or a
                            // valid error; the connection's descriptor is open while
                            // the connection lives, and dup gives one of our own.
                            let result = unsafe {
                                match (connection.as_ref(), error.as_ref()) {
                                    (Some(connection), _) => {
                                        let fd = libc::dup(connection.fileDescriptor());
                                        if fd < 0 {
                                            Err(std::io::Error::last_os_error().to_string())
                                        } else {
                                            Ok(OwnedFd::from_raw_fd(fd))
                                        }
                                    }
                                    (None, Some(error)) => Err(described(error)),
                                    (None, None) => Err("no connection and no error".to_string()),
                                }
                            };
                            let _ = tx.send(result);
                        },
                    );
                    // SAFETY: on the VM's queue, with a block the framework retains.
                    unsafe { device.connectToPort_completionHandler(port, &block) };
                });
                match rx.recv_timeout(Duration::from_secs(10)) {
                    Ok(Ok(fd)) => return Ok(fd),
                    Ok(Err(error)) if Instant::now() > deadline => {
                        return Err(format!(
                            "the agent did not answer on vsock port {port}: {error}"
                        ));
                    }
                    Err(_) => return Err(format!("connecting to vsock port {port} did not end")),
                    Ok(Err(error)) if self.ended() => {
                        return Err(format!(
                            "the VM stopped before its agent answered ({error})"
                        ));
                    }
                    Ok(Err(_)) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
        }

        /// Stops the VM (the framework's forced stop; nothing in the guest
        /// needs a clean end), waits until the framework says it stopped,
        /// and releases the framework's object on its queue.
        pub fn stop(self) {
            let (tx, rx) = mpsc::channel::<()>();
            self.on_queue(move |vm| {
                // SAFETY: on the VM's queue; the block lives as long as the
                // framework retains it.
                unsafe {
                    if vm.canStop() {
                        let block = RcBlock::new(move |_: *mut NSError| {
                            let _ = tx.send(());
                        });
                        vm.stopWithCompletionHandler(&block);
                    } else {
                        let _ = tx.send(());
                    }
                }
            });
            let _ = rx.recv_timeout(Duration::from_secs(10));
            let (tx, rx) = mpsc::channel::<()>();
            let vm = self.vm;
            self.queue.exec_async(move || {
                drop(vm);
                let _ = tx.send(());
            });
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }

        /// The VM's state is stopped or error.
        pub fn ended(&self) -> bool {
            let (tx, rx) = mpsc::channel();
            self.on_queue(move |vm| {
                // SAFETY: on the VM's queue.
                let state = unsafe { vm.state() };
                let _ = tx.send(
                    state == VZVirtualMachineState::Stopped
                        || state == VZVirtualMachineState::Error,
                );
            });
            rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false)
        }
    }
}

/// A started VM.
#[cfg(target_os = "macos")]
struct Machine {
    vm: vz::Vm,
    /// The disk image this VM has (`.vm/disk-<n>.img`).
    disk: PathBuf,
    /// The lock of that disk (`.vm/disk-<n>.lock`), held while the VM runs
    /// and until the framework has let go of the image.
    lock: std::fs::File,
}

#[cfg(target_os = "macos")]
impl Machine {
    /// Unpacks the guest base, makes the disk, boots the VM and connects to
    /// the agent.
    fn boot(
        root: &Path,
        rootfs: &(PathBuf, String),
        guest: &GuestBase,
    ) -> Result<(Machine, Connection), String> {
        let base = unpack_guest(guest, root)?;
        let vm_dir = root.join(".vm");
        std::fs::create_dir_all(&vm_dir).map_err(|e| format!("{}: {e}", vm_dir.display()))?;
        let (n, lock) = take_disk(&vm_dir)?;
        let disk = vm_dir.join(format!("disk-{n}.img"));
        let console = vm_dir.join(format!("console-{n}.log"));
        if !disk.exists() {
            let file =
                std::fs::File::create(&disk).map_err(|e| format!("{}: {e}", disk.display()))?;
            file.set_len(DISK_BYTES)
                .map_err(|e| format!("{}: {e}", disk.display()))?;
        }
        // A VM of a process that ended without stopping it (killed) holds
        // the image until the framework's VM process has gone.
        if !image_released(&disk, Duration::from_secs(10)) {
            return Err(format!(
                "the disk {} is still held by an earlier VM",
                disk.display()
            ));
        }
        let (tarball, sha256) = rootfs;
        let rootfs_folder = tarball
            .parent()
            .ok_or_else(|| format!("the root filesystem {} has no folder", tarball.display()))?;
        let rootfs_name = tarball
            .file_name()
            .ok_or_else(|| format!("the root filesystem {} has no name", tarball.display()))?
            .to_string_lossy()
            .into_owned();
        if !tarball.is_file() {
            return Err(format!(
                "the root filesystem {} is missing",
                tarball.display()
            ));
        }
        let (uid, gid) = GUEST_USER;
        let command_line = format!(
            "console=hvc0 quiet panic=-1 mcbench.user={uid}:{gid} mcbench.port={PORT} mcbench.rootfs={rootfs_name} mcbench.sha256={sha256}"
        );
        let vm = vz::start(vz::Parts {
            kernel: &base.join("kernel"),
            initramfs: &base.join("initramfs"),
            command_line,
            sandbox: root,
            rootfs_folder,
            disk: &disk,
            console: &console,
            cpus: CPUS,
            memory: MEMORY,
        })?;
        let started = Instant::now();
        match vm.connect(PORT, Duration::from_secs(60)) {
            Ok(connection) => Ok((Machine { vm, disk, lock }, connection)),
            Err(error) => {
                let ended = vm.ended();
                Machine { vm, disk, lock }.stop();
                let console = std::fs::read_to_string(&console).unwrap_or_default();
                let last = console
                    .lines()
                    .rev()
                    .find(|l| l.starts_with("mcbench guest init:") || l.contains("Kernel panic"));
                Err(match (ended, last) {
                    (_, Some(line)) => line.to_string(),
                    (true, None) => format!(
                        "the VM stopped after {:?} before its agent answered",
                        started.elapsed()
                    ),
                    (false, None) => error,
                })
            }
        }
    }

    /// Stops the VM and returns once the framework has stopped it and let
    /// go of the disk image (the framework holds an exclusive `flock` on an
    /// image it has attached); the disk's lock is released only then, so
    /// the next VM on the sandbox folder finds the image free.
    fn stop(self) {
        self.vm.stop();
        image_released(&self.disk, Duration::from_secs(10));
        drop(self.lock);
    }
}

/// Waits until no process holds the image's `flock` (the framework takes an
/// exclusive one on an image it attaches), at most `within`; whether it is
/// free.
#[cfg(target_os = "macos")]
fn image_released(image: &Path, within: Duration) -> bool {
    use std::os::fd::AsRawFd;
    let deadline = Instant::now() + within;
    loop {
        let Ok(file) = std::fs::File::open(image) else {
            return true;
        };
        // SAFETY: flock on a descriptor we hold; the lock is let go at once.
        unsafe {
            if libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
                return true;
            }
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The agent's vsock connection.
#[cfg(target_os = "macos")]
type Connection = std::os::fd::OwnedFd;
/// None elsewhere.
#[cfg(not(target_os = "macos"))]
type Connection = std::convert::Infallible;

#[cfg(target_os = "macos")]
fn connection_stream(connection: Connection) -> crate::agent::FdStream {
    crate::agent::FdStream::new(connection).expect("the vsock connection")
}

#[cfg(not(target_os = "macos"))]
fn connection_stream(connection: Connection) -> tokio::io::DuplexStream {
    match connection {}
}

/// The first disk of `vm_dir` no other VM holds: its number and its lock
/// (`flock`, released when the file is closed or the process ends).
#[cfg(target_os = "macos")]
fn take_disk(vm_dir: &Path) -> Result<(usize, std::fs::File), String> {
    use std::os::fd::AsRawFd;
    for n in 0..16 {
        let path = vm_dir.join(format!("disk-{n}.lock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: flock on a descriptor we hold.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok((n, file));
        }
    }
    Err(format!(
        "16 VMs already use the disks of {}",
        vm_dir.display()
    ))
}

/// The VM elsewhere: the Virtualization framework is macOS's.
#[cfg(not(target_os = "macos"))]
struct Machine;

#[cfg(not(target_os = "macos"))]
impl Machine {
    fn boot(
        _: &Path,
        _: &(PathBuf, String),
        _: &GuestBase,
    ) -> Result<(Machine, Connection), String> {
        Err("the Virtualization framework is macOS's".to_string())
    }
    fn stop(self) {}
}

/// Unpacks the guest base's tar.zst (pinned by its sha256) under `root`,
/// once: a folder `.guest-<sha256>` holding a `.complete` marker is taken as
/// it is.
#[cfg(any(target_os = "macos", windows))]
pub(crate) fn unpack_guest(guest: &GuestBase, root: &Path) -> Result<PathBuf, String> {
    let dir = root.join(format!(".guest-{}", guest.sha256));
    if dir.join(".complete").exists() {
        return Ok(dir);
    }
    use sha2::Digest;
    let bytes = std::fs::read(&guest.asset)
        .map_err(|e| format!("the guest base {}: {e}", guest.asset.display()))?;
    let got: String = sha2::Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != guest.sha256 {
        return Err(format!(
            "the guest base {} has sha256 {got}, not the pinned {}",
            guest.asset.display(),
            guest.sha256
        ));
    }
    let partial = root.join(format!(
        ".guest-{}.partial-{}",
        guest.sha256,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    let decoder = ruzstd::decoding::StreamingDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("the guest base: {e}"))?;
    tar::Archive::new(decoder)
        .unpack(&partial)
        .map_err(|e| format!("unpacking the guest base: {e}"))?;
    for name in ["kernel", "initramfs"] {
        if !partial.join(name).is_file() {
            return Err(format!("the guest base has no {name}"));
        }
    }
    std::fs::write(partial.join(".complete"), &guest.sha256).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}
