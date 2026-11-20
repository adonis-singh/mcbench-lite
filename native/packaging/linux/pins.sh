# Sourced by the Linux build, packaging and test scripts: every image and
# downloaded tool they run, pinned. An image is named by its digest (the
# multi-platform index, so the same line serves x86_64 and aarch64); a
# download by its address and sha256.

# The release build: Debian 10 "buster" (glibc 2.28), the image
# scripts/release-engine.js builds the engine in (README.md says why this one).
BUILD_IMAGE=debian:buster@sha256:58ce6f1271ae1c8a2006ff7d3e54e9874d839f573d8009c20154ad0f2fb0a225

# The packaging tools: dpkg-deb, apt-ftparchive and gpg; rpmbuild, rpmsign
# and createrepo_c; makepkg.
DEBIAN_IMAGE=debian:12@sha256:268fb24c39bca92d326dd83ea3abebe4a3f7751c804ffef23bb6d857dd343ff8
FEDORA_IMAGE=fedora@sha256:a7d72ba2745b100a3351087472e5ee7b2450af7cf331e7673007e3817612839c
ARCH_IMAGE=archlinux@sha256:d4594bb2300cba11f41c2f27923eb56f63d9988909c402f9c95517d1e362c899

# rustup-init 1.29.3, as server/tools.js pins it.
RUSTUP_INIT_URL_x86_64=https://static.rust-lang.org/rustup/archive/1.29.3/x86_64-unknown-linux-gnu/rustup-init
RUSTUP_INIT_SHA256_x86_64=b1c8cb14e30889e7e2999e6b69e62d9e9b9ffc0f4df8180564d306d9680e6c55
RUSTUP_INIT_URL_aarch64=https://static.rust-lang.org/rustup/archive/1.29.3/aarch64-unknown-linux-gnu/rustup-init
RUSTUP_INIT_SHA256_aarch64=9f608c142e0e38a78957d0661ba1dfaf0b6fb37184cfaa4b9a0aa57af6184282

# appimagetool 1.9.1 and the static type-2 runtime of type2-runtime's
# release 20251108, which needs no libfuse2 on the user's machine.
APPIMAGETOOL_URL_x86_64=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256_x86_64=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
APPIMAGETOOL_URL_aarch64=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-aarch64.AppImage
APPIMAGETOOL_SHA256_aarch64=f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158
APPIMAGE_RUNTIME_URL_x86_64=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
APPIMAGE_RUNTIME_SHA256_x86_64=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
APPIMAGE_RUNTIME_URL_aarch64=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-aarch64
APPIMAGE_RUNTIME_SHA256_aarch64=00cbdfcf917cc6c0ff6d3347d59e0ca1f7f45a6df1a428a0d6d8a78664d87444

# The release addresses. Every package and script takes them from these two
# variables (a staging release sets both): the release assets, and the site,
# which holds the dnf repository's metadata.
MCBENCH_LITE_RELEASES=${MCBENCH_LITE_RELEASES:-https://github.com/adonis-singh/mcbench-lite/releases/latest/download}
MCBENCH_LITE_SITE=${MCBENCH_LITE_SITE:-https://adonis-singh.github.io/mcbench-lite}

# fetch <url> <sha256> <file>: downloads <url> to <file> unless <file> is
# already there, and fails unless its sha256 is <sha256>.
fetch() {
  if [ ! -f "$3" ]; then curl -fsSL -o "$3.part" "$1"; mv "$3.part" "$3"; fi
  echo "$2  $3" | sha256sum -c --quiet - || { echo "$3 from $1 is not the pinned file (sha256 $2)" >&2; exit 1; }
}

# The Debian and RPM names of an architecture.
deb_arch() { case $1 in x86_64) echo amd64 ;; aarch64) echo arm64 ;; *) echo "no architecture $1: x86_64 or aarch64" >&2; exit 1 ;; esac; }
