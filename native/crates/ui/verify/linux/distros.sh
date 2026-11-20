#!/usr/bin/env bash
# The dev binary on five distributions, each in four states, in fresh
# containers with a private Xvfb inside: does it start, stay up, name its
# renderer and draw its text?
#
#   distros.sh <out dir> [<distribution>...]
#
# Distributions: debian-12 debian-13 ubuntu-24.04 fedora arch (all when none
# is named), from the images the packaging tests pin (the same digests). The
# binary is built once, in release (`cargo build --release --locked -p
# mcbench-ui --bin mcbench-ui-dev`), as the release build makes the app
# (native/packaging/build/linux.sh): inside Debian 10's image (glibc 2.28,
# so it runs on all five; a build on a newer system needs that system's
# glibc), with Rust installed there by the pinned rustup-init, the tree
# mounted read-only, and rustup, cargo's registry and the target folder in
# MCBENCH_BUILD_WORK (default <out dir>/build), kept for the next run. The
# binary is mounted read-only into every container.
#
# States, each a fresh container from an image made here:
#
#   fonts    the libraries the packages depend on, fontconfig and the
#            distribution's default fonts (DEFAULT_FONTS below)
#   none     the libraries only: no fontconfig (no library, no
#            configuration) and no font file at all
#   free     the libraries, fontconfig and GNU FreeFont only (Arch's
#            gnu-free-fonts; the other systems' package of the same fonts;
#            on Fedora, whose fontconfig requires its default fonts, those
#            are removed again)
#   novulkan the fonts state with Vulkan hidden (VK_ICD_FILENAMES and
#            VK_DRIVER_FILES name a file that does not exist)
#
# and on debian-12 one more, noadapter: Vulkan hidden and GL hidden too
# (glvnd's EGL vendor list empty), where nothing can draw the window and the
# app must end with a sentence and a nonzero status.
#
# Each run (distro_inside.sh, in the container) starts Xvfb, then the dev
# binary on the gate fixture's new chat (--open /new) at 1280x820 with --log, waits for the
# renderer line in the log, lets it draw for SETTLE seconds, copies Xvfb's
# frame buffer, reads the window's WM_CLASS (fonts state: x11-utils is
# installed there) and checks the process is still up, then quits it by
# SIGTERM. distro_shot.py then turns the frame buffer into screen.png and
# checks that the empty chat's heading and the sidebar's rows hold text.
# Results in <out dir>/<distribution>-<state>/; a table in
# <out dir>/summary.txt.
#
# Images are named mcbn-s4-linux-*, containers mcbn-s4-linux-<this run's
# pid>-*, so two runs side by side never remove each other's; every container runs
# with --cpus 4 and --memory 6g, no
# privileges, the default bridge network (package installs), no mount but
# the out dir, this folder and the binary; it is removed when it ends, and
# the images made here are removed at the end (MCBENCH_KEEP_IMAGES=1 keeps
# them for a rerun).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$1"; shift
mkdir -p "$out"
out="$(cd "$out" && pwd)"
distros=("$@")
[ ${#distros[@]} -gt 0 ] || distros=(debian-12 debian-13 ubuntu-24.04 fedora arch)
settle="${SETTLE:-12}"
images=mcbn-s4-linux
prefix=$images-$$

image_of() {
  case $1 in
    debian-12) echo debian:12@sha256:268fb24c39bca92d326dd83ea3abebe4a3f7751c804ffef23bb6d857dd343ff8 ;;
    debian-13) echo debian:13@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6 ;;
    ubuntu-24.04) echo ubuntu:24.04@sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8 ;;
    fedora) echo fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c ;;
    arch) echo archlinux@sha256:d4594bb2300cba11f41c2f27923eb56f63d9988909c402f9c95517d1e362c899 ;;
    *) echo "distros.sh: no distribution $1" >&2; exit 2 ;;
  esac
}

