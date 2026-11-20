#!/bin/bash
# The macOS release build of one architecture, from a clean checkout:
# mcbench-lite.app and its archive, mcbench-lite-macos-<arch>.tar.gz.
#
#   native/packaging/build/macos.sh <repository> <target> <out>
#
# <target> is aarch64-apple-darwin or x86_64-apple-darwin. Into <out> go the
# two executables as cargo built them, the signed mcbench-lite.app
# (macos/bundle.sh) and the archive (macos/archive.sh). The two architectures
# are two runs, on one Mac or on two; nothing joins them, because install.sh
# fetches the archive of the machine it runs on.
#
# It reads from the environment, and fails naming the one that is unset:
#   MCBENCH_DATA_BUNDLE      the Minecraft data bundle; mcbench-setup's build
#                            refuses one that is not native/minecraft-data.lock's
#   MCBENCH_UPDATE_KEY_FILE  the updater's minisign public key
#   MCBENCH_UPDATE_FEED      the address of latest.json
#   MCBENCH_SANDBOX_ASSETS_URL  optional: the folder a staging build fetches the
#                            sandbox assets from in place of the lock's url
#   CARGO_TARGET_DIR         the workspace's target folder; the build child's is
#                            the same path with -build-child after it
#
# MACOSX_DEPLOYMENT_TARGET is 12.0, the plan's minimum (Info.plist.in's
# LSMinimumSystemVersion). V8's prebuilt library in the build child is itself
# built for 12.0 (the v8 crate's build/config/mac/mac_sdk.gni), and the bash
# tool's VM uses Virtualization.framework calls that macOS 12 added (virtiofs
# sharing, stopWithCompletionHandler).
#
# Paths of the build machine are remapped (the repository to /mcbench-lite,
# CARGO_HOME to /cargo), so the binaries do not depend on where the checkout
# or the registry is. The bundle's version is the workspace's, and every
# archive entry carries the commit's time.
#
# mcbench-lite is built with profile-guided optimisation (pgo.md) when
# native/packaging/build/pgo/<target>.profdata exists (the engine's pinned
# profile, recorded on the corpus): the build uses it alone, with no
# instrumented build, and fails when an engine function has a record for
# other code or more engine functions than at recording (the .record file
# beside it) find no record: the engine changed and the profile must be
# recorded again. The profile and the build's log are kept in <out> as
# mcbench-lite.profdata and pgo.log, outside the bundle and the archive.
# When there is no pinned profile for <target> (today x86_64-apple-darwin),
# mcbench-lite is the release build with no profile, and neither file is
# written. MCBENCH_PGO_PIN=<recording> records the pinned profile instead (a
# private recording of the corpus, pgo-workload.js, with its .counts beside
# it): an instrumented build in $CARGO_TARGET_DIR-pgo, the replay of
# <recording> on its `headless engine` by pgo-replay.js with the pinned
# Node.js (node-pins.txt), the merge with llvm-profdata of the toolchain's
# llvm-tools, the profile reduced to the engine crate's functions and the
# build with it; <out>/mcbench-lite.profdata and <out>/mcbench-lite.record
# are then the files to copy to native/packaging/build/pgo/<target>.profdata
# and .record. The instrumented program runs here, so MCBENCH_PGO_PIN needs a
# Mac of <target>'s own architecture; every build is made on one (the
# release workflow builds x86_64-apple-darwin on an Intel runner).
set -euo pipefail
die() { echo "macos.sh: $*" >&2; exit 2; }
[ $# -eq 3 ] || die "usage: macos.sh <repository> <target> <out>"
repo=$1 target=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)
macos="$here/../macos"
case "$target" in aarch64-apple-darwin|x86_64-apple-darwin) ;; *) die "the target $target is not aarch64-apple-darwin or x86_64-apple-darwin" ;; esac
arch=${target%%-*}
machine=$(uname -m)
[ "${machine/arm64/aarch64}" = "$arch" ] || die "this Mac is $machine: $target is built on a Mac of its own architecture, which runs its instrumented program"
[ -n "${MCBENCH_DATA_BUNDLE:-}" ] || die "MCBENCH_DATA_BUNDLE is not set"
[ -n "${MCBENCH_UPDATE_KEY_FILE:-}" ] || die "MCBENCH_UPDATE_KEY_FILE is not set"
[ -n "${MCBENCH_UPDATE_FEED:-}" ] || die "MCBENCH_UPDATE_FEED is not set"
[ -n "${CARGO_TARGET_DIR:-}" ] || die "CARGO_TARGET_DIR is not set"
repo=$(cd "$repo" && pwd)
[ -z "$(git -C "$repo" status --porcelain)" ] || die "$repo has uncommitted or untracked files"
stamp=$(git -C "$repo" log -1 --format=%ct)
# The build runs from a copy of the tracked tree at one fixed path. Cargo
# puts a path dependency's absolute path into its crate hash (cargo 1.99.1:
# the engine's -C metadata differed between two checkout paths and not with
# other remap flags, another -Cprofile-use path or another CARGO_HOME), and
# that hash is in every mangled name, so the engine's pinned profile
# (pgo.md) matches only a build from the path it was recorded at. The copy
# is brought to the tree by content in two steps, as native/check.sh does:
# the listed files into an emptied stage folder, then the stage onto the copy
# with --checksum and --delete (no times), so an unchanged file keeps its
# time and cargo its build, and a file that left the tree leaves the copy
# (with --files-from, --delete removes nothing that left the list).
src=/private/tmp/mcbench-lite-build/src
stage=/private/tmp/mcbench-lite-build/stage
rm -rf "$stage"
mkdir -p "$src" "$stage"
git -C "$repo" ls-files -z native core renderer server README.md LICENSE NOTICE.md \
  | rsync -a --from0 --files-from=- "$repo/" "$stage/"
