#!/bin/bash
# native/packaging/release.sh: assembles a release from what the build and
# package steps made (native/packaging/README.md). Nothing is uploaded.
#
#   native/packaging/release.sh <inputs> <version> <out> [--base <address> --site <address>]
#                               [--chain <latest.json>] [--only <platform>...]
#
# <inputs> is a folder holding the assets of the table below under their
# fixed names, anywhere beneath it (each platform's packaging step writes
# into its own folder); each name must be found exactly once. <version> is
# MAJOR.MINOR.PATCH. <out> must not exist. It becomes
#
#   <out>/release/           every asset; the flat apt repository (Packages,
#                            Release, InRelease, Release.gpg); latest.json;
#                            SHA256SUMS over every other file in release/
#   <out>/release/aur/       PKGBUILD and .SRCINFO of mcbench-lite-bin
#   <out>/release/winget/    the winget manifest's three files
#   <out>/site/              native/site/ with the sources link filled in from
#                            native/sandbox-assets.lock and the release and
#                            site addresses set to --base's and --site's, and
#                            the dnf repository's metadata in rpm/repodata/
#
# The files directly in release/ are the release's assets; its subfolders
# are not (the AUR and winget files are published by the owner's own steps).
#
# --base is the address of the repository the release is served from: the
# fixed links are <address>/releases/latest/download/<name> and the
# versioned ones <address>/releases/download/v<version>/<name>, as GitHub
# serves them. --site is the address the site is served from. The defaults
# are the public repository and its GitHub Pages address; --base and --site
# are given together or not at all. A staging release is the same call with
# local addresses and staging keys (native/packaging/test/linux/server.py
# answers in GitHub's shape). The Linux packages in <inputs> name these
# addresses too, so they are made with the same MCBENCH_LITE_RELEASES
# (<base>/releases/latest/download) and MCBENCH_LITE_SITE (<site>).
#
# --chain names the previous release's latest.json after a key rotation:
# its key chain is copied into the new latest.json (mcbench-feed sign
# --chain; native/crates/updater/LATEST.md, Rotating the key).
#
# --only assembles a partial release of the named platforms, for a local
# test of one platform; latest.json then names only their targets.
#
# The environment:
#   MCBENCH_SIGNING_KEY_FILE  the updater's minisign secret key file, for
#                             mcbench-feed sign (always)
#   MCBENCH_REPO_KEY          the OpenPGP key id, in the caller's GnuPG home,
#                             that signs the apt and dnf repositories (when a
#                             Linux platform is in the release); its secret
#                             key must have no passphrase
# and on PATH: mcbench-feed (cargo build -p mcbench-updater --features feed
# --bin mcbench-feed), and, for a Linux platform, gpg and docker (the Linux
# scripts run their tools in pinned images).
set -euo pipefail
die() { echo "release.sh: $*" >&2; exit 1; }

# The assets a release requires: platform, file name, and the updater's
# target name for the assets latest.json names (- for the others).
ASSETS="
macos-aarch64    mcbench-lite-macos-aarch64.tar.gz     macos-aarch64
macos-x86_64     mcbench-lite-macos-x86_64.tar.gz      macos-x86_64
linux-x86_64     mcbench-lite-linux-x86_64.tar.gz      linux-x86_64
linux-x86_64     mcbench-lite-x86_64.AppImage          linux-x86_64-appimage
linux-x86_64     mcbench-lite_amd64.deb                -
linux-x86_64     mcbench-lite.x86_64.rpm               -
linux-aarch64    mcbench-lite-linux-aarch64.tar.gz     linux-aarch64
linux-aarch64    mcbench-lite-aarch64.AppImage         linux-aarch64-appimage
linux-aarch64    mcbench-lite_arm64.deb                -
linux-aarch64    mcbench-lite.aarch64.rpm              -
windows-x86_64   mcbench-lite-setup-x64.exe            -
windows-x86_64   mcbench-lite-windows-x86_64.tar.gz    windows-x86_64
"
PUBLIC=https://github.com/adonis-singh/mcbench-lite
PUBLIC_SITE=https://adonis-singh.github.io/mcbench-lite
USAGE="usage: native/packaging/release.sh <inputs> <version> <out> [--base <address> --site <address>] [--chain <latest.json>] [--only <platform>...]"

