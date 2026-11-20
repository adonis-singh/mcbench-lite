#!/usr/bin/env bash
# The pane's pointer lock on Wayland (vendor patch 50:
# zwp_pointer_constraints_v1 and zwp_relative_pointer_v1), which cage does
# not offer: check_pane_lock.py in a private session whose compositor is a
# headless sway (session.sh, MCBENCH_COMPOSITOR=sway), inside a container
# with no network and no display of the host's.
#
#   wayland_lock.sh <out dir>
#
# The dev binary is <out dir>/mcbench-ui-dev, the release build distros.sh
# makes in Debian 10 (MCBENCH_ONLY_BUILD=1 distros.sh <out dir>). The image,
# mcbn-s4-pane-sway:1, is Debian 13 (the digest the packaging tests pin,
# already present) with sway, foot, wtype, grim, AT-SPI, D-Bus and Mesa's
# software drivers, made by a container that installs them and is
# committed; it is removed at the end (MCBENCH_KEEP_IMAGES=1 keeps it).
# The check runs as the caller's user (an account of the same uid in the
# image), --cpus 4 --memory 6g, --network none, the verify folder and the
# binary mounted read-only, <out dir> the only folder written. Results in
# <out dir>/run (check.log, the pictures, sway.log under session/).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$(cd "$1" && pwd)"
bin="$out/mcbench-ui-dev"
[ -x "$bin" ] || { echo "wayland_lock.sh: no $bin (MCBENCH_ONLY_BUILD=1 distros.sh $out)" >&2; exit 2; }
image=mcbn-s4-pane-sway:1
base=debian:13@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6
uid="$(id -u)"; gid="$(id -g)"

cleanup() {
  for name in $(docker ps -a --format '{{.Names}}' | grep '^mcbn-s4-pane-' || true); do
    docker rm -f "$name" >/dev/null 2>&1 || true
  done
  [ "${MCBENCH_KEEP_IMAGES:-0}" = 1 ] || docker image rm "$image" >/dev/null 2>&1 || true
}
trap cleanup EXIT

if ! docker image inspect "$image" >/dev/null 2>&1; then
  packages="sway foot wtype grim python3 python3-gi gir1.2-atspi-2.0 at-spi2-core dbus dbus-bin dbus-daemon dconf-cli \
python3-pil procps iproute2 libvulkan1 mesa-vulkan-drivers libegl1 libegl-mesa0 libgl1-mesa-dri libwayland-client0 libwayland-egl1 \
libxkbcommon0 libxcb1 libxkbcommon-x11-0 fontconfig fonts-dejavu-core"
  docker run --name mcbn-s4-pane-prep --cpus 4 --memory 6g "$base" sh -c "
    export DEBIAN_FRONTEND=noninteractive
    apt-get -qq update && apt-get -qq install -y --no-install-recommends $packages >/dev/null && rm -rf /var/lib/apt/lists/*
    ln -s /usr/libexec/at-spi2-registryd /usr/lib/at-spi2-registryd
    groupadd -g $gid pane && useradd -u $uid -g $gid -M -d /out/home pane" >"$out/prep.log" 2>&1 \
    || { tail -20 "$out/prep.log" >&2; exit 1; }
  docker commit mcbn-s4-pane-prep "$image" >/dev/null
  docker rm mcbn-s4-pane-prep >/dev/null
fi

rm -rf "$out/run" "$out/saved" "$out/rt"
mkdir -p "$out/home" "$out/saved" "$out/rt"
status=0
docker run --rm --name mcbn-s4-pane-lock --cpus 4 --memory 6g --network none --security-opt no-new-privileges \
  --user "$uid:$gid" -e HOME=/out/home -e USER=pane -e LOGNAME=pane \
  -e MCBENCH_COMPOSITOR=sway -e MCBENCH_RUNTIME_ROOT=/out/rt -e MCBENCH_APP_ARGS="--save-to /out/saved" \
  -v "$here:/verify:ro" -v "$bin:/app/mcbench-ui-dev:ro" -v "$out:/out" \
  "$image" /verify/run.sh wayland wayland-lock /app/mcbench-ui-dev /out/run gate /verify/check_pane_lock.py /out/saved \
  >"$out/docker.log" 2>&1 || status=$?
echo "$(cat "$out/run/exit.txt" 2>/dev/null | tr '\n' ' ')| $(tail -1 "$out/run/check.log" 2>/dev/null)"
grep -E "^(ok|FAIL)" "$out/run/check.log" 2>/dev/null || true
exit "$status"
