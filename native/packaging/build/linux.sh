#!/usr/bin/env bash
# The Linux release build: the payload folder for one architecture.
#
#   MCBENCH_DATA_BUNDLE=<bundle> native/packaging/build/linux.sh <repository> <arch> <out>
#
# <arch> is x86_64 or aarch64 and must be this machine's (each is built
# natively: aarch64 on an arm64 machine). <out> must not exist; it becomes the
# payload folder, holding exactly the three programs the app ships on Linux:
#
#   mcbench-lite            native/ --release --locked --features shipped-data
#   mcbench-build-child     native/build-child --release --locked
#   mcbench-sandbox-agent   native/ -p mcbench-sandbox, static, for <arch>-unknown-linux-musl
#
# and on x86_64 the software Vulkan driver the renderer opens on a machine
# with no GPU, with its licences:
#
#   libvk_swiftshader.so    SwiftShader, by swiftshader.sh, refused unless its sha256 is the pin's
#   licenses/swiftshader/   the licences of everything compiled into it
#
# aarch64 has no driver: it needs SwiftShader's LLVM reactor built on an
# arm64 machine, which has never been built. swiftshader.sh keeps its source
# and the checked driver in MCBENCH_SWIFTSHADER_CACHE (default
# <work>/swiftshader), so a later build copies the driver without building it.
#
# Everything is built inside BUILD_IMAGE (native/packaging/linux/pins.sh, by
# digest), with Rust 1.99.1 installed in it by the pinned rustup-init, from
# the repository mounted read-only at /src and into /cache, so a build depends
# on the commit and the image, not on the machine or the folder it ran in.
# /cache is <out>.work (or MCBENCH_BUILD_WORK): rustup, cargo's registry and
# the target folders, kept for the next build. Delete it for a cold build.
#
# MCBENCH_DATA_BUNDLE names the Minecraft data bundle (its sha256 must be
# native/minecraft-data.lock's; the build refuses any other).
# MCBENCH_UPDATE_KEY_FILE and MCBENCH_UPDATE_FEED, when set, reach the build
# as the updater crate reads them (the key file is mounted read-only).
# MCBENCH_SANDBOX_ASSETS_URL, when set, reaches the build as the setup crate
# reads it: the folder a staging build fetches the sandbox assets from in
# place of native/sandbox-assets.lock's url (a release build leaves it unset).
# CARGO_BUILD_JOBS is cargo's job count and the container's CPU limit, and
# MCBENCH_BUILD_MEMORY the container's memory limit (docker's --memory); they
# default to 2 and 8g, which a shared build machine can spare, and a release
# runner raises them.
#
# mcbench-lite is built with profile-guided optimisation (pgo.md) when
# native/packaging/build/pgo/<arch>-unknown-linux-gnu.profdata exists (the
# engine's pinned profile, recorded on the corpus): the build uses it alone,
# with no instrumented build, and fails when an engine function has a record
# for other code or more engine functions than at recording (the .record
# file beside it) find no record: the engine changed and the profile must be
# recorded again. The build is given --target, so the flags reach the
# shipped program and not the build scripts. The profile is kept as
# <out>.profdata and the build's log as <out>.pgo.log, beside the payload
# folder and outside it. When there is no pinned profile for <arch> (today
# aarch64), mcbench-lite is the release build with no profile, and neither
# file is written.
#
# MCBENCH_PGO_PIN=<recording> records the pinned profile instead: an
# instrumented build, <recording> (a private recording of the corpus,
# pgo-workload.js, with its .counts beside it) replayed on its `headless
# engine` by pgo-replay.js with the pinned Node.js (node-pins.txt), the merge
# with llvm-profdata of the toolchain's llvm-tools, the profile reduced to
# the engine crate's functions, and the build with it as above;
# <out>.profdata and <out>.record are then the files to copy to
# native/packaging/build/pgo/.
#
# It ends by printing, for each program, its size, the shared libraries it
# names (DT_NEEDED) and the newest glibc symbol version it needs.
set -euo pipefail

