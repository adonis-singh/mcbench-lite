#!/bin/bash
# native/crates/sandbox/rootfs/build.sh OUT [amd64|arm64]
#
# The bash sandbox's root filesystem as a pinned release asset, from
# sandbox/Dockerfile (today's image), for one architecture (default amd64):
#
#   OUT/mcbench-sandbox-rootfs-<arch>.tar.zst   image.json, pasta (and pasta.avx2 on amd64), rootfs/
#   OUT/mcbench-sandbox-rootfs-<arch>.sha256    its sha256
#   OUT/passt-<version>.tar.gz                  pasta's source, offered beside it (GPL-2.0-or-later)
#   OUT/docker-names.txt                        every Docker object this script made (kind, name, id)
#
# and prints the sha256 and the size. Docker is this script's tool only (a
# release-time step); the app never needs it. Steps:
#
#   1. docker buildx build of sandbox/Dockerfile for the architecture, its
#      FROM given as the pinned image index below (both architectures from
#      one index, so they hold the same Python), as
#      <prefix>rootfs:<the first 12 hex digits of the Dockerfile's sha256>[-arch],
#      then docker export of a container of it named <prefix>export-<pid>.
#   2. pasta (passt), static, built from the pinned source tarball in a
#      container of the pinned debian:13 index for the architecture named
#      <prefix>pasta-<pid>, with 2 CPUs
#      (under qemu for the other one). On amd64 passt's `make static` also
#      builds passt.avx2, which pasta execs as its own path with `.avx2`
#      added when the processor has AVX2 (passt's arch.c; without it pasta
#      logs "Can't run AVX2 build, using non-AVX2 version"): it is shipped
#      beside pasta as pasta.avx2.
#   3. normalize.py: one tar sorted by name, owned by 0:0, under rootfs/, with
#      /.dockerenv and device nodes left out, hard links kept, mtimes kept
#      (Python's .pyc files are checked against them), image.json, pasta
#      and pasta.avx2 beside rootfs/; then zstd -19.
#
# Docker discipline (the DOCKER RULE of stage three): every name this script
# makes starts with the caller's prefix, MCBENCH_DOCKER_PREFIX (default
# mcbn-s3-, stage three's; another session passes its own, of the form
# mcbn-<letters and digits>-, and any other form is refused), never
# localhost/mcbench-sandbox or any mcb- or mcbench- name (the app's own). Before it builds or names anything it lists
# what exists under that exact name and refuses unless it is an object this
# script recorded in OUT/docker-names.txt. When it ends, in every exit path,
# it removes exactly the objects it made, by id. It never prunes and never
# pulls by tag: the two base images are pulled, when missing, by the image
# index digests pinned below, which names no tag and so cannot move one.
# What sandbox/Dockerfile installs (pip's NumPy and Pillow, apt's packages)
# is not pinned there: the two architectures match when they are built from
# the same base on the same day, which the release pipeline does.
#
# Needs: docker (with binfmt qemu for a foreign architecture), curl or the
# tarball in OUT, python3, zstd, sha256sum.
set -euo pipefail
OUT=$(mkdir -p "${1:?usage: build.sh OUT [amd64|arm64]}" && cd "$1" && pwd)
ARCH=${2:-amd64}
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
RECORD="$OUT/docker-names.txt"
touch "$RECORD"

# pasta: passt's tag 2026_10_02.cba3570. passt.top publishes no checksum for
# its snapshot tarballs; this pins the sha256 measured on 2026-11-17 (the
# netspike's build-pasta.sh).
PASST=2026_10_02.cba3570
PASST_SHA256=7b81892b1070521ed217de672cdce43d7a37436e6f850cd04f2b63919ac484b6

HASH=$(sha256sum "$REPO/sandbox/Dockerfile" | cut -c1-64)
PREFIX="${MCBENCH_DOCKER_PREFIX:-mcbn-s3-}"
case "$PREFIX" in
  mcbn-*-) [[ "$PREFIX" =~ ^mcbn-[a-z0-9]+-$ ]] || { echo "rootfs/build.sh: MCBENCH_DOCKER_PREFIX is '$PREFIX', not of the form mcbn-<letters and digits>-" >&2; exit 2; } ;;
  *) echo "rootfs/build.sh: MCBENCH_DOCKER_PREFIX is '$PREFIX', not of the form mcbn-<letters and digits>-" >&2; exit 2 ;;
esac
IMAGE="${PREFIX}rootfs:${HASH:0:12}"
[ "$ARCH" = amd64 ] || IMAGE="$IMAGE-$ARCH"
EXPORT="${PREFIX}export-$$"
PASTA="${PREFIX}pasta-$$"
WORK=$(mktemp -d "$OUT/.build.XXXXXX")

# Ids made by this run, removed at exit (containers first, then the image).
MADE_CONTAINERS=()
MADE_IMAGE=""
cleanup() {
  for id in "${MADE_CONTAINERS[@]}"; do docker container rm -f "$id" > /dev/null 2>&1 || true; done
  [ -z "$MADE_IMAGE" ] || docker image rm "$MADE_IMAGE" > /dev/null 2>&1 || echo "[rootfs] could not remove image $MADE_IMAGE; remove it by this id" >&2
  /bin/rm -rf "$WORK"
}
trap cleanup EXIT

