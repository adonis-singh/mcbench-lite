#!/usr/bin/env bash
# The flat apt repository among a release's assets:
#
#   MCBENCH_LITE_SIGNING_KEY=<secret key> native/packaging/linux/apt-repo.sh <release folder>
#
# writes Packages, Release, InRelease (clear-signed) and Release.gpg into the
# release folder, over the .deb files in it (mcbench-lite_amd64.deb and
# mcbench-lite_arm64.deb, whichever are there; at least one must be).
# Every .deb's source file names MCBENCH_LITE_RELEASES/ with "Suites: ./", so
# apt reads these files from the latest release and fetches the package
# named in Packages from beside them. Made with apt-ftparchive and gpg in
# DEBIAN_IMAGE.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
[ $# -eq 1 ] || { echo "usage: native/packaging/linux/apt-repo.sh <release folder>" >&2; exit 2; }
release=$(cd "$1" && pwd)
signing_key
compgen -G "$release/mcbench-lite_*.deb" >/dev/null || { echo "$release holds no mcbench-lite_<arch>.deb: make it with native/packaging/linux/deb.sh" >&2; exit 1; }
work=$release
in_image "$DEBIAN_IMAGE" "
apt-get -qq update && apt-get -qq install -y --no-install-recommends apt-utils gnupg >/dev/null
export GNUPGHOME=\$(mktemp -d)
gpg --batch --quiet --import /key.asc
cd /w
apt-ftparchive packages . > Packages
apt-ftparchive -o APT::FTPArchive::Release::Origin=mcbench-lite -o APT::FTPArchive::Release::Label=mcbench-lite release . > Release
gpg --batch --yes --clearsign -o InRelease Release
gpg --batch --yes --detach-sign -o Release.gpg Release
sed -n 's/^\(Package\|Version\|Architecture\|Filename\): /\1: /p' Packages" -v "$key:/key.asc:ro"
