#!/usr/bin/env bash
# native/check.sh: the checks every session runs before it merges into native-app.
#
#   native/check.sh                   cargo fmt --check, cargo clippy -D warnings, cargo test and
#                                     cargo doc (warnings are errors) for the workspace in native/,
#                                     and fmt, clippy and test of the build child's own workspace
#                                     (native/build-child, built first), with the pinned toolchain, here
#   native/check.sh --jobs N --remote adipc --slice mcbench-build-<session>.slice
#                                    the same here and on adipc, and both results printed
#   native/check.sh --windows --slice mcbench-build-<session>.slice
#                                     also cargo clippy -D warnings and cargo test for
#                                     x86_64-pc-windows-msvc in the Windows 11 VM on adipc,
#                                     with real MSVC, and the WSL tests (see "The Windows
#                                     check" and "The WSL tests" below)
#   native/check.sh --windows-only --slice mcbench-build-<session>.slice
#                                     the Windows half alone, from the Mac or adipc: the tree as git
#                                     lists it goes to the VM's check slot, and nothing runs here
#                                     (no local half, so the Mac's turn is never taken)
#   native/check.sh --node            also npm test in this checkout (the Node app keeps passing)
#   native/check.sh --macos-vm        Mac only: also the bash sandbox's VM tests (see "The macOS
#                                     VM tests" below)
#
# The flags combine. It exits non-zero when any check fails on any machine.
# With --remote adipc --windows the Windows part runs once, after both
# machines' checks, so the VM's build never runs beside adipc's.
#
# Where it builds. Nothing goes inside the repository, and nothing into
# ~/.mcbench-lite/tools (the app's live tools folder): the script refuses both.
#
#   MCBENCH_NATIVE_CHECK_HOME  default $HOME/.cache/mcbench-native-check
#   RUSTUP_HOME                default $MCBENCH_NATIVE_CHECK_HOME/rustup: the pinned toolchain
#                              (native/rust-toolchain.toml) with clippy and rustfmt
#   CARGO_HOME                 default $MCBENCH_NATIVE_CHECK_HOME/cargo: rustup itself and the registry
#   CARGO_TARGET_DIR           default $MCBENCH_NATIVE_CHECK_HOME/target-<branch>
#
# The tests' TMPDIR is always $MCBENCH_NATIVE_CHECK_HOME/t/<the branch name's cksum>
# (a short name of fixed length: tests put Unix sockets under it, whose path
# must stay shorter than SUN_LEN, 104 bytes on macOS and 108 on Linux), emptied at
# the start of each run and kept after it, so what a failed test left there can
# be read. It cannot be set from outside: the script deletes it, and a folder
# named by a variable could be anyone's.
#   --jobs N                  required, 1 to 8: CARGO_BUILD_JOBS and RUST_TEST_THREADS,
#                              on every machine (adipc's pool allows at most -j 8 for
#                              one build; native/README.md).
#   --slice NAME               required with --remote, --windows and --windows-only (the
#                              WSL tests' Linux files are built on adipc): the caller's session's slice on
#                              adipc, mcbench-build-<session>.slice (mcbench-build-s2.slice
#                              and so on). It is a child of mcbench-build.slice, whose
#                              MemoryHigh and MemoryMax bound every session's builds and
#                              tests together, inside mcbench.slice, whose CPUQuota bounds
#                              them and the Windows VMs (native/README.md); the remote run
#                              is a scope in it with no limit of its own. The run fails at
#                              once, saying how to set them, when mcbench.slice has no
#                              CPUQuota or mcbench-build.slice no MemoryMax (runtime
#                              properties, lost on a reboot).
#
# Each is taken from the environment when set. rustup is installed into
# CARGO_HOME/bin from rustup-init 1.29.3 (static.rust-lang.org, checked
# against the sha256 server/tools.js pins) the first time, and the pinned
# toolchain into RUSTUP_HOME; later runs reuse both.
#
# On adipc everything is under ~/mcbench-native-scratch: the tree is brought
# by content (see "Why the trees are synced by content") to check-<branch>/tree
# (git's tracked and untracked-but-not-ignored files of native/, core/,
# renderer/, server/ and README.md), built into check-<branch>/target, with the
# toolchain and registry shared by every branch in check-toolchain/. Remove a
# branch's folder with rm -rf ~/mcbench-native-scratch/check-<branch> when it
# is merged. adipc's kiosk session is not touched; the remote run is a
# transient systemd user scope in the caller's --slice, under mcbench-build.slice.
#
# The Windows check. The workspace has C code (the bundled SQLite of
# rusqlite, ring, V8's prebuilt static library), so it is built where MSVC
# is: the Windows 11 VM on adipc (native/spikes/winvm on the spikes branch,
# installed in ~/mcbench-native-scratch/winvm). From the machine it is
# started on (the Mac or adipc, a git checkout) the tree (the files of
# native/, core/, renderer/, server/ and README.md as git lists them) goes by rsync to
# adipc:~/mcbench-native-scratch/check-<branch>/windows-tree, and
# native/check.sh --windows-vm runs there from that copy. It
#   runs in the VM's check slot (WINVM_SLOT=check: check.sh uses the check
#   slot; everything else takes the hold slot; each slot has its own LOCK;
#   the slots' table is in native/tools/winvm/env.sh) and takes that slot's
#   LOCK (~/mcbench-native-scratch/winvm/LOCK-check; one user at a time) with
#   ~/mcbench-native-scratch/winvm/scripts/lock.sh, which every use of the
#   VM goes through, waiting up to an hour with a line each time
#   the holder changes, then fails quoting the holder's line and saying how
#   to clear a stale LOCK, and exports WINVM_LOCK_LINE, without which the
#   winvm scripts and env.sh's functions refuse; the installed lock.sh,
#   env.sh, start.sh, stop.sh, reset.sh and snapshot.sh must be the
#   repository's native/tools/winvm/ copies, or the check fails saying so;
#   makes a fresh overlay of the base win11-tools for this run alone
#   (~/mcbench-native-scratch/check-windows/run: disk, OVMF vars, TPM
#   state; the base is never written) and starts the VM on it, headless,
#   with the check's cache disk as a second drive;
#   brings the tree kept on the cache disk (mcbcache's \tree) to the run's
#   tree by content (below) and runs cargo clippy and cargo test
#   of its whole workspace with the pinned toolchain (rustup reads
#   native/rust-toolchain.toml), without debug information or incremental
#   compilation (CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0,
#   CARGO_PROFILE_TEST_DEBUG=0: a check needs neither, and MSVC's PDB files
#   and the incremental cache would fill the cache disk), and prints what
#   the cache disk's registry, targets and tree take;
#   stops the VM, deletes the run's overlay and the tree's .tar and .ps1 and
#   releases the LOCK on every exit, failed or interrupted (one trap).
# What is kept between runs, and why: the cache disk
# ~/mcbench-native-scratch/check-windows/cache.qcow2 (a sparse 40 GiB qcow2,
# an NTFS volume labelled mcbcache, made and formatted when missing), shared
# by every session's checks as check-toolchain is, holds only cargo's
# registry (CARGO_HOME) and the target folder, which keeps V8's downloaded
# static library (about 200 MB) and the built dependencies: with it a run is
# incremental, as the target folder is kept on the Mac and on adipc; without
# it every run downloads and builds all again. Deleting it is always safe.
# Windows itself starts clean every run. It is bounded: when a run starts
# and the disk takes more than MCBENCH_WINDOWS_CACHE_MAX_GB on adipc
# (qemu-img's "disk size"; default 20, decimals allowed), the run deletes it
# and says so, and that run is cold.
# Measured on adipc at bb59785b with 4 cores: a cold cache 1,064 s and 13 GB
# on disk afterwards (sparse, so it takes only what was written); a warm one
# 510 s after edits to two crates. A tree with the interface's GPUI adds
# about 800 crates to a cold build. Nothing bounds the build's own time:
# only the LOCK wait (an hour) and the guest's ssh (2,400 s to boot) are
# bounded.
# The VM runs inside mcbench.slice's pool, in mcbench-vm.slice, beside the
# builds and outside their memory ceiling: with --jobs N
# QEMU runs pinned to the check slot's four host threads (28-31, env.sh's
# vm_start pins them and starts it in a scope of its own), and the guest runs cargo with min(N, 4) jobs and test
# threads. QEMU is found and stopped only by the pid file its start writes.
#
# The WSL tests. On Windows the bash sandbox and the build child run in the
# app's WSL2 distro (crates/sandbox/src/windows.rs, crates/build/src/sandbox/
# windows.rs), so the guest needs WSL and three Linux files. The check slot's
# overlay is made of win11-tools-wsl (win11-tools with WSL installed once and
# Windows restarted, a qcow2 beside it whose backing file is win11-tools;
# neither is ever written). Before the LOCK is taken, adipc makes them
# (check.sh --wsl-assets, in a scope of the caller's --slice, from
# check-<branch>/tree brought by content as the adipc half's, with the
# scratch's toolchain and the branch's target check-<branch>/target), into
# ~/mcbench-native-scratch/check-<branch>/wsl-assets/run: the x86_64 guest
# base (crates/sandbox/guest/build.sh, its packages kept in wsl-assets/guest),
# the Linux x86_64 build child (native/build-child, into
# <target>-build-child) and the Linux build of mcbench-build's child test
# binary (the escape probe), each refused when it needs a glibc symbol
# version that the libc.so.6 of the guest base just built does not define;
# and it copies the x86_64 root filesystem installed once in
# check-windows/assets (native/README.md says how; the check fails naming
# the file when it is not there). The guest gets them as C:\mcb\assets, and its
# test steps run with MCBENCH_BUILD_CHILD and MCBENCH_BUILD_PROBE (the Linux
# programs), MCBENCH_SANDBOX_ROOTFS, MCBENCH_SANDBOX_GUEST (with their
# _SHA256), MCBENCH_SANDBOX_SCRATCH (a folder on C:, so the test distro is
# imported once per run) and MCBENCH_WSL_DISTRO=mcbench-check. The "wsl"
# step runs before the workspace's tests: it imports the test distro (the
# build child's tests then find it registered) and runs the sandbox's WSL
# tests, which are ignored elsewhere: cargo test -p mcbench-sandbox --test
# windows_wsl --test linux_box -- --ignored --nocapture --test-threads 1.
#
# Why the trees are synced by content. Cargo decides that a kept target's
# build of a path crate is fresh by comparing the files' modification times
# with that build's, so a tree copied with its own (older) times over a
# target that another branch or a later commit built would be compiled
# against that other build's crates. Both kept targets are therefore fed by
# content, never by times: on adipc (--remote) the run's files go to
# check-<branch>/stage and `rsync -rlp --checksum --delete` without -t
# brings check-<branch>/tree to it; in the Windows guest the run's tar is
# unpacked to the cache disk's \stage and, for every file of the tar's own
# list, a file of the kept \tree whose size or SHA-256 differs, or that is
# missing, is written with the time now, identical ones keep their times, and
# files not in the list are deleted. Exactly the changed files are then newer
# than the last build, whatever branch or commit ran before, while the
# registry's crates and unchanged path crates stay built. No target folder is
# inside a tree.
#
# The macOS VM tests. --macos-vm (on a Mac only; elsewhere it fails saying
# so) also runs the integration tests of the bash sandbox against the app's
# Linux VM (crates/sandbox/src/lib.rs): cargo test -p mcbench-sandbox --test
# macos_vm --test linux_box --test network --test native_diff -- --ignored
# --test-threads 1, with crates/sandbox/macos/run-signed.sh as cargo's runner
# (it signs the test binary ad hoc with the virtualization entitlement, which
# shows nothing and asks nothing; the VM has no display device). It needs
#   MCBENCH_SANDBOX_GUEST, MCBENCH_SANDBOX_GUEST_SHA256
#                              the guest base's tar.zst (crates/sandbox/guest/build.sh)
#                              and its sha256
#   MCBENCH_SANDBOX_ROOTFS, MCBENCH_SANDBOX_ROOTFS_SHA256
#                              the aarch64 root filesystem's tar.zst and its sha256
# and fails at once naming the one that is not set. It sets
# MCBENCH_SANDBOX_NETWORK=1 (the network test reaches the internet) and
# MCBENCH_SANDBOX_SCRATCH, default $MCBENCH_NATIVE_CHECK_HOME/sandbox-vm, kept
# between runs so the root filesystem is unpacked once.
# Windows on ARM is not checked here: the C build scripts need an ARM64
# C toolchain; the release pipeline's windows-11-arm runner builds it.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
REMOTE=""
WINDOWS=0
WINDOWS_ONLY=0
WINDOWS_VM=0
WSL_ASSETS=0
NODE=0
MACOS_VM=0
JOBS=""
SLICE=""
while [ $# -gt 0 ]; do
  case "$1" in
    --remote) REMOTE="${2:?--remote needs a host}"; shift 2 ;;
    --windows) WINDOWS=1; shift ;;
    --windows-only) WINDOWS=1; WINDOWS_ONLY=1; shift ;;
    --windows-vm) WINDOWS_VM=1; shift ;;
    --wsl-assets) WSL_ASSETS=1; shift ;;
    --node) NODE=1; shift ;;
    --macos-vm) MACOS_VM=1; shift ;;
    --jobs) JOBS="${2:?--jobs needs a number}"; shift 2 ;;
    --slice) SLICE="${2:?--slice needs a slice name}"; shift 2 ;;
    *) echo "native/check.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