[ $# -ge 3 ] || { echo "$USAGE" >&2; exit 2; }
inputs=$1 version=$2 out=$3
shift 3
base=$PUBLIC site_address=$PUBLIC_SITE chain=()
platforms=$(echo "$ASSETS" | awk 'NF { print $1 }' | uniq | tr '\n' ' ')
while [ $# -gt 0 ]; do
  case "$1" in
    --base) [ $# -ge 2 ] || { echo "$USAGE" >&2; exit 2; }; base=${2%/}; shift 2 ;;
    --site) [ $# -ge 2 ] || { echo "$USAGE" >&2; exit 2; }; site_address=${2%/}; shift 2 ;;
    --chain) [ $# -ge 2 ] || { echo "$USAGE" >&2; exit 2; }; [ -f "$2" ] || die "--chain names $2, which is not a file"; chain=(--chain "$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"); shift 2 ;;
    --only)
      shift
      only=""
      while [ $# -gt 0 ] && [ "${1#--}" = "$1" ]; do
        case " $platforms " in *" $1 "*) ;; *) die "no platform $1: the platforms are $platforms" ;; esac
        only="$only $1"
        shift
      done
      [ -n "$only" ] || die "--only names no platform"
      platforms="${only# } "
      ;;
    *) echo "$USAGE" >&2; exit 2 ;;
  esac
done

public_base=0 public_site=0
[ "$base" != "$PUBLIC" ] || public_base=1
[ "$site_address" != "$PUBLIC_SITE" ] || public_site=1
[ "$public_base" = "$public_site" ] || die "--base and --site are given together: a staging release names its own address for both"
[[ $version =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || die "the version $version is not MAJOR.MINOR.PATCH"
[ -d "$inputs" ] || die "the inputs folder $inputs does not exist"
[ ! -e "$out" ] || die "$out exists; a release is assembled into a new folder"
packaging=$(cd "$(dirname "$0")" && pwd)
native=$(cd "$packaging/.." && pwd)
inputs=$(cd "$inputs" && pwd)
has() { case " $platforms " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }
linux=""
for arch in x86_64 aarch64; do has "linux-$arch" && linux="$linux $arch"; done
linux=${linux# }

sha() { if command -v sha256sum > /dev/null; then sha256sum "$1" | cut -c1-64; else shasum -a 256 "$1" | cut -c1-64; fi; }
size() { wc -c < "$1" | tr -d ' '; }

[ -n "${MCBENCH_SIGNING_KEY_FILE:-}" ] || die "MCBENCH_SIGNING_KEY_FILE is not set: name the updater's minisign secret key file"
[ -f "$MCBENCH_SIGNING_KEY_FILE" ] || die "MCBENCH_SIGNING_KEY_FILE names $MCBENCH_SIGNING_KEY_FILE, which is not a file"
command -v mcbench-feed > /dev/null || die "mcbench-feed is not on PATH: build it with cargo build -p mcbench-updater --features feed --bin mcbench-feed"
if [ -n "$linux" ]; then
  [ -n "${MCBENCH_REPO_KEY:-}" ] || die "MCBENCH_REPO_KEY is not set: name the OpenPGP key id that signs the apt and dnf repositories"
  command -v gpg > /dev/null || die "gpg is not on PATH, and the Linux repositories are signed with MCBENCH_REPO_KEY through it"
  command -v docker > /dev/null || die "docker is not on PATH, and the Linux scripts run their tools in pinned images"
fi

# The sources link: the release page of the sandbox assets, whose release
# also holds the source bundle. While the lock's url is not-published-yet
# (a staging release, or a trial run of the release workflow) that word is
# the link; the release workflow's version job refuses it for a release.
lock_url=$(awk -F'\t' '$1 == "url" { print $2 }' "$native/sandbox-assets.lock")
case "$lock_url" in
  not-published-yet) sources=$lock_url ;;
  */releases/download/*) sources="${lock_url%/releases/download/*}/releases/tag/${lock_url##*/releases/download/}" ;;
  *) die "native/sandbox-assets.lock's url $lock_url is not a releases/download/<name> address" ;;