rsync -rlp --checksum --delete "$stage/" "$src/"
rm -rf "$stage"
repo=$src
mkdir -p "$out"
out=$(cd "$out" && pwd)
cargo_home=${CARGO_HOME:-$HOME/.cargo}
export MACOSX_DEPLOYMENT_TARGET=12.0
flags="--remap-path-prefix=$repo=/mcbench-lite"$'\x1f'"--remap-path-prefix=$cargo_home=/cargo"
export CARGO_ENCODED_RUSTFLAGS="$flags"

# rustup takes the toolchain from native/rust-toolchain.toml in both folders.
(cd "$repo/native" && rustup target add "$target" && rustup component add llvm-tools)

profile=$out/mcbench-lite.profdata
profdata="$(cd "$repo/native" && rustc --print sysroot)/lib/rustlib/$target/bin/llvm-profdata"
pinned=$repo/native/packaging/build/pgo/$target.profdata
if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
  # The pinned profile recorded here (pgo.md): the instrumented build, the
  # replay of the corpus recording on it, the merge.
  recording=$MCBENCH_PGO_PIN
  [ -f "$recording" ] && [ -f "${recording%.tar.zst}.counts" ] || die "MCBENCH_PGO_PIN names $recording, which is not a recording with its .counts beside it"
  pgo=$CARGO_TARGET_DIR-pgo
  rm -rf "$pgo/raw" "$pgo/node"
  mkdir -p "$pgo/raw" "$pgo/node"
  read -r node_url node_sum < <(awk -v p="macos-$arch" '$1 == p { print $2, $3 }' "$repo/native/packaging/build/node-pins.txt")
  if [ ! -f "$pgo/node.tar.gz" ]; then curl -fsSL -o "$pgo/node.tar.gz.part" "$node_url"; mv "$pgo/node.tar.gz.part" "$pgo/node.tar.gz"; fi
  echo "$node_sum  $pgo/node.tar.gz" | shasum -a 256 -c --quiet - || die "$pgo/node.tar.gz from $node_url is not the pinned file (sha256 $node_sum)"
  tar -xzf "$pgo/node.tar.gz" -C "$pgo/node" --strip-components=1
  SECONDS=0
  (cd "$repo/native" && CARGO_TARGET_DIR="$pgo" CARGO_ENCODED_RUSTFLAGS="$flags"$'\x1f'"-Cprofile-generate=$pgo/raw" \
    cargo build --release --locked -p mcbench-lite --features shipped-data --target "$target")
  echo "pgo: instrumented build ${SECONDS} s"; SECONDS=0
  "$pgo/node/bin/node" "$repo/native/packaging/build/pgo-replay.js" "$recording" "$MCBENCH_DATA_BUNDLE" "$pgo/work" "$pgo/$target/release/mcbench-lite"
  echo "pgo: workload ${SECONDS} s"
  ls "$pgo"/raw/*.profraw >/dev/null 2>&1 || die "the instrumented mcbench-lite wrote no profile"
  "$profdata" merge -o "$pgo/recorded.profdata" "$pgo"/raw/*.profraw
  echo "pgo: merged $(ls "$pgo"/raw/*.profraw | wc -l | tr -d ' ') raw profile files (the engine processes of one program share a file) into $(stat -f %z "$pgo/recorded.profdata") bytes, sha256 $(shasum -a 256 "$pgo/recorded.profdata" | cut -c1-64)"
  # The engine crate's records: those of its own code units, and the global
  # functions whose names hold the crate (the engine's generic code
  # instantiated in another crate's units carries that crate's hash, which a
  # version bump changes, so it is left out).
  "$profdata" merge --function='^mcbench_core\.' -o "$pgo/local.profdata" "$pgo/recorded.profdata"
  "$profdata" merge --function=mcbench_core --no-function='-cgu\.[0-9]+;' -o "$pgo/global.profdata" "$pgo/recorded.profdata"
  "$profdata" merge -o "$profile" "$pgo/local.profdata" "$pgo/global.profdata"
elif [ -f "$pinned" ]; then
  cp "$pinned" "$profile"
fi
SECONDS=0
if [ -n "${MCBENCH_PGO_PIN:-}" ] || [ -f "$pinned" ]; then
  (cd "$repo/native" && CARGO_ENCODED_RUSTFLAGS="$flags"$'\x1f'"-Cprofile-use=$profile"$'\x1f'"-Cllvm-args=-pgo-warn-missing-function" \
    cargo build --release --locked -p mcbench-lite --features shipped-data --target "$target") 2>&1 | tee "$out/pgo.log"
  echo "pgo: final build ${SECONDS} s"
  grep -q 'Compiling mcbench-lite' "$out/pgo.log" || die "the final build did not compile mcbench-lite with the profile ($out/pgo.log)"
  # The engine's profile: only the engine crate's code units are judged. The
  # engine functions -dead_strip removed from the recording's instrumented
  # program have no record; their number at recording is the .record's, and
  # a build may have no more.
  missing=$(grep -cE '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: no profile data available for function' "$out/pgo.log" || true)
  mismatched=$(grep -cE '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: .*hash mismatch' "$out/pgo.log" || true)
  if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
    printf 'engine-missing\t%s\n' "$missing" > "$out/mcbench-lite.record"
    echo "pgo: the engine's profile, $(stat -f %z "$profile") bytes, sha256 $(shasum -a 256 "$profile" | cut -c1-64)"
  else
    allowed=$(awk -F'\t' '$1 == "engine-missing" { print $2 }' "${pinned%.profdata}.record")
    [ "$mismatched" = 0 ] && [ "$missing" -le "$allowed" ] || die "the engine's pinned profile does not fit the engine: $mismatched engine functions with a hash mismatch, $missing without a record where it had $allowed ($out/pgo.log); the engine changed, so record the profile again (pgo.md, Recording the engine's profile)"
  fi
  echo "pgo: the engine's profile applied: $mismatched engine functions with a hash mismatch, $missing without a record"
else
  # No pinned profile for this target: the release build with no profile.
  (cd "$repo/native" && cargo build --release --locked -p mcbench-lite --features shipped-data --target "$target")
  echo "pgo: no pinned profile for $target, so mcbench-lite is built with none: ${SECONDS} s"
fi
(cd "$repo/native/build-child" && CARGO_TARGET_DIR="$CARGO_TARGET_DIR-build-child" cargo build --release --locked --target "$target")
cp -X "$CARGO_TARGET_DIR/$target/release/mcbench-lite" "$CARGO_TARGET_DIR-build-child/$target/release/mcbench-build-child" "$out/"

version=$(awk '/^\[workspace.package\]/ { p = 1; next } /^\[/ { p = 0 } p && $1 == "version" { gsub(/"/, "", $3); print $3 }' "$repo/native/Cargo.toml")
"$macos/bundle.sh" "$out/mcbench-lite" "$out/mcbench-build-child" "$version" "$out"
"$macos/archive.sh" "$out/mcbench-lite.app" "$stamp" "$out"