if [ "$WINDOWS_ONLY" = 1 ] && { [ -n "$REMOTE" ] || [ "$NODE" = 1 ] || [ "$MACOS_VM" = 1 ]; }; then
  echo "native/check.sh: --windows-only runs the Windows half alone; it takes no --remote, --node or --macos-vm" >&2
  exit 2
fi
if { [ -n "$REMOTE" ] || [ "$WINDOWS" = 1 ]; }; then
  case "$SLICE" in
    mcbench-build-?*.slice) ;;
    *) echo "native/check.sh: --remote, --windows and --windows-only need --slice mcbench-build-<session>.slice, the caller's session's slice under mcbench-build.slice (native/README.md); got '$SLICE'" >&2; exit 2 ;;
  esac
fi
if [ "$WINDOWS" = 1 ] && ! git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "native/check.sh: --windows needs a git checkout: it sends the VM the files git lists, and $ROOT is not one" >&2
  exit 2
fi

if [ "$MACOS_VM" = 1 ]; then
  if [ "$(uname -s)" != Darwin ]; then
    echo "native/check.sh: --macos-vm runs the bash sandbox's tests against the macOS VM and so only on a Mac; this machine runs $(uname -s)" >&2
    exit 2
  fi
  for var in MCBENCH_SANDBOX_GUEST MCBENCH_SANDBOX_GUEST_SHA256 MCBENCH_SANDBOX_ROOTFS MCBENCH_SANDBOX_ROOTFS_SHA256; do
    if [ -z "${!var:-}" ]; then
      echo "native/check.sh: --macos-vm needs $var (the guest base's tar.zst and the aarch64 root filesystem's, each with its sha256; see \"The macOS VM tests\" in this script's header)" >&2
      exit 2
    fi
  done
fi

CHANNEL="$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$HERE/rust-toolchain.toml")"
[ -n "$CHANNEL" ] || { echo "native/check.sh: no channel in native/rust-toolchain.toml" >&2; exit 2; }
BRANCH="${MCBENCH_NATIVE_CHECK_BRANCH:-$(git -C "$ROOT" rev-parse --abbrev-ref HEAD | tr '/' '-')}"

CHECK_HOME="${MCBENCH_NATIVE_CHECK_HOME:-$HOME/.cache/mcbench-native-check}"
export RUSTUP_HOME="${RUSTUP_HOME:-$CHECK_HOME/rustup}"
export CARGO_HOME="${CARGO_HOME:-$CHECK_HOME/cargo}"
# On the Mac one target serves every session and branch (one Mac half runs
# at a time, under the Mac's turn, and third-party crates are most of a
# target); elsewhere one target per branch. The Mac's target is bounded below.
if [ "$(uname -s)" = Darwin ]; then
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$CHECK_HOME/target-mac}"
else
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$CHECK_HOME/target-$BRANCH}"
fi
case "$JOBS" in
  [1-8]) ;;
  "") echo "native/check.sh: --jobs N is required, 1 to 8 (native/README.md)" >&2; exit 2 ;;
  *) echo "native/check.sh: --jobs $JOBS is outside 1 to 8 (adipc's pool allows at most -j 8 for one build)" >&2; exit 2 ;;
