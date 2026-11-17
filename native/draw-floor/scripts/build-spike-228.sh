#!/bin/sh
# On adipc, after build-spike.sh: the same `shot` and `window` built inside
# almalinux:8 (glibc 2.28, the engine's floor) for the container proofs,
# with the machine's Rust toolchain mounted read only.
# Output: $R/target-228/release/{shot,window}
set -eu
. "$(dirname "$0")/env.sh"
TC=$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu
mkdir -p "$R/target-228"
docker run --rm --cpus 24 -v "$TC:/tc:ro" -v "$R:/r" -w /r/src/native/spikes/renderer \
  -e PATH=/tc/bin:/usr/bin:/bin -e CARGO_HOME=/r/cargo-home -e CARGO_TARGET_DIR=/r/target-228 -e CARGO_BUILD_JOBS=24 \
  almalinux:8 bash -euc '
  trap "chown -R '"$(id -u):$(id -g)"' /r/target-228 /r/cargo-home" EXIT
  dnf -q -y install gcc >/dev/null
  cargo build --release --bin shot --bin window 2>&1 | tail -2
'
objdump -T "$R/target-228/release/shot" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