refuse() { echo "build.sh: $*" >&2; exit 1; }
recorded() { grep -qxF "$1" "$RECORD"; }

# An existing object under a name this script uses must be one it recorded.
guard_image() {
  local id
  id=$(docker image ls --no-trunc --quiet "$1" | head -1)
  [ -z "$id" ] || recorded "image $1 $id" || refuse "an image named $1 exists ($id) that this script did not make (not in $RECORD); it is not touched"
}
guard_container() {
  local id
  id=$(docker container ls --all --no-trunc --quiet --filter "name=^/$1\$" | head -1)
  [ -z "$id" ] || recorded "container $1 $id" || refuse "a container named $1 exists ($id) that this script did not make; it is not touched"
}

# The base images, by index digest (docker.io's python:3.12-slim and
# debian:13 on 2026-11-17; the first is what the amd64 rootfs 786d552a...
# was built from).
FROM=$(sed -n 's/^FROM[[:space:]]\+\([^[:space:]]\+\).*/\1/p' "$REPO/sandbox/Dockerfile" | head -1)
[ "$FROM" = docker.io/library/python:3.12-slim ] || refuse "sandbox/Dockerfile's FROM is $FROM; pin its index here"
PYTHON_BASE=docker.io/library/python@sha256:5e2bdb1b0d542d5039cffb8181c65930093949194a65c365dfb1b0fe13a4fa5b
DEBIAN_BASE=docker.io/library/debian@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6

echo "[rootfs] image $IMAGE (linux/$ARCH)" >&2
guard_image "$IMAGE"
docker buildx build --load --platform "linux/$ARCH" --build-context "$FROM=docker-image://$PYTHON_BASE" -t "$IMAGE" "$REPO/sandbox" >&2
MADE_IMAGE=$(docker image ls --no-trunc --quiet "$IMAGE" | head -1)
echo "image $IMAGE $MADE_IMAGE" >> "$RECORD"

guard_container "$EXPORT"
cid=$(docker create --platform "linux/$ARCH" --name "$EXPORT" "$IMAGE")
MADE_CONTAINERS+=("$cid")
echo "container $EXPORT $cid" >> "$RECORD"
docker export "$cid" > "$WORK/export.tar"
docker image inspect "$IMAGE" --format '{{json .Config.Env}}' > "$WORK/env.json"
CREATED=$(date -d "$(docker image inspect "$IMAGE" --format "{{.Created}}")" +%s)

echo "[rootfs] pasta $PASST (linux/$ARCH)" >&2
TARBALL="$OUT/passt-$PASST.tar.gz"
[ -f "$TARBALL" ] || curl -sSfL -o "$TARBALL" "https://passt.top/passt/snapshot/passt-$PASST.tar.gz"
echo "$PASST_SHA256  $TARBALL" | sha256sum -c - >&2
guard_container "$PASTA"
# --rm removes it when it ends; its id (--cidfile) is recorded and removed
# again at exit in case it was interrupted.
docker run --rm -i --pull missing --cpus 2 --platform "linux/$ARCH" --name "$PASTA" --cidfile "$WORK/pasta.cid" "$DEBIAN_BASE" sh -c "
  set -e
  apt-get update -qq >&2 && apt-get install -y -qq --no-install-recommends gcc make libc6-dev >&2
  mkdir /src && cd /src && tar xz --strip-components=1
  make -j2 static >&2
  tar -c passt \$(test ! -f passt.avx2 || echo passt.avx2)" < "$TARBALL" > "$WORK/pasta.tar" &
run=$!
for _ in $(seq 1 100); do [ -s "$WORK/pasta.cid" ] && break; sleep 0.1; done
if [ -s "$WORK/pasta.cid" ]; then
  MADE_CONTAINERS+=("$(cat "$WORK/pasta.cid")")
  echo "container $PASTA $(cat "$WORK/pasta.cid")" >> "$RECORD"
fi
wait "$run"
tar -xf "$WORK/pasta.tar" -C "$WORK"
mv "$WORK/passt" "$WORK/pasta"
chmod 0755 "$WORK/pasta"
AVX2=""
if [ "$ARCH" = amd64 ]; then
  [ -f "$WORK/passt.avx2" ] || refuse "passt's make static built no passt.avx2 for amd64"
  mv "$WORK/passt.avx2" "$WORK/pasta.avx2"
  chmod 0755 "$WORK/pasta.avx2"
  AVX2="$WORK/pasta.avx2"
fi

python3 -I "$HERE/normalize.py" "$WORK/export.tar" "$WORK/env.json" "$WORK/pasta" "$IMAGE" "$HASH" "$PASST" "$CREATED" "$AVX2" > "$WORK/rootfs.tar"
ASSET="$OUT/mcbench-sandbox-rootfs-$ARCH.tar.zst"
zstd -q -19 -T4 -f "$WORK/rootfs.tar" -o "$ASSET"
SHA=$(sha256sum "$ASSET" | cut -c1-64)
echo "$SHA" > "$OUT/mcbench-sandbox-rootfs-$ARCH.sha256"
echo "sha256 $SHA"
echo "size $(stat -c %s "$ASSET") bytes ($(stat -c %s "$WORK/rootfs.tar") unpacked as tar)"
