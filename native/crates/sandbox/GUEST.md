# The guest agent's protocol

The macOS VM runs `mcbench-sandbox-agent guest <vsock port> <uid>:<gid> <cgroup folder> <sandbox folder> <rootfs.tar.zst> <sha256>` (started as root by the guest base's `/init`, which delegated the cgroup folder; the agent raises its hard open-file limit to the boxes' 65536, which the kernel starts at 4096 and a user cannot raise, becomes the user and serves the host's one vsock connection), and `mcbench-sandbox-agent serve <sandbox folder> <rootfs.tar.zst> <sha256>` (or `mcbench-lite sandbox-agent serve ...`) serves on its stdin and stdout with each box in a systemd user scope: the Linux sandbox of `src/linux` behind this protocol either way. The host's side is `agent::GuestClient`, a `Backend` over any `AsyncRead` and `AsyncWrite` pair (a vsock, a virtio console, the pipes of `wsl.exe`). The lifecycle (each chat's state, the storage watchdog, the idle reaper) stays on the host beside the chats' storage; the guest only runs boxes.

The program is a static musl binary for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` (`cargo build --release -p mcbench-sandbox --bin mcbench-sandbox-agent --target <t>`, the aarch64 one with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`; 2.6 and 2.9 MB unstripped on 2026-11-17). The whole `mcbench-lite` binary is not the guest program: it links the renderer, GPUI and V8, which have no musl or aarch64-Linux build in this workspace, and the guest needs none of them. The same program is also every box's keeper (`keep`, `netinit`) and, inside Codex's box, its network relay (`relay`) and its tool bridge (`tool-bridge <socket>`, the `mcbench-relay` crate's relay that `mcbench-lite tool-bridge` runs too).

The guest needs: a hard open-file limit of at least 65536 for the agent (every box's keeper sets the box's soft and hard limit to it, `linux::keeper::RLIMITS`, and refuses the box otherwise; `guest` raises its own, `serve` takes the one it is given), Linux with unprivileged user namespaces, bubblewrap, a cgroup v2 folder with the `memory`, `pids` and `cpu` controllers delegated to the agent's user (`guest`; the agent runs in a leaf beside the boxes' folders) or a user systemd session that delegates them (`serve`), `nf_tables` with `nft_compat` and `REJECT` for the networked command's rules, `/dev/net/tun` open to the agent's user, and the chats' folders at the paths the host's `WorkMap` gives.

A guest's box starts in a user and mount namespace of its own (the caller's uid and gid mapped to themselves) holding an empty `binfmt_misc`, mounted on an empty folder of the box's run folder (`binfmt`, `linux::keeper::own_binfmt`), so no handler of the machine's (WSL's `WSLInterop`, which hands `MZ` files to WSL's `/init`) reaches it and an `MZ` file answers bash's `cannot execute binary file: Exec format error` and 126 as in today's Docker box; `ENODEV` from the mount (no such filesystem type: no `binfmt_misc` in the kernel, so no handler anywhere) starts the box, any other error (`ENOENT` included: the folder is always there) refuses it. `binfmt_misc` is deliberately not in the macOS VM's modules (`guest/build.py` `MODULES`): absent, no handler can be registered in that VM at all, and loading it only to mount an empty instance would add kernel code that hides nothing. The kernel's config has `CONFIG_BINFMT_MISC=m`, `MODULES` does not list it, `guest/init` loads only `MODULES` (with `insmod`), and the userland has no `modprobe` for the kernel's `request_module`: in the macOS VM the mount answers success or `ENODEV`, both allowed, and nothing reads which.

## The guest base

`guest/build.sh OUT aarch64|x86_64` makes it: Debian 13's cloud kernel, busybox-static, bubblewrap and the C libraries it links (pinned by sha256 at snapshot.debian.org 20261117T000000Z), the static agent, `guest/init` and `guest/udhcpc.sh`, as a kernel, a newc initramfs (with the kernel modules the boxes need) and the same userland as `userland.tar`, in one tar.zst. Two builds on adipc gave the same asset byte for byte (aarch64 `ebc31c6d…`, 15.1 MB; x86_64 `7bf304c9…`, 16.2 MB). The x86_64 userland also carries `prlimit` (util-linux), `libsmartcols` and `libgcc_s` for the build child the WSL2 distro runs. Since the WSL2 distro (the agent's `guest -`, those three files) and the box's own resource limits (the `guest` command raises its hard open-file limit to 65536 as root first, in the VM and in the distro) the assets are aarch64 `238d181e256e870f1365dfd29c17481ad5411085ebbdcfd9f0f2da62dfcb3a2b` (15,143,613 bytes) and x86_64 `479d74b5c8d1198d13d014ed82b3057f95bb9b35a29fa5b51ffe26f422847e78` (17,740,232 bytes), built on adipc on 2026-11-18 from `s3-wsl3w` 7c256f93 (the keeper's refused calls and a guest box's own binfmt_misc). The macOS VM boots the kernel and the initramfs; `guest/init` is the VM's: what it mounts, where, and what it gives the agent is written at its top.