esac
LOCAL_JOBS="$JOBS"
# The Mac half (native/README.md, the Mac rule): one at a time across every
# session, taken as the folder ~/.cache/mcbench-native-check/MAC.turn (mkdir
# fails when it exists) holding one file, holder, of exactly three lines:
# "session <name>", "pid <n>", "started <YYYY-MM-DD HH:MM:SS>". A turn whose
# pid is not alive is cleared, saying so; a folder without a readable holder
# is taken and reported, never cleared. The turn is waited for at most an
# hour and released when the Mac half ends and on every exit path. Under 30%
# battery and not on mains the check refuses; on battery, or with the owner at
# the Mac (input idle under 10 minutes), it builds with at most 4 jobs, and on
# mains with him away with at most 8.
MAC_TURN="$HOME/.cache/mcbench-native-check/MAC.turn"
MAC_TURN_WAIT=3600
mac_turn_mine=0
# Waiters queue first come, first served, every Mac run of every session:
# each writes a ticket into MAC.queue named <seconds since 1970>-<session>-<pid>
# and holding the same three lines as the holder, and takes the turn only
# when its ticket is the oldest of a live pid (the pid is what follows the
# last "-"); a dead waiter's ticket is removed by whoever sees it, saying so;
# the ticket goes when the turn is taken and on every exit path.
MAC_QUEUE="$HOME/.cache/mcbench-native-check/MAC.queue"
mac_ticket=""
mac_first_in_queue() {
  local t pid
  for t in $(ls "$MAC_QUEUE" | LC_ALL=C sort -t- -k1,1n); do
    pid="${t##*-}"
    if kill -0 "$pid" 2>/dev/null; then [ "$t" = "$mac_ticket" ]; return; fi
    echo "check: removing the Mac queue's ticket $t, whose pid $pid is not alive"
    /bin/rm -f "$MAC_QUEUE/$t"
  done
  return 1
}
mac_turn_release() {
  if [ -n "$mac_ticket" ]; then /bin/rm -f "$MAC_QUEUE/$mac_ticket"; mac_ticket=""; fi
  if [ "$mac_turn_mine" = 1 ]; then
    /bin/rm -f "$MAC_TURN/holder"
    rmdir "$MAC_TURN"
    mac_turn_mine=0
  fi
}
if [ "$(uname -s)" = Darwin ] && [ "$WINDOWS_ONLY" = 0 ]; then
  batt="$(pmset -g batt)"
  # No pipe that stops reading early (head, grep -q, awk exit): with
  # pipefail its writer's SIGPIPE would end the script.
  [[ $batt =~ ([0-9]+)% ]] || { echo "native/check.sh: pmset -g batt gave no battery percentage: $batt" >&2; exit 2; }
  percent="${BASH_REMATCH[1]}"
  case "$batt" in *"'AC Power'"*) mains=1 ;; *) mains=0 ;; esac
  if [ "$mains" = 0 ] && [ "$percent" -lt 30 ]; then
    echo "native/check.sh: the Mac is on battery at $percent%: no builds on it under 30% until it is on mains (native/README.md)" >&2
    exit 2
  fi
  idle="$(ioreg -c IOHIDSystem | awk '/HIDIdleTime/ && !done { print int($NF / 1000000000); done = 1 }')"
  [ -n "$idle" ] || { echo "native/check.sh: ioreg gave no HIDIdleTime" >&2; exit 2; }
  if [ "$mains" = 0 ] || [ "$idle" -lt 600 ]; then mac_jobs=4; else mac_jobs=8; fi
  if [ "$LOCAL_JOBS" -gt "$mac_jobs" ]; then LOCAL_JOBS="$mac_jobs"; fi
  echo "check: the Mac is $([ "$mains" = 1 ] && echo "on mains" || echo "on battery at $percent%"), input idle ${idle} s: at most $mac_jobs jobs here, $LOCAL_JOBS used"
  mkdir -p "$(dirname "$MAC_TURN")"
  trap mac_turn_release EXIT
  trap 'exit 130' INT TERM HUP
  deadline=$((SECONDS + MAC_TURN_WAIT))
  seen=""
  mkdir -p "$MAC_QUEUE"
  mac_ticket="$(date +%s)-check.sh-on-$BRANCH-$$"
  printf 'session check.sh on %s\npid %s\nstarted %s\n' "$BRANCH" "$$" "$(date '+%Y-%m-%d %H:%M:%S')" > "$MAC_QUEUE/$mac_ticket"
  until mac_first_in_queue && mkdir "$MAC_TURN" 2>/dev/null; do
    holder="$(tr '\n' ';' < "$MAC_TURN/holder" 2>/dev/null || true)"
    owner="$(sed -n '2s/^pid \([0-9][0-9]*\)$/\1/p' "$MAC_TURN/holder" 2>/dev/null || true)"
    if [ -n "$owner" ] && ! kill -0 "$owner" 2>/dev/null; then
      echo "check: clearing the Mac's turn, whose pid $owner is not alive ($holder)"
      /bin/rm -f "$MAC_TURN/holder"
      rmdir "$MAC_TURN" 2>/dev/null || true
      continue
    fi
    if [ -d "$MAC_TURN" ]; then state="held: ${holder:-no readable holder, treated as taken}"; else state="free, with earlier tickets in $MAC_QUEUE: $(ls "$MAC_QUEUE" | LC_ALL=C sort -t- -k1,1n | tr '\n' ' ')"; fi
    if [ "$state" != "$seen" ]; then
      echo "check: waiting for the Mac's turn (my ticket $mac_ticket); it is $state"
      seen="$state"
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "check: FAILED  the Mac's turn was not free within $MAC_TURN_WAIT s; $MAC_TURN says: ${holder:-no readable holder}" >&2
      exit 1
    fi
    sleep 20
  done
  mac_turn_mine=1
  /bin/rm -f "$MAC_QUEUE/$mac_ticket"
  mac_ticket=""
  printf 'session check.sh on %s\npid %s\nstarted %s\n' "$BRANCH" "$$" "$(date '+%Y-%m-%d %H:%M:%S')" > "$MAC_TURN/holder"
  echo "check: took the Mac's turn"
  # The Mac's shared target and its build child's and drawing crate's beside
  # it: when together over MAC_TARGET_MAX_GB at the start of a turn they are
  # deleted, saying so, and this run builds them again. Measured only under
  # the turn, so no other run is using them.
  MAC_TARGET_MAX_GB=24
  if [ "$CARGO_TARGET_DIR" = "$CHECK_HOME/target-mac" ]; then
    kb=0
    for dir in "$CARGO_TARGET_DIR" "$CARGO_TARGET_DIR-build-child" "$CARGO_TARGET_DIR-draw"; do
      [ -d "$dir" ] && kb=$((kb + $(du -sk "$dir" | cut -f1)))
    done
    if [ "$kb" -gt $((MAC_TARGET_MAX_GB * 1024 * 1024)) ]; then
      echo "check: the Mac's shared target holds $((kb / 1024 / 1024)) GB, over $MAC_TARGET_MAX_GB: deleted, this run builds it again"
      /bin/rm -rf "$CARGO_TARGET_DIR" "$CARGO_TARGET_DIR-build-child" "$CARGO_TARGET_DIR-draw"
    fi
  fi
fi
export CARGO_BUILD_JOBS="$LOCAL_JOBS"
# A check needs neither incremental compilation nor full debug information,
# and with cargo's defaults one cold target folder takes about 54 GB on adipc
# (16 GB without); line tables keep file and line in a failing test's backtrace.
export CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=line-tables-only
export RUST_TEST_THREADS="$LOCAL_JOBS"

# The Windows VM's host, and how long --windows waits for its LOCK.
WINVM_HOST=adipc
WINVM_LOCK_WAIT=3600

# The build child's workspace (native/build-child: its own [workspace] and
# Cargo.lock, the toolchain of native/rust-toolchain.toml), checked before the
# main workspace's tests, which run its program, into its own target folder
# beside the main one (<target>-build-child). One function for both paths:
# `build_child_checks local` (run_checks, on Linux and the Mac) runs cargo fmt
# --check, cargo clippy --all-targets --locked -- -D warnings and cargo test
# --locked here and exports MCBENCH_BUILD_CHILD, the built program the main
# workspace's tests start (mcbench_build::child_program); `build_child_checks
# windows` (the --windows-vm path) runs the same clippy and test in the guest
# (check.ps1's child-clippy and child-test, with the cache disk's
# target-build-child), whose main test step names the built
# mcbench-build-child.exe the same way.
build_child_checks() {
  if [ "$1" = windows ]; then
    wstep "windows: build-child: cargo clippy --all-targets -- -D warnings" child-clippy
    wstep "windows: build-child: cargo test" child-test
    return
  fi
  local child=(env CARGO_TARGET_DIR="$CARGO_TARGET_DIR-build-child" "$RUSTUP" run "$CHANNEL" cargo)
  local manifest=(--manifest-path "$HERE/build-child/Cargo.toml")
  step "build-child: cargo fmt --check" "${child[@]}" fmt --check "${manifest[@]}"
  step "build-child: cargo clippy --all-targets -- -D warnings" "${child[@]}" clippy "${manifest[@]}" --all-targets --locked -- -D warnings
  step "build-child: cargo test" "${child[@]}" test "${manifest[@]}" --locked
  export MCBENCH_BUILD_CHILD="$CARGO_TARGET_DIR-build-child/debug/mcbench-build-child"
}