# The install command of each state's packages on each system. LIBS are the
# packages' library dependencies (native/packaging/linux: deb.sh, rpm.sh,
# PKGBUILD) with Mesa's software drivers and Xvfb, never fontconfig or a
# font; DEFAULT_FONTS what fontconfig and the system's default fonts are;
# FREE fontconfig with GNU FreeFont only.
install_of() {
  local system=$1 state=$2
  case $system in
    debian-* | ubuntu-*)
      local libs="libxcb1 libxkbcommon0 libxkbcommon-x11-0 libegl1 libegl-mesa0 libgl1-mesa-dri libwayland-client0 libwayland-egl1 libvulkan1 mesa-vulkan-drivers xvfb"
      local fonts="fontconfig fonts-dejavu-core x11-utils"
      [ "$system" = ubuntu-24.04 ] && fonts="fontconfig fonts-ubuntu fonts-dejavu-core x11-utils"
      local free="fontconfig fonts-freefont-ttf"
      local pkgs=$libs
      case $state in fonts) pkgs="$libs $fonts" ;; free) pkgs="$libs $free" ;; esac
      echo "export DEBIAN_FRONTEND=noninteractive; apt-get -qq update && apt-get -qq install -y --no-install-recommends $pkgs >/dev/null && rm -rf /var/lib/apt/lists/*" ;;
    fedora)
      local libs="libxcb libxkbcommon libxkbcommon-x11 libglvnd-egl mesa-libEGL mesa-dri-drivers libwayland-client libwayland-egl vulkan-loader mesa-vulkan-drivers xorg-x11-server-Xvfb"
      local pkgs=$libs
      case $state in
        fonts) pkgs="$libs fontconfig default-fonts-core-sans default-fonts-core-mono xprop xwininfo" ;;
        free) pkgs="$libs fontconfig gnu-free-sans-fonts gnu-free-mono-fonts" ;;
      esac
      # fontconfig requires Fedora's default fonts: the free state removes
      # them again, leaving FreeFont only.
      local strip=""
      [ "$state" = free ] && strip=" && rpm -e --nodeps \$(rpm -qa 'default-fonts*' 'google-noto*' 'abattis-cantarell*')"
      echo "dnf -q -y --setopt=install_weak_deps=False install $pkgs >/dev/null$strip && dnf clean all >/dev/null" ;;
    arch)
      local libs="libxcb libxkbcommon libxkbcommon-x11 libglvnd mesa vulkan-swrast vulkan-icd-loader wayland xorg-server-xvfb"
      local pkgs=$libs
      case $state in
        fonts) pkgs="$libs fontconfig noto-fonts xorg-xprop xorg-xwininfo" ;;
        free) pkgs="$libs fontconfig gnu-free-fonts" ;;
      esac
      echo "pacman -Syu --noconfirm --needed -q $pkgs >/dev/null 2>&1 && pacman -Scc --noconfirm >/dev/null 2>&1" ;;
  esac
}

# Every container of this script: pinned to the CPU list, 6 GB, no
# privileges, removed when it ends.
run_container() {
  local name=$1; shift
  docker run --rm --name "$name" --cpus 4 --memory 6g --security-opt no-new-privileges "$@"
}

made_images=()
cleanup() {
  for name in $(docker ps -a --format '{{.Names}}' | grep "^$prefix-" || true); do
    docker rm -f "$name" >/dev/null 2>&1 || true
  done
  if [ "${MCBENCH_KEEP_IMAGES:-0}" != 1 ]; then
    for image in "${made_images[@]}"; do docker image rm "$image" >/dev/null 2>&1 || true; done
  fi
}
trap cleanup EXIT

# The image of one state: a container installs its packages, then is
# committed.
make_image() {
  local system=$1 state=$2 tag="$images-$1:$2"
  if docker image inspect "$tag" >/dev/null 2>&1; then made_images+=("$tag"); return; fi
  local name="$prefix-prep-$system-$state"
  docker run --name "$name" --cpus 4 --memory 6g "$(image_of "$system")" \
    sh -c "$(install_of "$system" "$state")" >"$out/prep-$system-$state.log" 2>&1 \
    || { tail -20 "$out/prep-$system-$state.log" >&2; docker rm -f "$name" >/dev/null; echo "distros.sh: installing $system $state failed" >&2; exit 1; }
  docker commit "$name" "$tag" >/dev/null
  docker rm "$name" >/dev/null
  made_images+=("$tag")
}

echo "== building the dev binary (release, in Debian 10)"
work="${MCBENCH_BUILD_WORK:-$out/build}"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
repo="$(cd "$here/../../../../.." && pwd)"
rustup_init_url=https://static.rust-lang.org/rustup/archive/1.29.3/x86_64-unknown-linux-gnu/rustup-init
rustup_init_sha256=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
[ -f "$work/rustup-init" ] || curl -fsSL -o "$work/rustup-init" "$rustup_init_url"
echo "$rustup_init_sha256  $work/rustup-init" | sha256sum -c --quiet - \
  || { echo "distros.sh: $work/rustup-init is not the pinned rustup-init" >&2; exit 1; }