## The WSL2 distro

What it reuses: `userland.tar` (busybox, bubblewrap and its libraries, the static agent), the agent's `guest` command with `-` for its stream (stdin and stdout, the pipes of `wsl.exe`) and its `Delegation::Subtree`, and the whole Linux implementation behind it. Microsoft's kernel replaces the guest base's (the modules list is not used: `nft_compat` and `ipt_REJECT` are loaded from WSL's own modules disk). WSL's init replaces `guest/init`; `guest/wsl-init` does the init's part for the boxes: the drvfs mounts of the sandbox folder (`/mnt/sandbox`) and the root filesystem's folder (`/mnt/rootfs`), `/var/mcbench` on the distro's own disk with `.run` in memory, the netfilter modules, and a cgroup folder `mcbench-<distro>` with `memory`, `pids` and `cpu` in the distro's own cgroup namespace, then the agent. The import adds `/etc/wsl.conf`, the user `mcbench` 1000:1000, `/usr/bin/sh`, `/usr/bin/mount` and `/usr/sbin` (`src/windows.rs` `import_tar`). No clock keeper: WSL keeps the distro's clock on Windows'.

## Frames

Every frame is a 4-byte big-endian length (of what follows), one kind byte, and the body. A frame is at most 16 MiB.

| Kind | Direction | Body |
|---|---|---|
| 1 `CALL` | host to guest | JSON `{"id": n, "call": "<name>", ...arguments}` |
| 2 `ANSWER` | guest to host | JSON `{"id": n, "<answer>": value}`, one per call, in any order (`tests` in `src/agent.rs` hold these forms) |
| 3 `OUTPUT` | guest to host | the call's id (8 bytes, big-endian), then bytes a running command wrote, as they arrive |
| 4 `CANCEL` | host to guest | JSON `{"id": n}`: cancel that call |

Calls run concurrently; ids are the host's. The stream's end ends the agent, which removes every box first.

## Calls

Each is `Backend`'s method of the same name (`src/backend.rs` says what each does).

| `call` | Arguments | Answer |
|---|---|---|
| `prepare` | | `{"done": null}` |
| `ensure` | `chat`, `work`, `network` | `{"done": null}` |
| `has_session` | `chat`, `work` | `{"bool": b}` |
| `session` | `chat`, `work`, `command`, `ms` | `{"session": {"text", "exit_code", "timed_out", "reset"}}`; `OUTPUT` frames while it runs |
| `kill_session` | `chat` | `{"done": null}` |
| `one_shot` | `chat`, `work`, `network`, `command`, `ms`, `limits` | `{"one_shot": {"output", "code"}}`; `OUTPUT` frames while it runs |
| `end_processes` | `chat`, `network` | `{"done": null}` |
| `program` | `chat`, `work`, `argv`, `timeout_ms` | `{"program": {"stdout", "all", "code"}}`, the bytes as base64 |
| `remove` | `chat` | `{"done": null}` |
| `shutdown` | | `{"done": null}` |

`work` is the chat's `/work` folder in the guest. `ms` is the limit in milliseconds as a JavaScript number (it may be fractional). Any call can answer `{"unavailable": message}`, `{"cancelled": null}` or `{"failed": message}`: `SandboxError`'s three kinds, the message being what the model reads.

A `CANCEL` of a `session` or `one_shot` acts as the cancellation token does on the host: the session's command is abandoned (the host then resets the shell), a one-shot's box has its processes ended, and the call answers `{"cancelled": null}`.

## Proof

`MCBENCH_SANDBOX_VIA_AGENT=1 cargo test -p mcbench-sandbox --test linux_box -- --ignored` runs the integration tests a second time through `GuestClient` talking to `mcbench-sandbox-agent serve` over a pipe (adipc, 2026-11-17: 8 of 8 pass both ways). On Windows the same tests, `network`, `native_diff` and `windows_wsl` run against the distro (`MCBENCH_WSL_DISTRO` names the test's): in the Windows VM on 2026-11-17 as `plain` and as `bench`, linux_box 6 of 6, windows_wsl 4 of 4, network 2 of 2, the 106 cases 103 identical to the Linux implementation and 3 differing by the machine only. On macOS the same tests, `network`, `native_diff` and `macos_vm` run against the VM (`src/lib.rs` gives the command; 2026-11-17 on the Mac: linux_box 5 of 5 run, network 2 of 2, macos_vm 4 of 4, the 106 cases 101 identical to the Linux implementation on adipc and 5 differing by the machine only, listed in `tests/native_diff.rs`).
