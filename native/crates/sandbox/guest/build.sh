#!/bin/bash
# native/crates/sandbox/guest/build.sh OUT aarch64|x86_64
#
# The bash sandbox's guest base for one architecture, as a pinned release
# asset: the macOS VM boots its kernel and initramfs (aarch64 on Apple
# silicon, x86_64 on Intel Macs), and the Windows WSL2 distro imports its
# userland.tar (x86_64; aarch64 on Windows on ARM).
#
#   OUT/mcbench-sandbox-guest-<arch>.tar.zst   kernel, initramfs, userland.tar, manifest.json
#   OUT/mcbench-sandbox-guest-<arch>.sha256    its sha256
#   OUT/mcbench-sandbox-guest-<arch>/          the four files unpacked
#   OUT/debs/                                  the pinned Debian packages, fetched once
#
# Steps: the static agent (cargo, musl, paths inside the binary remapped so
# that it does not depend on where the checkout or cargo's home is), then
# build.py (pinned Debian 13 packages from snapshot.debian.org, checked by
# sha256; a deterministic cpio and tar), then zstd -19 of the asset.
#
# Needs: the workspace's toolchain (rustup run 1.99.1, with the target
# <arch>-unknown-linux-musl added), python3, zstd, sha256sum or shasum,
# and the internet the first time. Runs on Linux, macOS or WSL alike;
# nothing in it needs root, Docker or a Linux kernel.
set -euo pipefail
OUT=$(mkdir -p "${1:?usage: build.sh OUT aarch64|x86_64}" && cd "$1" && pwd)
ARCH=${2:?usage: build.sh OUT aarch64|x86_64}
case "$ARCH" in aarch64 | x86_64) ;; *) echo "build.sh: ARCH is aarch64 or x86_64" >&2; exit 2 ;; esac
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
NATIVE=$(cd "$HERE/../../.." && pwd)
TARGET="$ARCH-unknown-linux-musl"
sha() { if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -c1-64; else shasum -a 256 "$1" | cut -c1-64; fi; }

echo "[guest] mcbench-sandbox-agent for $TARGET" >&2
CARGO_HOME_DIR=${CARGO_HOME:-$HOME/.cargo}
RUSTFLAGS="--remap-path-prefix=$NATIVE=/mcbench/native --remap-path-prefix=$CARGO_HOME_DIR=/cargo -C strip=symbols" \
  CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  rustup run 1.99.1 cargo build --manifest-path "$NATIVE/Cargo.toml" --release --locked \
  -p mcbench-sandbox --bin mcbench-sandbox-agent --target "$TARGET" >&2
TARGET_DIR=${CARGO_TARGET_DIR:-$NATIVE/target}
AGENT="$TARGET_DIR/$TARGET/release/mcbench-sandbox-agent"

python3 -I "$HERE/build.py" "$OUT" "$ARCH" "$AGENT"
ASSET="$OUT/mcbench-sandbox-guest-$ARCH.tar.zst"
zstd -q -19 -f "$OUT/mcbench-sandbox-guest-$ARCH.tar" -o "$ASSET"
/bin/rm -f "$OUT/mcbench-sandbox-guest-$ARCH.tar"
SHA=$(sha "$ASSET")
echo "$SHA" > "$OUT/mcbench-sandbox-guest-$ARCH.sha256"
echo "asset $ASSET sha256 $SHA size $(wc -c < "$ASSET" | tr -d ' ') bytes"
