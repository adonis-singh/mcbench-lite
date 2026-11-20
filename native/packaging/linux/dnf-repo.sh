#!/usr/bin/env bash
# The dnf repository's metadata for the site:
#
#   MCBENCH_LITE_SIGNING_KEY=<secret key> native/packaging/linux/dnf-repo.sh <release folder> <version> <site folder>
#
# writes <site>/rpm/repodata/ (replacing what was there) over the .rpm files
# in the release folder (mcbench-lite.x86_64.rpm and mcbench-lite.aarch64.rpm,
# whichever are there; at least one must be), with repomd.xml signed
# (repomd.xml.asc). The packages stay among the release's assets: the
# metadata names them under the release v<version> (createrepo_c
# --baseurl), so the site holds about 10 KB and a site deployed a little
# before or after the release never names a file that is not there. Every
# .rpm's repo file names MCBENCH_LITE_SITE/rpm/. Made with createrepo_c and
# gpg in FEDORA_IMAGE.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
[ $# -eq 3 ] || { echo "usage: native/packaging/linux/dnf-repo.sh <release folder> <version> <site folder>" >&2; exit 2; }
release=$(cd "$1" && pwd)
version=$2
mkdir -p "$3"
site=$(cd "$3" && pwd)
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version $version is not MAJOR.MINOR.PATCH" >&2; exit 1; }
signing_key
download=$(release_download "$version")
compgen -G "$release/mcbench-lite.*.rpm" >/dev/null || { echo "$release holds no mcbench-lite.<arch>.rpm: make it with native/packaging/linux/rpm.sh" >&2; exit 1; }
out=$site
new_work
cp "$release"/mcbench-lite.*.rpm "$work/"
in_image "$FEDORA_IMAGE" "
dnf -q -y install createrepo_c gnupg2 >/dev/null 2>&1
export GNUPGHOME=\$(mktemp -d)
gpg --batch --quiet --import /key.asc
mkdir /w/rpm
createrepo_c --quiet --general-compress-type gz --baseurl '$download' --outputdir /w/rpm /w
gpg --batch --yes --armor --detach-sign -o /w/rpm/repodata/repomd.xml.asc /w/rpm/repodata/repomd.xml
zcat /w/rpm/repodata/*-primary.xml.gz | grep -o '<location[^>]*>'" -v "$key:/key.asc:ro"
/bin/rm -rf "$site/rpm"
mv "$work/rpm" "$site/rpm"
echo "$site/rpm/repodata: $(du -sk "$site/rpm" | cut -f1) KB"