# The drawing crate's own workspace (native/draw: its own [workspace] and
# Cargo.lock, excluded from native/'s members): cargo fmt --check, cargo clippy
# --all-targets --locked -- -D warnings and cargo test --locked, into its own
# target folder (<target>-draw), on Linux and the Mac. Its tests that need
# Minecraft's data or a real build say so and are skipped without them.
draw_checks() {
  local draw=(env CARGO_TARGET_DIR="$CARGO_TARGET_DIR-draw" "$RUSTUP" run "$CHANNEL" cargo)
  local manifest=(--manifest-path "$HERE/draw/Cargo.toml")
  step "draw: cargo fmt --check" "${draw[@]}" fmt --check "${manifest[@]}"
  step "draw: cargo clippy --all-targets -- -D warnings" "${draw[@]}" clippy "${manifest[@]}" --all-targets --locked -- -D warnings
  step "draw: cargo test" "${draw[@]}" test "${manifest[@]}" --locked
}

# rustup-init 1.29.3, as server/tools.js pins it.
rustup_pin() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo "aarch64-apple-darwin 63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin bc107f1fb0228de7fa32413c89c1b305cea9cd256e94b01150cd054128145429" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu 9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282" ;;
    Linux-x86_64) echo "x86_64-unknown-linux-gnu b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55" ;;
    *) echo "native/check.sh: no rustup-init pin for $(uname -s)-$(uname -m)" >&2; exit 2 ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

RUSTUP="$CARGO_HOME/bin/rustup"
toolchain() {
  if [ ! -x "$RUSTUP" ]; then
    read -r triple sha <<<"$(rustup_pin)"
    mkdir -p "$CARGO_HOME" "$RUSTUP_HOME"
    local init="$CARGO_HOME/rustup-init-1.29.3"
    curl -fsSL -o "$init.part" "https://static.rust-lang.org/rustup/archive/1.29.3/$triple/rustup-init"
    [ "$(sha256_of "$init.part")" = "$sha" ] || { echo "native/check.sh: rustup-init's sha256 is not the pinned $sha" >&2; exit 2; }
    mv "$init.part" "$init"
    chmod +x "$init"
    RUSTUP_INIT_SKIP_PATH_CHECK=yes "$init" -y --no-modify-path --default-toolchain none --profile minimal >/dev/null
  fi
  if ! "$RUSTUP" run "$CHANNEL" cargo clippy --version >/dev/null 2>&1 \
    || ! "$RUSTUP" run "$CHANNEL" cargo fmt --version >/dev/null 2>&1; then
    "$RUSTUP" toolchain install "$CHANNEL" --profile minimal --component clippy --component rustfmt
  fi
}

# --windows-vm: run on adipc from the copied tree ($ROOT); see "The Windows
# check" above. Exits with the result.
# The cache disk's bound (see "The Windows check" above): before a run, a
# cache disk $CACHE that takes more than MCBENCH_WINDOWS_CACHE_MAX_GB
# (default 20) on the host is deleted, and the run says so.
cache_bound() {
  local max="${MCBENCH_WINDOWS_CACHE_MAX_GB:-20}" bytes
  [ -f "$CACHE" ] || return 0
  bytes="$(qemu-img info --output=json "$CACHE" | python3 -I -c 'import json, sys; print(json.load(sys.stdin)["actual-size"])')"
  if awk -v b="$bytes" -v m="$max" 'BEGIN { exit !(b > m * 2^30) }'; then
    rm -f "$CACHE"
    echo "check: the cache disk held $(awk -v b="$bytes" 'BEGIN { printf "%.1f", b / 2^30 }') GB, over $max: deleted, this run is cold"
  fi
}

# The pinned Linux x86_64 Codex package (mcbench-providers' install.rs
# pins it; its tests install it through the pin, which checks the sha256
# again), fetched once into $CHECK_HOME/codex-package.
codex_package() {
  local sha=3b8ed091149cc63a59046b9f094bcfa21cdb4dda15ede441ce369bcee4a71ba0
  CODEX_PACKAGE="$CHECK_HOME/codex-package/codex-package-x86_64-unknown-linux-musl-0.169.0.tar.gz"
  [ -f "$CODEX_PACKAGE" ] && return 0
  mkdir -p "$CHECK_HOME/codex-package"
  curl -fsSL -o "$CODEX_PACKAGE.part" https://github.com/openai/codex/releases/download/rust-v0.169.0/codex-package-x86_64-unknown-linux-musl.tar.gz || return 1
  [ "$(sha256_of "$CODEX_PACKAGE.part")" = "$sha" ] || { echo "native/check.sh: the Codex package's sha256 is not the pinned $sha (crates/providers/src/harness/install.rs pins it)" >&2; return 1; }
  mv "$CODEX_PACKAGE.part" "$CODEX_PACKAGE"
}

# --wsl-assets: the Linux files of the guest's WSL tests (see "The WSL tests"
# above), made on adipc from check-<branch>/tree (brought by content, as the
# adipc half's tree) in a scope of the caller's slice, into
# check-<branch>/wsl-assets/run: the guest base, the root filesystem installed
# once in check-windows/assets, the build child and the escape probe. A Linux
# program is refused when it needs a glibc symbol version that the libc.so.6
# of the guest base just built does not define. Exits with the result.
if [ "$WSL_ASSETS" = 1 ]; then
  installed="$HOME/mcbench-native-scratch/check-windows/assets"
  assets="$HOME/mcbench-native-scratch/check-$BRANCH/wsl-assets"
  out="$assets/run"
  rootfs="$installed/mcbench-sandbox-rootfs-amd64.tar.zst"
  if [ ! -f "$rootfs" ] || [ ! -f "$installed/mcbench-sandbox-rootfs-amd64.sha256" ]; then
    echo "check: FAILED  windows: the x86_64 root filesystem $rootfs and its .sha256 are not there (native/README.md says how it is made there, once)" >&2
    exit 1
  fi
  rm -rf "$out"
  mkdir -p "$out"
  toolchain
  "$RUSTUP" target add --toolchain "$CHANNEL" x86_64-unknown-linux-musl >/dev/null
  PATH="$CARGO_HOME/bin:$PATH" bash "$HERE/crates/sandbox/guest/build.sh" "$assets/guest" x86_64 >/dev/null
  cp "$assets/guest/mcbench-sandbox-guest-x86_64.tar.zst" "$assets/guest/mcbench-sandbox-guest-x86_64.sha256" \
    "$rootfs" "$installed/mcbench-sandbox-rootfs-amd64.sha256" "$out/"
  # The pinned Linux Codex for the guest's Codex tests (windows_codex_wsl*).
  codex_package || exit 1
  cp "$CODEX_PACKAGE" "$out/codex-package-x86_64-unknown-linux-musl.tar.gz"
  env CARGO_TARGET_DIR="$CARGO_TARGET_DIR-build-child" "$RUSTUP" run "$CHANNEL" cargo build --manifest-path "$HERE/build-child/Cargo.toml" --locked
  cp "$CARGO_TARGET_DIR-build-child/debug/mcbench-build-child" "$out/mcbench-build-child"
  probe="$("$RUSTUP" run "$CHANNEL" cargo test --manifest-path "$HERE/Cargo.toml" -p mcbench-build --test child --no-run --locked --message-format=json \
    | python3 -I -c 'import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "child":
        print(m["executable"])')"
  cp "$probe" "$out/child-probe"
  # The glibc symbol versions the guest base's own libc.so.6 defines.
  libc="$out/.guest-libc.so.6"
  if ! tar -xOf "$assets/guest/mcbench-sandbox-guest-x86_64/userland.tar" usr/lib/x86_64-linux-gnu/libc.so.6 >"$libc"; then
    echo "check: FAILED  windows: the guest base's userland.tar has no usr/lib/x86_64-linux-gnu/libc.so.6" >&2
    exit 1
  fi
  defined="$(objdump -p "$libc" | awk '/^Version definitions:/ { on = 1; next } on && NF == 0 { exit } on' | grep -o 'GLIBC_[0-9][0-9.]*' | sort -u)"
  rm -f "$libc"
  if [ -z "$defined" ]; then
    echo "check: FAILED  windows: no GLIBC version could be read from the guest base's libc.so.6" >&2
    exit 1
  fi
  echo "check: the guest base's glibc defines versions up to $(printf '%s\n' "$defined" | sed 's/^GLIBC_//' | sort -V | tail -n 1)"
  for file in "$out/mcbench-build-child" "$out/child-probe"; do
    missing="$(comm -23 <(objdump -T "$file" | grep -o 'GLIBC_[0-9][0-9.]*' | sort -u) <(printf '%s\n' "$defined"))"
    if [ -n "$missing" ]; then
      echo "check: FAILED  windows: $file needs $(echo $missing), which the guest base's glibc does not define" >&2
      exit 1
    fi
  done
  exit 0
