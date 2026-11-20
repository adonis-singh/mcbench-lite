#!/usr/bin/env bash
# One clean-machine test of one install route on one system, against the
# staging release serve.sh serves:
#
#   native/packaging/test/linux/machine.sh <system> <route> <staging folder> <version> [<upgrade version>] <results folder>
#
# <system> is one of env.sh's (debian-12, debian-13, ubuntu-24.04, fedora,
# arch); <route> apt (the Debian family), dnf (fedora), aur (arch), script
# or appimage (any); apt-bare is apt without recommended packages, a system
# with no fonts and no fontconfig. Both versions must be staged (stage.sh);
# the test serves each in turn (serve_version, env.sh).
#
# Two fresh containers of the system's image on NET, each removed at the end:
#
#   pure    the user u with sudo and only what the page's command needs; the
#           install command timed with the bytes the container received, and
#           every installed program's libraries resolved
#   full    the same plus Xvfb and xdotool; a listing of the file system; the
#           install; the first window through the user's command on a
#           private display (time to mapped, screenshot); the build child's
#           lookup by a headless build call; the upgrade to <upgrade
#           version> when given (apt and dnf); the removal; a second listing,
#           and the difference of the two
#
# Each container runs --init, so the app, which outlives the su that
# started it, is reaped when it quits. The full container runs with seccomp, the masked /proc paths and
# the host's AppArmor profile for containers lifted (--security-opt
# seccomp=unconfined, systempaths=unconfined, apparmor=unconfined) so
# bubblewrap can make its namespaces and mounts inside it, as it does on a
# machine; an AppImage's containers lift AppArmor too, for its FUSE mount.
# On a host that restricts unprivileged user namespaces through AppArmor
# (Ubuntu 23.10 and later), the profile the .deb installs is the container's,
# not loaded in the host's kernel: the caller lifts the restriction on the
# host (the release workflow's test-linux job does). Results in
# <results>/<system>-<route>/: pure.txt, full.txt, the logs, screen.png,
# before.list, after.list and leftovers.txt.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/env.sh"
case $# in
  5) system=$1 route=$2 staging=$3 version=$4 upgrade= results=$5 ;;
  6) system=$1 route=$2 staging=$3 version=$4 upgrade=$5 results=$6 ;;
  *) echo "usage: machine.sh <system> <route> <staging folder> <version> [<upgrade version>] <results folder>" >&2; exit 2 ;;
esac
image=$(system_image "$system")
staging=$(cd "$staging" && pwd)
for v in $version $upgrade; do
  [ -d "$staging/releases/download/v$v" ] || { echo "v$v is not staged in $staging: run stage.sh" >&2; exit 1; }
done
docker inspect "$SERVER" >/dev/null 2>&1 || { echo "$SERVER is not running: run serve.sh start $staging" >&2; exit 1; }
out=$results/$system-$route
mkdir -p "$out"
out=$(cd "$out" && pwd)
serve_version "$staging" "$version"

# The two containers' names; whatever is still running is stopped at the end.
name() { echo "mcbn-s5-linux-$system-$route-$1-$$"; }
trap 'for k in pure full; do docker stop -t 2 "$(name $k)" >/dev/null 2>&1 || true; done' EXIT
start() {
  local name
  name=$(name "$1")
  shift
  docker run -d --rm --init --name "$name" --cpus 1 --network "$NET" "$@" \
    -e MCBENCH_LITE_RELEASES="$STAGING_RELEASES" -e MCBENCH_LITE_SITE="$STAGING_SITE" \
    -v "$here/inside.sh:/inside.sh:ro" -v "$out:/results" "$image" sleep infinity >/dev/null
  echo "$name"
}
in_c() { local c=$1; shift; docker exec "$c" bash /inside.sh "$@"; }

fuse=()
[ "$route" != appimage ] || fuse=(--device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor=unconfined)

{
  echo "== pure: $system, $route, v$version"
  c=$(start pure "${fuse[@]}")
  in_c "$c" prep "$route"
  in_c "$c" install "$route"
  in_c "$c" libraries "$route"
  docker stop -t 2 "$c" >/dev/null
} 2>&1 | tee "$out/pure.txt"

{
  echo "== full: $system, $route, v$version${upgrade:+ then v$upgrade}"
  c=$(start full --security-opt seccomp=unconfined --security-opt systempaths=unconfined --security-opt apparmor=unconfined "${fuse[@]}")
  in_c "$c" prep "$route" harness
  in_c "$c" listing before
  in_c "$c" install "$route"
  in_c "$c" window "$route"
  in_c "$c" helpers "$route"
  if [ -n "$upgrade" ]; then
    serve_version "$staging" "$upgrade"
    in_c "$c" upgrade "$route"
  fi
  in_c "$c" uninstall "$route"
  in_c "$c" listing after
  docker exec "$c" chown -R "$(id -u):$(id -g)" /results
  docker stop -t 2 "$c" >/dev/null
  python3 -I "$here/xwd2png.py" "$out/screen.xwd" "$out/screen.png" && /bin/rm -f "$out/screen.xwd"
  echo "screenshot: $out/screen.png"
  diff "$out/before.list" "$out/after.list" > "$out/leftovers.txt" || true
  echo "after removal, $(grep -c '^>' "$out/leftovers.txt" || true) paths added and $(grep -c '^<' "$out/leftovers.txt" || true) removed (leftovers.txt)"
} 2>&1 | tee "$out/full.txt"
