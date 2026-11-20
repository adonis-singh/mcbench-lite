# Sourced by the clean-machine tests: the Docker names they own, the
# staging addresses, and the systems they run on, by digest.
. "$(dirname "${BASH_SOURCE[0]}")/../../linux/pins.sh"

NET=mcbn-s5-linux-net
SERVER=mcbn-s5-linux-serve
SERVER_IMAGE=python:3.13-slim@sha256:e48f59cdba69a3cbd9ed2cf0f348d90d45a8d350d98134cf1070bc106ee9ed90
STAGING_RELEASES=http://$SERVER:8000/releases/latest/download
STAGING_SITE=http://$SERVER:8001

# serve_version <staging folder> <version>: the server answers latest/download
# with that version's release and serves its site.
serve_version() {
  [ -d "$1/releases/download/v$2" ] && [ -d "$1/sites/v$2" ] || { echo "v$2 is not staged in $1: run stage.sh" >&2; exit 1; }
  echo "v$2" > "$1/releases/LATEST"
  ln -sfn "sites/v$2" "$1/site"
}

# system_image <system>: the image of each system the tests run on.
system_image() {
  case $1 in
    debian-12) echo "$DEBIAN_IMAGE" ;;
    debian-13) echo debian:13@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6 ;;
    ubuntu-24.04) echo ubuntu:24.04@sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8 ;;
    fedora) echo "$FEDORA_IMAGE" ;;
    arch) echo "$ARCH_IMAGE" ;;
    *) echo "no system $1: debian-12, debian-13, ubuntu-24.04, fedora or arch" >&2; exit 1 ;;
  esac
}
