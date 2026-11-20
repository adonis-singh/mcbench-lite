#!/usr/bin/env bash
# The in-app updater on one system for the script or AppImage route,
# against the staging releases serve.sh serves:
#
#   native/packaging/test/linux/update.sh <system> <route> <staging folder> <version A> <version B> <B's mcbench-lite sha256> <results folder>
#
# A and B are two real builds, both staged (stage.sh), and the refusal
# releases made by tamper.sh must be in the staging folder. Each case is a
# fresh container of the system's image on NET (Xvfb, xdotool and a
# desktop's libraries, as machine.sh's window test) with A installed by the
# page's command:
#
#   update    the feed offers A (nothing happens), then B: the download waits
#             beside the install, the next start swaps it in, the app runs B,
#             <name>.old goes; then the route's uninstall, and what is left
#   asset, json, older, otherkey
#             the feed offers a refusal release of tamper.sh: the app stays
#             A, nothing is left half-installed, update.log says why
#   hold      B waits beside the install while a headless engine of the
#             install runs: the next start does not swap
#
# Results in <results>/<system>-<route>-update/<case>.txt.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/env.sh"
[ $# -eq 7 ] || { echo "usage: update.sh <system> <route> <staging folder> <version A> <version B> <B's mcbench-lite sha256> <results folder>" >&2; exit 2; }
system=$1 route=$2 staging=$(cd "$3" && pwd) a=$4 b=$5 b_sum=$6
case $route in script | appimage) ;; *) echo "no route $route: the in-app updater serves script and appimage" >&2; exit 2 ;; esac
image=$(system_image "$system")
docker inspect "$SERVER" >/dev/null 2>&1 || { echo "$SERVER is not running: run serve.sh start $staging" >&2; exit 1; }
for tag in "v$a" "v$b" "v$b-asset" "v$b-json" v0.0.9-older "v$b-otherkey"; do
  [ -d "$staging/releases/download/$tag" ] || { echo "$staging/releases/download/$tag is missing: run stage.sh and tamper.sh" >&2; exit 1; }
done
out=$7/$system-$route-update
mkdir -p "$out"
out=$(cd "$out" && pwd)
a_sum=$(tar -xzOf "$staging/releases/download/v$a/mcbench-lite-linux-$(uname -m).tar.gz" mcbench-lite/mcbench-lite | sha256sum | cut -c1-64)

name() { echo "mcbn-s5-linux-$system-$route-$1-$$"; }
# Each case runs in a subshell, so the end stops every case's container by name.
current=
trap 'for k in update asset json older otherkey hold; do docker stop -t 2 "$(name $k)" >/dev/null 2>&1 || true; done' EXIT
fresh() {
  current=$(name "$1")
  docker run -d --rm --init --name "$current" --cpus 1 --network "$NET" --device /dev/fuse --cap-add SYS_ADMIN \
    --security-opt seccomp=unconfined --security-opt systempaths=unconfined \
    -e MCBENCH_LITE_RELEASES="$STAGING_RELEASES" -e MCBENCH_LITE_SITE="$STAGING_SITE" \
    -v "$here/inside.sh:/inside.sh:ro" -v "$here/update-inside.sh:/update-inside.sh:ro" -v "$out:/results" "$image" sleep infinity >/dev/null
  serve_version "$staging" "$a"
  docker exec "$current" bash /inside.sh prep "$route" harness
  docker exec "$current" bash /inside.sh install "$route"
}
done_with() { docker exec "$current" chown -R "$(id -u):$(id -g)" /results; docker stop -t 2 "$current" >/dev/null; current=; }
u() { docker exec "$current" bash /update-inside.sh "$1" "$route" "${@:2}"; }
offer() { echo "v$1" > "$staging/releases/LATEST"; echo "-- the feed offers $1"; }

{
  echo "== update: $system, $route, $a then $b"
  fresh update
  offer "$a"
  u start
  sleep 30
  u state "$a_sum"
  u quit
  offer "$b"
  u start
  u wait-update 300
  u quit
  u start
  u state "$b_sum"
  u old-gone 60
  u state "$b_sum"
  u quit
  docker exec "$current" bash /inside.sh uninstall "$route"
  docker exec "$current" bash -c "echo 'beside where the payload was:'; ls -A $( [ "$route" = script ] && echo '~u/.local/lib ~u/.local/bin' || echo '~u' ) 2>&1; echo 'the data folder logs:'; ls ~u/.local/share/mcbench-lite/logs"
  done_with
} 2>&1 | tee "$out/update.txt" || echo "the update case failed"

for case in asset json older otherkey; do
  tag=v$b-$case
  [ "$case" != older ] || tag=v0.0.9-older
  {
    echo "== $case: $system, $route, $a, the feed offers ${tag#v}"
    fresh "$case"
    offer "${tag#v}"
    u start
    u wait-log 240
    u quit
    u start
    u state "$a_sum"
    u quit
    done_with
  } 2>&1 | tee "$out/$case.txt" || echo "the $case case failed"
done

{
  echo "== hold: $system, $route, $b waiting while a headless engine of the install runs"
  fresh hold
  offer "$b"
  u start
  u wait-update 300
  u quit
  u hold
  u start
  u state "$a_sum"
  u quit
  u unhold
  done_with
} 2>&1 | tee "$out/hold.txt" || echo "the hold case failed"
