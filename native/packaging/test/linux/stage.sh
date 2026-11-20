#!/usr/bin/env bash
# Adds one version to a staging release folder, made as a real release is:
#
#   native/packaging/test/linux/stage.sh <payload folder> <version> <staging folder>
#
# with, in the environment, MCBENCH_LITE_SIGNING_KEY (the armoured staging
# OpenPGP secret key, for the .deb and .rpm), MCBENCH_SIGNING_KEY_FILE (the
# staging minisign secret key) and MCBENCH_REPO_KEY (the same OpenPGP key's
# id in GNUPGHOME) for release.sh, and mcbench-feed on PATH.
#
# The four Linux assets of <arch> (this machine's) are packaged from the
# payload into <staging>/work-v<version>/inputs with the staging server's
# addresses (env.sh), and native/packaging/release.sh --only linux-<arch>
# assembles them into <staging>/work-v<version>/out. Then the release's
# files go to <staging>/releases/download/v<version>/ and its site to
# <staging>/sites/v<version>/, which also gets aur/mcbench-lite-bin.git, a
# bare repository (git's dumb HTTP protocol, a stand-in for the AUR) holding
# one commit per version staged so far. serve_version (env.sh) makes a
# staged version the one served. Run it again with a higher version for the
# upgrade and update tests.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/env.sh"
[ $# -eq 3 ] || { echo "usage: native/packaging/test/linux/stage.sh <payload folder> <version> <staging folder>" >&2; exit 2; }
payload=$1 version=$2
mkdir -p "$3"
staging=$(cd "$3" && pwd)
export MCBENCH_LITE_RELEASES=$STAGING_RELEASES MCBENCH_LITE_SITE=$STAGING_SITE
packaging=$here/../..
arch=$(uname -m)
work=$staging/work-v$version
[ ! -e "$work" ] || { echo "$work exists: stage each version once" >&2; exit 1; }
mkdir -p "$work/inputs"

for step in archive deb rpm appimage; do "$packaging/linux/$step.sh" "$payload" "$version" "$arch" "$work/inputs"; done
"$packaging/release.sh" "$work/inputs" "$version" "$work/out" --base "${STAGING_RELEASES%/releases/latest/download}" \
  --site "$STAGING_SITE" --only "linux-$arch"

rel=$staging/releases/download/v$version site=$staging/sites/v$version
mkdir -p "$rel" "$site"
find "$work/out/release" -maxdepth 1 -type f -exec cp {} "$rel/" \;
cp -R "$work/out/site/." "$site/"

# The AUR stand-in: the previous version's repository, if any, and a commit.
previous=$(ls -d "$staging"/sites/v*/aur/mcbench-lite-bin.git 2>/dev/null | grep -v "/v$version/" | sort -V | tail -1 || true)
mkdir -p "$site/aur"
[ -z "$previous" ] || cp -R "$previous" "$site/aur/"
docker run --rm --name "mcbn-s5-linux-aurgit-$$" --cpus 1 -v "$staging:/s" "$ARCH_IMAGE" bash -euo pipefail -c "
trap 'chown -R $(id -u):$(id -g) /s/sites' EXIT
pacman -Sy --noconfirm --needed -q git >/dev/null 2>&1
git config --global user.name staging; git config --global user.email staging@mcbench-lite.invalid
git config --global init.defaultBranch master; git config --global --add safe.directory '*'
repo=/s/sites/v$version/aur/mcbench-lite-bin.git
[ -d \$repo ] || git init -q --bare \$repo
rm -rf /tmp/c && git clone -q \$repo /tmp/c 2>/dev/null
cp /s/work-v$version/out/release/aur/PKGBUILD /s/work-v$version/out/release/aur/.SRCINFO /tmp/c/
cd /tmp/c && git add PKGBUILD .SRCINFO && git commit -qm 'mcbench-lite-bin $version' && git push -q origin HEAD:master
git -C \$repo update-server-info"
ls -l "$rel"