fi

if [ "$WINDOWS_VM" = 1 ]; then
  VM_JOBS="$JOBS"
  [ "$VM_JOBS" -gt 4 ] && VM_JOBS=4
  # check.sh uses the check slot; everything else takes the hold slot.
  export WINVM_SLOT=check
  # The guest gets as many processors as the slot has host threads (4), not
  # env.sh's default of 12 on those 4: with WSL installed Windows is the root
  # partition of its own Hyper-V under KVM, and with 12 on 4 the guest took 2
  # to 27 minutes to boot and five check runs died in ssh; with 4 it boots in
  # about 33 s (the main session's ruling, 2026-11-18; env.sh's default is
  # foundations' to change). A run's timings are not compared with runs at 12.
  export WINVM_CPUS="${WINVM_CPUS:-4}"
  WINVM="$HOME/mcbench-native-scratch/winvm"
  LOCKSH="$WINVM/scripts/lock.sh"
  OVERLAY="$HOME/mcbench-native-scratch/check-windows/run"
  CACHE="$HOME/mcbench-native-scratch/check-windows/cache.qcow2"
  # The WSL tests' Linux files, made by --wsl-assets before this run.
  WASSETS="$HOME/mcbench-native-scratch/check-$BRANCH/wsl-assets/run"
  WTREE="$ROOT"
  if [ ! -f "$WASSETS/mcbench-build-child" ]; then
    echo "check: FAILED  windows: $WASSETS has no WSL files; native/check.sh --windows makes them (--wsl-assets) before it starts this run" >&2
    exit 1
  fi
  # The shared scripts that take the LOCK and refuse without it are the
  # repository's (native/tools/winvm), installed in winvm/scripts. lock.sh is
  # compared before the take; the others, and env.sh's sourcing, only once the
  # LOCK is held, so a run that waited uses what is installed when its turn
  # comes, and a script is installed (with the LOCK held) while no other run
  # has it loaded.
  same_as_repo() {
    local script
    for script in "$@"; do
      if ! cmp -s "$HERE/tools/winvm/$script" "$WINVM/scripts/$script"; then
        echo "check: FAILED  windows: $WINVM/scripts/$script is missing or differs from this tree's native/tools/winvm/$script; every use of the Windows VM goes through the repository's scripts, so install it there (cp native/tools/winvm/$script ~/mcbench-native-scratch/winvm/scripts/$script) while you hold the VM's LOCK" >&2
        exit 1
      fi
    done
  }
  same_as_repo lock.sh
  # ssh takes the first value of an option: 60 s for the banner, not env.sh's
  # 5 s, because a guest just booted on few host cores answers slowly.
  guest() { ssh -o ConnectTimeout=60 $(ssh_opts) bench@127.0.0.1 "$@"; }
  to_guest() { scp -q -o ConnectTimeout=60 $(ssh_opts | sed 's/-p /-P /') "$1" "bench@127.0.0.1:$2"; }
  # A guest command runs in the background and is waited for, so a TERM or
  # INT reaches the trap at once and not only when the command ends.
  bg_wait() { "$@" & wait $!; }

  LOCK_LINE="native/check.sh --windows, tree $WTREE, overlay $OVERLAY, pid $$, started $(date '+%Y-%m-%d %H:%M:%S')"
  locked=0
  made=0
  started=0
  # A closed ssh connection must not stop the release half way: writes fail
  # instead of killing it, and no failed step skips the ones after it.
  release() {
    set +e
    trap '' PIPE
    if [ "$started" = 1 ]; then bash "$WINVM/scripts/stop.sh"; fi
    rm -f "$WTREE.tar" "$WTREE.ps1" "$WTREE.assets.tar"
    # The overlay is removed only by the run that made it (a run that found
    # the VM running leaves that VM's disk alone).
    if [ "$made" = 1 ]; then rm -rf "$OVERLAY"; fi
    if [ "$locked" = 1 ]; then bash "$LOCKSH" release "$LOCK_LINE"; fi
  }
  trap release EXIT
  trap 'exit 130' INT TERM HUP PIPE
  if ! bash "$LOCKSH" take "$LOCK_LINE" "$WINVM_LOCK_WAIT"; then
    echo "check: FAILED  windows: the Windows VM's check slot's LOCK was not free within ${WINVM_LOCK_WAIT} s" >&2
    exit 1
  fi
  locked=1
  same_as_repo env.sh start.sh stop.sh reset.sh snapshot.sh
  # env.sh: vm_start, vm_running, wait_ssh, ssh_opts (it sets its own HERE).
  source "$WINVM/scripts/env.sh"
  # env.sh's functions and the winvm scripts refuse unless this is the LOCK's line.
  export WINVM_LOCK_LINE="$LOCK_LINE"
  if vm_running; then echo "check: FAILED  the Windows VM runs (qemu pid $(cat "$SLOTDIR/qemu.pid")) although its LOCK was free" >&2; exit 1; fi

  # An overlay a killed run left goes; this run's is made new.
  made=1
  rm -rf "$OVERLAY"
  mkdir -p "$OVERLAY"
  # win11-tools with WSL installed (see "The WSL tests" above).
  qemu-img create -q -f qcow2 -b "$BASES/win11-tools-wsl.qcow2" -F qcow2 "$OVERLAY/disk.qcow2"
  cp "$BASES/win11-tools-wsl.vars.fd" "$OVERLAY/vars.fd"
  cp -r "$BASES/win11-tools-wsl.tpm" "$OVERLAY/tpm"
  # The base's files are not writable; the run's copies are its own.
  chmod -R u+w "$OVERLAY/vars.fd" "$OVERLAY/tpm"
  cache_bound
  if [ ! -f "$CACHE" ]; then
    qemu-img create -q -f qcow2 "$CACHE" 40G
    echo "check: made the cache disk $CACHE (empty: this run downloads and builds everything)"
  fi
  started=1
  vm_start "$OVERLAY/disk.qcow2" "$OVERLAY/vars.fd" "$OVERLAY/tpm" \
    -drive file="$CACHE",if=none,id=cache,format=qcow2,cache=writeback,discard=unmap,detect-zeroes=unmap \
    -device virtio-blk-pci,drive=cache
  # With WSL installed, Windows boots as the root partition of its own
  # hypervisor under KVM: 2 to 27 minutes to ssh in the hold slot (measured
  # 2026-11-18 at load 15 to 25).
  wait_ssh 2400

  tar -cf "$WTREE.tar" -C "$WTREE" .
  tar -cf "$WTREE.assets.tar" -C "$WASSETS" .
  cat >"$WTREE.ps1" <<'PS'