[ $# -eq 3 ] || { echo "usage: native/packaging/build/linux.sh <repository> <arch> <out>" >&2; exit 2; }
repo=$(cd "$1" && pwd)
arch=$2
out=$3
here=$(cd "$(dirname "$0")/../linux" && pwd)
. "$here/pins.sh"

[ "$arch" = "$(uname -m)" ] || { echo "this machine is $(uname -m): $arch is built on an $arch machine" >&2; exit 1; }
deb_arch "$arch" >/dev/null
[ -n "${MCBENCH_DATA_BUNDLE:-}" ] || { echo "MCBENCH_DATA_BUNDLE is not set: name the bundle native/minecraft-data.lock describes" >&2; exit 1; }
[ -f "$MCBENCH_DATA_BUNDLE" ] || { echo "MCBENCH_DATA_BUNDLE names $MCBENCH_DATA_BUNDLE, which is not a file" >&2; exit 1; }
[ ! -e "$out" ] || { echo "$out exists: the payload folder is written fresh" >&2; exit 1; }
[ ! -e "$out.profdata" ] || { echo "$out.profdata exists: the profile is recorded fresh" >&2; exit 1; }
[ -f "$repo/native/Cargo.lock" ] || { echo "$repo is not the repository (no native/Cargo.lock)" >&2; exit 1; }

work=${MCBENCH_BUILD_WORK:-$out.work}
mkdir -p "$work" "$out"
work=$(cd "$work" && pwd)
out=$(cd "$out" && pwd)
url_var=RUSTUP_INIT_URL_$arch sum_var=RUSTUP_INIT_SHA256_$arch
fetch "${!url_var}" "${!sum_var}" "$work/rustup-init"
chmod 755 "$work/rustup-init"

bundle_name=$(basename "$MCBENCH_DATA_BUNDLE")
mounts=(-v "$repo:/src:ro" -v "$work:/cache" -v "$out:/out" -v "$(cd "$(dirname "$MCBENCH_DATA_BUNDLE")" && pwd)/$bundle_name:/bundle/$bundle_name:ro")
envs=(-e "MCBENCH_DATA_BUNDLE=/bundle/$bundle_name")
if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
  [ -f "$MCBENCH_PGO_PIN" ] && [ -f "${MCBENCH_PGO_PIN%.tar.zst}.counts" ] || { echo "MCBENCH_PGO_PIN names $MCBENCH_PGO_PIN, which is not a recording with its .counts beside it" >&2; exit 1; }
  pin=$(cd "$(dirname "$MCBENCH_PGO_PIN")" && pwd)/$(basename "$MCBENCH_PGO_PIN")
  mounts+=(-v "$pin:/pin/recording.tar.zst:ro" -v "${pin%.tar.zst}.counts:/pin/recording.counts:ro")
  envs+=(-e MCBENCH_PGO_PIN=1)
  read -r node_url node_sum < <(awk -v p="linux-$arch" '$1 == p { print $2, $3 }' "$repo/native/packaging/build/node-pins.txt")
  fetch "$node_url" "$node_sum" "$work/node.tar.xz"
fi
if [ -n "${MCBENCH_UPDATE_KEY_FILE:-}" ]; then
  [ -f "$MCBENCH_UPDATE_KEY_FILE" ] || { echo "MCBENCH_UPDATE_KEY_FILE names $MCBENCH_UPDATE_KEY_FILE, which is not a file" >&2; exit 1; }
  mounts+=(-v "$(cd "$(dirname "$MCBENCH_UPDATE_KEY_FILE")" && pwd)/$(basename "$MCBENCH_UPDATE_KEY_FILE"):/keys/minisign.pub:ro")
  envs+=(-e MCBENCH_UPDATE_KEY_FILE=/keys/minisign.pub)
fi
[ -z "${MCBENCH_UPDATE_FEED:-}" ] || envs+=(-e "MCBENCH_UPDATE_FEED=$MCBENCH_UPDATE_FEED")
[ -z "${MCBENCH_SANDBOX_ASSETS_URL:-}" ] || envs+=(-e "MCBENCH_SANDBOX_ASSETS_URL=$MCBENCH_SANDBOX_ASSETS_URL")
jobs=${CARGO_BUILD_JOBS:-2}
limits=(--cpus "$jobs" --memory "${MCBENCH_BUILD_MEMORY:-8g}")
envs+=(-e "CARGO_BUILD_JOBS=$jobs")

# The container's script. Buster's packages come from archive.debian.org,
# which no longer changes. The headers are those of the libraries the window
# toolkit links (xcb, xkbcommon, freetype, fontconfig) and wayland's.
script=$(cat <<'SCRIPT'
set -euo pipefail
# The work folder and the payload go back to the caller's user however the
# build ends, so a failed build can be inspected and deleted without root.
trap 'chown -R "$HOST_UID:$HOST_GID" /cache /out' EXIT
arch=$(uname -m)
printf 'deb http://archive.debian.org/debian buster main\ndeb http://archive.debian.org/debian-security buster/updates main\n' > /etc/apt/sources.list
export DEBIAN_FRONTEND=noninteractive
apt-get -qq update
apt-get -qq install -y --no-install-recommends ca-certificates curl gcc g++ libc6-dev make pkg-config binutils \
  libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libfreetype6-dev libfontconfig1-dev libwayland-dev xz-utils >/dev/null
export RUSTUP_HOME=/cache/rustup CARGO_HOME=/cache/cargo PATH=/cache/cargo/bin:$PATH
[ -x /cache/cargo/bin/rustup ] || /cache/rustup-init -y --quiet --no-modify-path --profile minimal --default-toolchain none
cd /src/native
rustup show active-toolchain >/dev/null 2>&1 || rustup install
rustup target add "$arch-unknown-linux-musl"
rustup component add llvm-tools
gnu=$arch-unknown-linux-gnu
profdata="$(rustc --print sysroot)/lib/rustlib/$gnu/bin/llvm-profdata"
pinned=/src/native/packaging/build/pgo/$gnu.profdata
rm -rf /cache/pgo /cache/node
mkdir -p /cache/pgo/raw /cache/node
if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
  # The pinned profile recorded here (pgo.md): the instrumented build, the
  # replay of the corpus recording on it, the merge.
  tar -xJf /cache/node.tar.xz -C /cache/node --strip-components=1
  SECONDS=0
  # The instrumented program keeps every function (its profile records their
  # addresses), among them the xkbcommon crate's wrapper of
  # xkb_utf32_to_keysym, which buster's libxkbcommon 0.8 does not have and the
  # shipped build drops as unused. In the instrumented program alone the
  # symbol is defined as address 0, so it links and loads; the workload runs
  # the engine alone and never calls it.
  cargo build --release --locked --features shipped-data -p mcbench-lite --target "$gnu" --target-dir /cache/target-pgo \
    --config "build.rustflags=['-Cprofile-generate=/cache/pgo/raw', '-Clink-arg=-Wl,--defsym=xkb_utf32_to_keysym=0']"
  echo "pgo: instrumented build ${SECONDS} s"; SECONDS=0
  /cache/node/bin/node /src/native/packaging/build/pgo-replay.js /pin/recording.tar.zst "$MCBENCH_DATA_BUNDLE" /cache/pgo/work "/cache/target-pgo/$gnu/release/mcbench-lite"
  echo "pgo: workload ${SECONDS} s"; SECONDS=0
  ls /cache/pgo/raw/*.profraw >/dev/null 2>&1 || { echo "pgo: the instrumented mcbench-lite wrote no profile" >&2; exit 1; }
  "$profdata" merge -o /cache/pgo/mcbench-lite.profdata /cache/pgo/raw/*.profraw
  echo "pgo: merged $(ls /cache/pgo/raw/*.profraw | wc -l) raw profile files (the engine processes of one program share a file) into $(stat -c %s /cache/pgo/mcbench-lite.profdata) bytes, sha256 $(sha256sum /cache/pgo/mcbench-lite.profdata | cut -c1-64)"
fi
if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
  # The engine crate's records: those of its own code units, and the global
  # functions whose names hold the crate (the engine's generic code
  # instantiated in another crate's units carries that crate's hash, which a
  # version bump changes, so it is left out).
  "$profdata" merge --function='^mcbench_core\.' -o /cache/pgo/local.profdata /cache/pgo/mcbench-lite.profdata
  "$profdata" merge --function=mcbench_core --no-function='-cgu\.[0-9]+;' -o /cache/pgo/global.profdata /cache/pgo/mcbench-lite.profdata
  "$profdata" merge -o /cache/pgo/pinned.profdata /cache/pgo/local.profdata /cache/pgo/global.profdata
  profile=/cache/pgo/pinned.profdata
elif [ -f "$pinned" ]; then
  profile=$pinned
fi
SECONDS=0
if [ -n "${MCBENCH_PGO_PIN:-}" ] || [ -f "$pinned" ]; then
  cargo build --release --locked --features shipped-data -p mcbench-lite --target "$gnu" --target-dir /cache/target \
    --config "build.rustflags=['-Cprofile-use=$profile', '-Cllvm-args=-pgo-warn-missing-function']" \
    2>&1 | tee /cache/pgo/build.log
  echo "pgo: final build ${SECONDS} s"
  cp "$profile" /cache/pgo/used.profdata
  # The engine's profile: only the engine crate's code units are judged.
  missing=$(grep -cE '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: no profile data available for function' /cache/pgo/build.log || true)
  mismatched=$(grep -cE '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: .*hash mismatch' /cache/pgo/build.log || true)
  if [ -n "${MCBENCH_PGO_PIN:-}" ]; then
    printf 'engine-missing\t%s\n' "$missing" > /cache/pgo/pinned.record
    echo "pgo: the engine's profile, $(stat -c %s "$profile") bytes, sha256 $(sha256sum "$profile" | cut -c1-64)"
  else
    allowed=$(awk -F'\t' '$1 == "engine-missing" { print $2 }' "${pinned%.profdata}.record")
    [ "$mismatched" = 0 ] && [ "$missing" -le "$allowed" ] || { echo "pgo: the engine's pinned profile does not fit the engine: $mismatched engine functions with a hash mismatch, $missing without a record where it had $allowed (/cache/pgo/build.log); the engine changed, so record the profile again (pgo.md, Recording the engine's profile)" >&2; exit 1; }
  fi
  echo "pgo: the engine's profile applied: $mismatched engine functions with a hash mismatch, $missing without a record"
else
  # No pinned profile for this target: the release build with no profile.
  cargo build --release --locked --features shipped-data -p mcbench-lite --target "$gnu" --target-dir /cache/target
  echo "pgo: no pinned profile for $gnu, so mcbench-lite is built with none: ${SECONDS} s"
fi
cargo build --release --locked -p mcbench-sandbox --bin mcbench-sandbox-agent --target "$arch-unknown-linux-musl" --target-dir /cache/target
(cd build-child && cargo build --release --locked --target-dir /cache/target-child)
cp "/cache/target/$gnu/release/mcbench-lite" /cache/target-child/release/mcbench-build-child \
  "/cache/target/$arch-unknown-linux-musl/release/mcbench-sandbox-agent" /out/
for f in /out/*; do
  glibc=$(objdump -T "$f" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1 || true)
  printf '%s: %s bytes; needs %s; %s\n' "$(basename "$f")" "$(stat -c %s "$f")" "${glibc:-no glibc}" \
    "$(readelf -d "$f" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tr '\n' ' ')"
done
SCRIPT
)
docker run --rm --name "mcbn-s5-linux-build-$$" "${limits[@]}" "${mounts[@]}" "${envs[@]}" \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" "$BUILD_IMAGE" bash -c "$script"
[ "$arch" != x86_64 ] || "$here/../build/swiftshader.sh" "${MCBENCH_SWIFTSHADER_CACHE:-$work/swiftshader}" "$out"
if [ -n "${MCBENCH_PGO_PIN:-}" ] || [ -f "$repo/native/packaging/build/pgo/$arch-unknown-linux-gnu.profdata" ]; then
  cp "$work/pgo/used.profdata" "$out.profdata"
  cp "$work/pgo/build.log" "$out.pgo.log"
fi
[ -z "${MCBENCH_PGO_PIN:-}" ] || cp "$work/pgo/pinned.record" "$out.record"
