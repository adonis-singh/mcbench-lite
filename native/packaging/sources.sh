#!/bin/bash
# native/packaging/sources.sh [--check] LISTS LOCK OUT
#
# The source bundle of the bash sandbox's two Linux userlands, published in
# the same sandbox-assets-<hash> release as the assets: the guest base and
# the root filesystem, both architectures. LISTS is the lists/ folder and
# LOCK the release/sandbox-assets.lock that `sandbox-assets.sh build` writes
# (native/packaging/sandbox-assets.md), with the two root filesystems beside
# the lock (their copyright files give their packages' licences); OUT is made
# or reused.
#
#   OUT/debian/<source>_<version>/   each Debian source package: its .dsc and the files it names
#   OUT/MANIFEST.tsv                 every package of the lists and every part of sources/other.tsv: source, files, sha256, licence
#   OUT/README.md                    what the bundle is, the release and assets it belongs to, how it was made
#
# Sources come from snapshot.debian.org, one request at a time with a pause
# between requests; a file already in OUT with the right sha256 is not
# fetched again, so an interrupted run is continued by running it again.
# Every file is checked against the sha256 its .dsc names. The run ends with
# the completeness check, which fails unless MANIFEST.tsv names exactly the
# packages of the lists, every file a row names is in OUT with its sha256,
# and every file in OUT is in a row. --check runs only that check (no
# network). pasta's source is not in OUT: it is passt-<version>.tar.gz in the
# same release.
#
# Needs: python3 (3.11 or later), zstd, the internet.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
exec python3 -I "$HERE/sources/sources.py" "$@"