# C:\mcb\check.ps1 STEP JOBS: one step of native/check.sh --windows in the guest.
# The cache disk is the volume labelled mcbcache: cargo's registry and the
# target folder.
param([Parameter(Mandatory)][string]$Step, [Parameter(Mandatory)][string]$Jobs)
$ErrorActionPreference = 'Stop'
if ($Step -eq 'cache') {
  # A fresh Windows may see the disk offline; an empty one is formatted.
  Get-Disk | Where-Object IsOffline | Set-Disk -IsOffline $false
  Get-Disk | Where-Object IsReadOnly | Set-Disk -IsReadOnly $false
  if (-not (Get-Volume -FileSystemLabel mcbcache -ErrorAction SilentlyContinue)) {
    $raw = @(Get-Disk | Where-Object PartitionStyle -eq 'RAW')
    if ($raw.Count -ne 1) { Write-Output "check.ps1: no volume mcbcache and $($raw.Count) empty disks"; exit 2 }
    $raw[0] | Initialize-Disk -PartitionStyle GPT -PassThru | New-Partition -UseMaximumSize -AssignDriveLetter |
      Format-Volume -FileSystem NTFS -NewFileSystemLabel mcbcache -Confirm:$false | Out-Null
    Write-Output "check.ps1: formatted the cache disk"
  }
  exit 0
}
$cache = (Get-Volume -FileSystemLabel mcbcache).DriveLetter
if (-not $cache) { Write-Output 'check.ps1: the cache volume mcbcache has no drive letter'; exit 2 }
$ErrorActionPreference = 'Continue'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_HOME = "${cache}:\cargo"
$env:CARGO_TARGET_DIR = "${cache}:\target"
$env:CARGO_BUILD_JOBS = $Jobs
$env:RUST_TEST_THREADS = $Jobs
# A check needs neither debug information (MSVC's PDB files) nor
# incremental compilation's cache, and both fill the cache disk.
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
# The WSL tests' files (C:\mcb\assets, from adipc) and the test distro, whose
# disk is in a folder of this run's Windows.
function Set-Wsl {
  $a = 'C:\mcb\assets'
  $env:MCBENCH_BUILD_CHILD = "$a\mcbench-build-child"
  $env:MCBENCH_BUILD_PROBE = "$a\child-probe"
  $env:MCBENCH_SANDBOX_ROOTFS = "$a\mcbench-sandbox-rootfs-amd64.tar.zst"
  $env:MCBENCH_SANDBOX_ROOTFS_SHA256 = (Get-Content "$a\mcbench-sandbox-rootfs-amd64.sha256").Trim()
  $env:MCBENCH_SANDBOX_GUEST = "$a\mcbench-sandbox-guest-x86_64.tar.zst"
  $env:MCBENCH_SANDBOX_GUEST_SHA256 = (Get-Content "$a\mcbench-sandbox-guest-x86_64.sha256").Trim()
  $env:MCBENCH_SANDBOX_SCRATCH = 'C:\mcb\sandbox'
  $env:MCBENCH_WSL_DISTRO = 'mcbench-check'
  $env:MCBENCH_CODEX_PACKAGE = "$a\codex-package-x86_64-unknown-linux-musl.tar.gz"
  New-Item -ItemType Directory -Force $env:MCBENCH_SANDBOX_SCRATCH | Out-Null
}
switch ($Step) {
  'assets' {
    New-Item -ItemType Directory -Force C:\mcb\assets | Out-Null
    tar -xf C:\mcb\assets.tar -C C:\mcb\assets
    if ($LASTEXITCODE -ne 0) { Write-Output "check.ps1: tar -xf of the WSL files failed ($LASTEXITCODE)"; exit 2 }
    Remove-Item C:\mcb\assets.tar
  }
  'sizes' {
    $parts = foreach ($d in 'cargo', 'target', 'target-build-child', 'tree') {
      $bytes = (Get-ChildItem -LiteralPath "${cache}:\$d" -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object -Sum Length).Sum
      '{0} {1:N2} GiB' -f $d, ($bytes / 1GB)
    }
    Write-Output ("check.ps1: the cache disk holds " + ($parts -join ', '))
  }
  'unpack' {
    # The kept tree on the cache disk is brought to this run's tree by
    # content: a file whose bytes differ, or a new one, is written and its
    # time becomes now; an identical file keeps its time; a file not in this
    # run's tree goes. Cargo's time-based fingerprints then see exactly the
    # changed files as newer than the kept target's last build.
    $ErrorActionPreference = 'Stop'
    $t0 = Get-Date
    $stage = "${cache}:\stage"; $tree = "${cache}:\tree"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory $stage | Out-Null
    New-Item -ItemType Directory -Force $tree | Out-Null
    tar -xf C:\mcb\tree.tar -C $stage
    if ($LASTEXITCODE -ne 0) { Write-Output "check.ps1: tar -xf failed ($LASTEXITCODE)"; exit 2 }
    # The run's files are the tar's own list (its directories end in /).
    $files = @(tar -tf C:\mcb\tree.tar | Where-Object { -not $_.EndsWith('/') } | ForEach-Object { ($_ -replace '^\./', '') -replace '/', '\' })
    if ($LASTEXITCODE -ne 0) { Write-Output "check.ps1: tar -tf failed ($LASTEXITCODE)"; exit 2 }
    $now = Get-Date
    $keep = @{}
    $written = 0; $same = 0; $removed = 0
    foreach ($rel in $files) {
      $keep[$rel] = $true
      $src = Get-Item -LiteralPath (Join-Path $stage $rel) -Force
      $dst = Join-Path $tree $rel
      $old = Get-Item -LiteralPath $dst -Force -ErrorAction SilentlyContinue
      if ($old -and $old.Length -eq $src.Length -and (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $src.FullName -Algorithm SHA256).Hash) { $same++; continue }
      New-Item -ItemType Directory -Force (Split-Path -Parent $dst) | Out-Null
      Copy-Item -LiteralPath $src.FullName -Destination $dst -Force
      (Get-Item -LiteralPath $dst -Force).LastWriteTime = $now
      $written++
    }
    foreach ($f in Get-ChildItem -LiteralPath $tree -Recurse -File -Force) {
      if (-not $keep.ContainsKey($f.FullName.Substring($tree.Length + 1))) { Remove-Item -LiteralPath $f.FullName -Force; $removed++ }
    }
    Get-ChildItem -LiteralPath $tree -Recurse -Directory -Force | Sort-Object { $_.FullName.Length } -Descending |
      Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } | Remove-Item -Force
    Remove-Item -Recurse -Force $stage
    Write-Output ("check.ps1: tree synced by content: {0} written, {1} unchanged, {2} removed, in {3:N1} s" -f $written, $same, $removed, ((Get-Date) - $t0).TotalSeconds)
  }
  'child-clippy' { Set-Location "${cache}:\tree\native\build-child"; $env:CARGO_TARGET_DIR = "${cache}:\target-build-child"; cmd /c 'cargo clippy --all-targets --locked -- -D warnings 2>&1' }
  'child-test' { Set-Location "${cache}:\tree\native\build-child"; $env:CARGO_TARGET_DIR = "${cache}:\target-build-child"; cmd /c 'cargo test --locked 2>&1' }
  'clippy' { Set-Location "${cache}:\tree\native"; cmd /c 'cargo clippy --workspace --all-targets --locked -- -D warnings 2>&1' }
  'test' { Set-Location "${cache}:\tree\native"; Set-Wsl; cmd /c 'cargo test --workspace --locked 2>&1' }
  'wsl' { Set-Location "${cache}:\tree\native"; Set-Wsl; $env:RUST_TEST_THREADS = '1'; cmd /c 'cargo test -p mcbench-sandbox --locked --test windows_wsl --test linux_box -- --ignored --nocapture 2>&1' }
  'codex' { Set-Location "${cache}:\tree\native"; Set-Wsl; cmd /c 'cargo test -p mcbench-providers --locked --test cli -- windows_codex_wsl 2>&1' }
  default { Write-Output "check.ps1: unknown step $Step"; exit 2 }
}
exit $LASTEXITCODE
PS
  bg_wait guest 'New-Item -ItemType Directory -Force C:\mcb | Out-Null'
  bg_wait to_guest "$WTREE.tar" C:/mcb/tree.tar
  bg_wait to_guest "$WTREE.ps1" C:/mcb/check.ps1
  bg_wait to_guest "$WTREE.assets.tar" C:/mcb/assets.tar
  # The cache step first: it formats a new cache disk, and every other step
  # (assets among them) reads the volume's drive letter. With assets first, a
  # run on a fresh cache disk died there ("No MSFT_Volume ... mcbcache").
  bg_wait guest "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\check.ps1 cache $VM_JOBS"
  bg_wait guest "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\check.ps1 assets $VM_JOBS"
  bg_wait guest "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\check.ps1 unpack $VM_JOBS"

  echo "check: Windows 11 VM on $(uname -n) (slot $SLOT, $CPUS vCPU on host threads $CORES, $MEM), $VM_JOBS jobs, x86_64-pc-windows-msvc, cache disk $CACHE ($(du -h "$CACHE" | cut -f1) used)"
  failed=0
  wstep() {
    local name="$1" step="$2"
    if bg_wait guest "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\check.ps1 $step $VM_JOBS"; then echo "check: ok      $name"; else echo "check: FAILED  $name"; failed=1; fi
  }
  build_child_checks windows
  wstep "windows: cargo clippy --workspace --all-targets -- -D warnings" clippy
  wstep "windows: WSL: cargo test -p mcbench-sandbox --test windows_wsl --test linux_box -- --ignored" wsl
  wstep "windows: WSL: cargo test -p mcbench-providers --test cli -- windows_codex_wsl (Codex in its box in the distro)" codex
  wstep "windows: cargo test --workspace" test
  bg_wait guest "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\check.ps1 sizes $VM_JOBS"
  exit "$failed"
fi

CHECK_TMP="$CHECK_HOME/t/$(printf %s "$BRANCH" | cksum | cut -d" " -f1)"
for dir in "$RUSTUP_HOME" "$CARGO_HOME" "$CARGO_TARGET_DIR" "$CHECK_TMP"; do
  case "$dir" in
    "$HOME/.mcbench-lite/tools"|"$HOME/.mcbench-lite/tools/"*) echo "native/check.sh: $dir is inside ~/.mcbench-lite/tools, the app's live tools folder" >&2; exit 2 ;;
    "$ROOT"|"$ROOT/"*) echo "native/check.sh: $dir is inside the repository" >&2; exit 2 ;;
  esac