esac

# Every asset of the chosen platforms, found once under <inputs>.
missing="" found=""
while read -r platform name target; do
  if [ -z "$platform" ] || ! has "$platform"; then continue; fi
  paths=$(find "$inputs" -name "$name" -type f)
  count=$(printf '%s' "$paths" | grep -c . || true)
  case "$count" in
    1) found="$found$name	$paths	$target"$'\n' ;;
    0) missing="$missing $name" ;;
    *) die "$name is in $inputs $count times: $(echo "$paths" | tr '\n' ' ')" ;;
  esac
done <<< "$ASSETS"
[ -z "$missing" ] || die "$inputs lacks the assets$missing"

release=$out/release site=$out/site
mkdir -p "$release" "$site"
out=$(cd "$out" && pwd) release=$out/release site=$out/site
targets=""
while IFS=$'\t' read -r name path target; do
  [ -n "$name" ] || continue
  cp "$path" "$release/$name"
  [ "$target" = - ] || targets="$targets $target"
done <<< "$found"
echo "[release] $(printf '%s' "$found" | grep -c .) assets for ${platforms% } from $inputs"

export MCBENCH_LITE_RELEASES=$base/releases/latest/download MCBENCH_LITE_SITE=$site_address
if [ -n "$linux" ]; then
  keys=$(mktemp -d)
  trap '/bin/rm -rf "$keys"' EXIT
  chmod 700 "$keys"
  gpg --batch --armor --export-secret-keys "$MCBENCH_REPO_KEY" > "$keys/repo.asc"
  [ -s "$keys/repo.asc" ] || die "the GnuPG home has no secret key $MCBENCH_REPO_KEY"
  export MCBENCH_LITE_SIGNING_KEY=$keys/repo.asc
  echo "[release] the apt repository"
  "$packaging/linux/apt-repo.sh" "$release"
  echo "[release] the AUR package for $linux"
  # shellcheck disable=SC2086 # one argument per architecture
  "$packaging/linux/aur.sh" "$release" "$version" "$release/aur" $linux
fi
if has windows-x86_64; then
  echo "[release] the winget manifest"
  "$packaging/windows/winget.sh" "$release" "$version" "$release/winget"
fi

echo "[release] latest.json for$targets"
# shellcheck disable=SC2086 # one argument per target
mcbench-feed sign ${chain[@]+"${chain[@]}"} "$release" "$version" $targets

(cd "$release" && for file in *; do if [ -f "$file" ]; then echo "$(sha "$file")  $file"; fi; done) > "$out/SHA256SUMS"
mv "$out/SHA256SUMS" "$release/SHA256SUMS"

cp -R "$native/site/." "$site/"
# The site's links to the release and to itself, for a staging release
# (the public release's addresses are the ones native/site/ holds).
for file in "$site"/*.html "$site"/install.sh "$site"/install.ps1; do
  awk -v releases="$MCBENCH_LITE_RELEASES" -v pages="$site_address" '
    { gsub("https://github.com/adonis-singh/mcbench-lite/releases/latest/download", releases)
      gsub("https://adonis-singh.github.io/mcbench-lite", pages) }
    { print }' "$file" > "$out/site-file"
  cat "$out/site-file" > "$file"
done
/bin/rm -f "$out/site-file"
[ "$(grep -c 'href="<sources address>"' "$site/index.html")" = 1 ] || die "native/site/index.html does not hold the sources link's marked href exactly once"
awk -v href="$sources" '{ gsub(/href="<sources address>"/, "href=\"" href "\"") } { print }' "$site/index.html" > "$out/index.html"
mv "$out/index.html" "$site/index.html"
if [ -n "$linux" ]; then
  echo "[release] the dnf repository's metadata"
  "$packaging/linux/dnf-repo.sh" "$release" "$version" "$site"
fi

echo "[release] v$version in $out, served from $MCBENCH_LITE_RELEASES and $base/releases/download/v$version, the site from $site_address; sources link $sources"
(cd "$out" && find release site -type f | LC_ALL=C sort | while read -r file; do printf '%12s  %s\n' "$(size "$file")" "$file"; done)