chmod 755 "$work/rustup-init"
build_script=$(cat <<'SCRIPT'
set -euo pipefail
printf 'deb http://archive.debian.org/debian buster main\ndeb http://archive.debian.org/debian-security buster/updates main\n' > /etc/apt/sources.list
export DEBIAN_FRONTEND=noninteractive
apt-get -qq update
apt-get -qq install -y --no-install-recommends ca-certificates curl gcc g++ libc6-dev make pkg-config binutils \
  libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libfreetype6-dev libfontconfig1-dev libwayland-dev >/dev/null
export RUSTUP_HOME=/cache/rustup CARGO_HOME=/cache/cargo PATH=/cache/cargo/bin:$PATH
[ -x /cache/cargo/bin/rustup ] || /cache/rustup-init -y --quiet --no-modify-path --profile minimal --default-toolchain none
cd /src/native
rustup show active-toolchain >/dev/null 2>&1 || rustup install
cargo build --release --locked -p mcbench-ui --bin mcbench-ui-dev --target-dir /cache/target
chown -R "$HOST_UID:$HOST_GID" /cache
SCRIPT
)
docker run --rm --name "$prefix-build" --cpus 4 --memory 6g -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" -v "$repo:/src:ro" -v "$work:/cache" \
  debian:buster@sha256:58ce6f1271ae1c8a2006ff7d3e54e9874d839f573d8009c20154ad0f2fb0a225 bash -c "$build_script" \
  >"$out/build.log" 2>&1 || { tail -30 "$out/build.log" >&2; exit 1; }
cp "$work/target/release/mcbench-ui-dev" "$out/mcbench-ui-dev"
bin="$out/mcbench-ui-dev"
{
  echo "the binary: $(sha256sum "$bin" | cut -d' ' -f1), $(stat -c %s "$bin") bytes"
  echo "the newest glibc symbol version it needs: $(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)"
  echo "the libraries it names (DT_NEEDED): $(readelf -d "$bin" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tr '\n' ' ')"
  echo "ldd on the build machine:"; ldd "$bin"
} >"$out/binary.txt"
cat "$out/binary.txt"
[ "${MCBENCH_ONLY_BUILD:-0}" != 1 ] || exit 0

printf '%-14s %-9s %-4s %-52s %-44s %-32s %-5s %s\n' system state up renderer 'fonts (ui / data)' wm_class text exit >"$out/summary.txt"
for system in "${distros[@]}"; do
  states=(fonts none free novulkan)
  [ "$system" = debian-12 ] && states+=(noadapter)
  for state in "${states[@]}"; do
    image_state=$state
    case $state in novulkan | noadapter) image_state=fonts ;; esac
    echo "== $system $state"
    make_image "$system" "$image_state"
    dir="$out/$system-$state"
    rm -rf "$dir"; mkdir -p "$dir"
    env=()
    case $state in
      novulkan) env=(-e VK_ICD_FILENAMES=/nonexistent.json -e VK_DRIVER_FILES=/nonexistent.json) ;;
      noadapter) env=(-e VK_ICD_FILENAMES=/nonexistent.json -e VK_DRIVER_FILES=/nonexistent.json
                      -e __EGL_VENDOR_LIBRARY_FILENAMES=/nonexistent.json) ;;
    esac
    run_container "$prefix-$system-$state" --user "$(id -u):$(id -g)" -e HOME=/tmp/home "${env[@]}" \
      -e SETTLE="$settle" -e STATE="$state" \
      -v "$bin:/app/mcbench-ui-dev:ro" -v "$here/distro_inside.sh:/app/inside.sh:ro" -v "$dir:/out" \
      "$images-$system:$image_state" bash /app/inside.sh >"$dir/run.txt" 2>&1 || true
    python3 -I "$here/distro_shot.py" "$dir" "$state" >"$dir/check.txt" 2>&1 || true
    tail -1 "$dir/check.txt"
    python3 -I "$here/distro_shot.py" --row "$system" "$state" "$dir" >>"$out/summary.txt"
  done
done
cat "$out/summary.txt"