done
# The tests' temporary files: a folder of this branch's, emptied now and kept
# after the run, so what a failed test left can be read.
rm -rf "$CHECK_TMP"
mkdir -p "$CHECK_TMP"
export TMPDIR="$CHECK_TMP"

# --macos-vm: the bash sandbox's tests against the macOS VM (see "The macOS
# VM tests" above), in a subshell so its variables stay its own.
macos_vm_tests() (
  export CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER="$HERE/crates/sandbox/macos/run-signed.sh"
  export MCBENCH_SANDBOX_NETWORK=1
  export MCBENCH_SANDBOX_SCRATCH="${MCBENCH_SANDBOX_SCRATCH:-$CHECK_HOME/sandbox-vm}"
  mkdir -p "$MCBENCH_SANDBOX_SCRATCH"
  cd "$HERE"
  "$RUSTUP" run "$CHANNEL" cargo test -p mcbench-sandbox --locked --test macos_vm --test linux_box --test network --test native_diff -- --ignored --test-threads 1
)

# The checks on this machine. Prints one line per check and returns non-zero
# when one failed; every check runs even after one fails.
# What a release build compiles beyond the steps above (native/packaging/build/
# linux.sh, macos.sh, windows.ps1): every target and feature set a release
# builds is built by some half. On Linux: the sandbox agent the release ships
# static for musl, and mcbench-lite with shipped-data, which embeds the data
# bundle native/minecraft-data.lock names; it is read from
# $CHECK_HOME/data-bundle/<the bundle's sha256>.tar.zst, put there by whoever
# changes the lock (native/README.md). On both: the updater's feed feature.
release_musl() {
  "$RUSTUP" target add --toolchain "$CHANNEL" "$1" >/dev/null
  cargo check -p mcbench-sandbox --lib --bins --locked --target "$1"
}
release_shipped_data() {
  local sha bundle
  sha="$(awk -F'\t' '$1 == "bundle" { print $2 }' "$HERE/minecraft-data.lock")"
  bundle="$CHECK_HOME/data-bundle/$sha.tar.zst"
  if [ ! -f "$bundle" ]; then
    echo "check: the data bundle $bundle (native/minecraft-data.lock's bundle line) is not there: put the bundle native/tools/extract/bundle.sh wrote for this lock there, named by its sha256" >&2
    return 1
  fi
  MCBENCH_DATA_BUNDLE="$bundle" cargo check -p mcbench-lite --features shipped-data --locked
}

# Linux: what the Codex box tests need (crates/providers/tests/unix/box_tests.rs,
# crates/sandbox/tests/codex_box.rs, install.rs's execute_only tests): the
# static sandbox agent built for x86_64-unknown-linux-musl, the x86_64 root
# filesystem installed once in check-windows/assets (as --wsl-assets takes
# it) and the pinned Codex package, as the variables those tests read.
codex_box_files() {
  local installed="$HOME/mcbench-native-scratch/check-windows/assets"
  if [ ! -f "$installed/mcbench-sandbox-rootfs-amd64.tar.zst" ] || [ ! -f "$installed/mcbench-sandbox-rootfs-amd64.sha256" ]; then
    echo "check: the x86_64 root filesystem $installed/mcbench-sandbox-rootfs-amd64.tar.zst and its .sha256 are not there (native/README.md says how it is made there, once)" >&2
    return 1
  fi
  "$RUSTUP" target add --toolchain "$CHANNEL" x86_64-unknown-linux-musl >/dev/null
  cargo build -p mcbench-sandbox --bin mcbench-sandbox-agent --target x86_64-unknown-linux-musl --locked || return 1
  codex_package || return 1
  export MCBENCH_SANDBOX_ROOTFS="$installed/mcbench-sandbox-rootfs-amd64.tar.zst"
  MCBENCH_SANDBOX_ROOTFS_SHA256="$(cut -d' ' -f1 "$installed/mcbench-sandbox-rootfs-amd64.sha256")"
  export MCBENCH_SANDBOX_ROOTFS_SHA256
  export MCBENCH_SANDBOX_AGENT_STATIC="$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/debug/mcbench-sandbox-agent"
  export MCBENCH_CODEX_PACKAGE="$CODEX_PACKAGE"
}

run_checks() {
  local failed=0
  cargo() { (cd "$HERE" && "$RUSTUP" run "$CHANNEL" cargo "$@"); }
  step() {
    local name="$1"; shift
    if "$@"; then echo "check: ok      $name"; else echo "check: FAILED  $name"; failed=1; fi
  }
  # Without --all: cargo fmt --all also formats local path dependencies,
  # which would rewrite core/.
  step "cargo fmt --check" cargo fmt --check
  build_child_checks local
  draw_checks
  step "cargo clippy --workspace --all-targets -- -D warnings" cargo clippy --workspace --all-targets --locked -- -D warnings
  if [ "$(uname -s)" = Linux ]; then
    step "Linux: Codex's box's files (the static sandbox agent, the root filesystem, the pinned Codex)" codex_box_files
  fi
  step "cargo test --workspace" cargo test --workspace --locked
  if [ "$(uname -s)" = Linux ]; then
    step "Linux: cargo test -p mcbench-sandbox --test codex_box -- --ignored" cargo test -p mcbench-sandbox --test codex_box --locked -- --ignored --test-threads 1
    step "Linux: cargo test -p mcbench-sandbox --test linux_box -- --ignored" cargo test -p mcbench-sandbox --test linux_box --locked -- --ignored
    step "Linux: cargo test -p mcbench-providers --lib execute_only -- --ignored (the pinned Codex installed execute-only)" cargo test -p mcbench-providers --lib --locked execute_only -- --ignored
  fi
  # The crates' documentation is the description of their seams: it builds
  # without a warning (a broken link names a type that moved).
  step "cargo doc --workspace --no-deps (warnings are errors)" env RUSTDOCFLAGS="-D warnings" "$RUSTUP" run "$CHANNEL" cargo doc --manifest-path "$HERE/Cargo.toml" --workspace --no-deps --locked
  step "release: cargo test -p mcbench-updater --features feed" cargo test -p mcbench-updater --features feed --locked
  if [ "$(uname -s)" = Linux ]; then
    for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
      step "release: the sandbox agent for $target (cargo check)" release_musl "$target"
    done
    step "release: mcbench-lite --features shipped-data (cargo check)" release_shipped_data
  fi
  if [ "$NODE" = 1 ]; then
    step "npm test" bash -c "cd '$SRC_ROOT' && npm test -- --test-concurrency=$LOCAL_JOBS"
  fi
  if [ "$MACOS_VM" = 1 ]; then
    step "macOS VM: cargo test -p mcbench-sandbox --test macos_vm --test linux_box --test network --test native_diff -- --ignored" macos_vm_tests
  fi
  # core/ and renderer/ are hashed into the engine's and the mesher's
  # versions (server/engine.js sourcesHash, scripts/build-renderer.js): a
  # build from the workspace must leave every file there as it was.
  step "core/ and renderer/ unchanged by the build" bash -c "[ \"\$(tree_sum)\" = '$BEFORE' ]"
  return "$failed"
}

# Every file of core/ and renderer/ with its checksum.
tree_sum() { (cd "$ROOT" && find core renderer -type f | LC_ALL=C sort | xargs cksum); }
export -f tree_sum
export ROOT

# On the Mac, one tree builds into the shared target: the tree under check is
# brought by content into $CHECK_HOME/mac-tree (the same files and the same
# rsync as adipc's, under the Mac's turn), and the Mac half builds there. A
# file whose content changed always gets a new time and a file no longer in
# the tree goes, so cargo never takes another tree's build as fresh (see "Why
# the trees are synced by content" above), whichever session or branch ran
# before. npm test still runs in the checkout itself, which has node_modules.
SRC_HERE="$HERE"
SRC_ROOT="$ROOT"
if [ "$(uname -s)" = Darwin ] && [ "$WINDOWS_ONLY" = 0 ]; then
  git -C "$ROOT" rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "native/check.sh: the Mac half needs a git checkout: it syncs the files git lists into the Mac's tree, and $ROOT is not one" >&2; exit 2; }
  MAC_TREE="$CHECK_HOME/mac-tree"
  MAC_STAGE="$CHECK_HOME/mac-stage"
  /bin/rm -rf "$MAC_STAGE"
  mkdir -p "$MAC_STAGE" "$MAC_TREE"
  (cd "$ROOT" && git ls-files -co --exclude-standard -- native core renderer server README.md) \
    | (cd "$ROOT" && rsync -a --files-from=- ./ "$MAC_STAGE/")
  rsync -rlp --checksum --delete "$MAC_STAGE/" "$MAC_TREE/"
  /bin/rm -rf "$MAC_STAGE"
  HERE="$MAC_TREE/native"
  ROOT="$MAC_TREE"
  export ROOT
  echo "check: the tree under check is synced by content into $MAC_TREE, where the Mac half builds"
fi

# The Mac half's crash reports (agreed with stage three): the names, and only
# the names, in DiagnosticReports and CrashReporter whose stem is one of this
# work's programs (mcbench-*, a build child, a JavaScript child, rust-objcopy,
# a test binary <name>-<16 hex>), listed before the checks and after; a name
# that appeared fails the half. The stem is the name before -YYYY-MM-DD in
# DiagnosticReports and before the first _ in CrashReporter. Nothing is read
# or deleted.
CRASH_STEM='^(mcbench-.*|child-[0-9a-f]{16}|js-child-.*|rust-objcopy|.*-[0-9a-f]{16})$'
crash_reports() {
  local dir names name stem
  dir="$HOME/Library/Logs/DiagnosticReports"
  names="$(ls -A "$dir")"
  while IFS= read -r name; do
    stem="${name%%-[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*}"
    if [ "$stem" != "$name" ] && [[ $stem =~ $CRASH_STEM ]]; then echo "$dir/$name"; fi
  done <<< "$names"
  dir="$HOME/Library/Application Support/CrashReporter"
  names="$(ls -A "$dir")"
  while IFS= read -r name; do
    stem="${name%%_*}"
    if [[ $stem =~ $CRASH_STEM ]]; then echo "$dir/$name"; fi
  done <<< "$names"
}

local_result=0
# --windows-only: no local half (the Windows half needs nothing of this machine).
if [ "$WINDOWS_ONLY" = 0 ]; then
  toolchain
  BEFORE="$(tree_sum)"
  echo "check: $(uname -s) $(uname -m), $(uname -n), toolchain $CHANNEL, target $CARGO_TARGET_DIR, $CARGO_BUILD_JOBS jobs"
  if [ "$(uname -s)" = Darwin ]; then crash_reports | LC_ALL=C sort > "$CHECK_HOME/crash-reports-before"; fi
  run_checks || local_result=1
  if [ "$(uname -s)" = Darwin ]; then
    crash_reports | LC_ALL=C sort > "$CHECK_HOME/crash-reports-after"
    new_reports="$(LC_ALL=C comm -13 "$CHECK_HOME/crash-reports-before" "$CHECK_HOME/crash-reports-after")"
    if [ -z "$new_reports" ]; then
      echo "check: ok      no new crash reports"
    else
      echo "check: FAILED  no new crash reports: these appeared during the checks:"
      echo "$new_reports"
      local_result=1
    fi
  fi
fi
# The Mac half is done: the next session may take the Mac.
mac_turn_release
HERE="$SRC_HERE"
ROOT="$SRC_ROOT"
export ROOT

# adipc's kept tree check-<branch>/tree brought to this checkout by content.
# The run's files go to a staging folder; the kept tree is brought to it by
# content and without times (no -t): identical files keep their old times,
# changed and new ones get the current time, files no longer in the run's
# tree go. See "Why the trees are synced by content" above. `on` runs a
# command there; `dest` is how rsync names its home ("adipc:" or "$HOME/").
base="mcbench-native-scratch/check-$BRANCH"
sync_tree() {
  echo "check: copying the tree to ${dest}$base/tree"
  "${on[@]}" "rm -rf ~/$base/stage && mkdir -p ~/$base/stage ~/$base/tree"
  # server/: the build child embeds five of its files unchanged (native/crates/build);
  # README.md: mcbench-setup's pins test holds the sizes it states.
  (cd "$ROOT" && git ls-files -co --exclude-standard -- native core renderer server README.md) \
    | (cd "$ROOT" && rsync -a --files-from=- ./ "${dest}$base/stage/")
  "${on[@]}" "rsync -rlp --checksum --delete ~/$base/stage/ ~/$base/tree/ && rm -rf ~/$base/stage"
}
# The pool's limits are runtime properties, lost on a reboot: refused without them.
pool_ready() {
  local set_pool="systemctl --user set-property --runtime mcbench.slice CPUQuota=2600%"
  local set_builds="systemctl --user set-property --runtime mcbench-build.slice MemoryHigh=20G MemoryMax=24G"
  local pool memory
  pool="$("${on[@]}" "systemctl --user show mcbench.slice -p CPUQuotaPerSecUSec --value")"
  memory="$("${on[@]}" "systemctl --user show mcbench-build.slice -p MemoryMax --value")"
  if [ -z "$pool" ] || [ "$pool" = infinity ] || [ -z "$memory" ] || [ "$memory" = infinity ]; then
    echo "native/check.sh: on $WINVM_HOST mcbench.slice has no CPUQuota or mcbench-build.slice no MemoryMax (runtime properties, lost on a reboot): set them with $set_pool and $set_builds there before starting work (native/README.md)" >&2
    exit 2
  fi
  echo "check: $WINVM_HOST runs in $SLICE under mcbench-build.slice (the builds' memory at most $memory bytes) in mcbench.slice (CPU quota $pool per second)"
}
# The environment of every run on adipc: its toolchain and the branch's target
# in the scratch (the adipc half's, so the WSL files reuse its warm target).
adipc_env="MCBENCH_NATIVE_CHECK_BRANCH=$BRANCH MCBENCH_NATIVE_CHECK_HOME=\$HOME/mcbench-native-scratch/check-toolchain CARGO_TARGET_DIR=\$HOME/$base/target"
scope="systemd-run --user --scope --quiet --collect --slice=$SLICE"

remote_result=0
if [ -n "$REMOTE" ]; then
  on=(ssh "$REMOTE"); dest="$REMOTE:"
  sync_tree
  pool_ready
  "${on[@]}" "cd ~/$base/tree && $adipc_env $scope bash native/check.sh --jobs $JOBS" || remote_result=1
fi

windows_result=0
if [ "$WINDOWS" = 1 ]; then
  wtree="$base/windows-tree"
  # Started on adipc itself, the VM's host is this machine.
  if [ -f "$HOME/mcbench-native-scratch/winvm/scripts/env.sh" ]; then
    on=(bash -c); dest="$HOME/"
  else
    on=(ssh "$WINVM_HOST"); dest="$WINVM_HOST:"
  fi
  # The WSL tests' Linux files, built in the caller's slice from the kept tree
  # brought by content (see "The WSL tests" above).
  [ -n "$REMOTE" ] || sync_tree
  pool_ready
  if ! "${on[@]}" "cd ~/$base/tree && $adipc_env $scope bash native/check.sh --wsl-assets --jobs $JOBS"; then
    echo "check: FAILED  windows: the WSL tests' Linux files" >&2
    windows_result=1
  else
    echo "check: copying the tree to ${dest}$wtree for the Windows VM"
    # rsync -a keeps the files' times; the guest brings its kept tree to it by content.
    "${on[@]}" "rm -rf ~/$wtree && mkdir -p ~/$wtree"
    (cd "$ROOT" && git ls-files -co --exclude-standard -- native core renderer server README.md) \
      | (cd "$ROOT" && rsync -a --files-from=- ./ "${dest}$wtree/")
    "${on[@]}" "$adipc_env bash ~/$wtree/native/check.sh --windows-vm --jobs $JOBS" || windows_result=1
  fi
fi

[ "$WINDOWS_ONLY" = 0 ] && echo "check: $(uname -n) $([ "$local_result" = 0 ] && echo passed || echo FAILED)"
[ -n "$REMOTE" ] && echo "check: $REMOTE $([ "$remote_result" = 0 ] && echo passed || echo FAILED)"
[ "$WINDOWS" = 1 ] && echo "check: windows (the VM on $WINVM_HOST) $([ "$windows_result" = 0 ] && echo passed || echo FAILED)"
[ "$local_result" = 0 ] && [ "$remote_result" = 0 ] && [ "$windows_result" = 0 ]
