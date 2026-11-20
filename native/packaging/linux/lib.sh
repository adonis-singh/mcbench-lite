# Sourced by the Linux packaging scripts after pins.sh: the arguments every
# one of them takes and the layout every package shares (the payload folder
# and what lies beside it, docs/native-app.md, Plan of the stage-five session).

# packaging_args <script> "$@": sets payload, version, arch and out from
# <payload folder> <version> <arch> <out folder>, and repo, here and icons.
packaging_args() {
  local script=$1; shift
  [ $# -eq 4 ] || { echo "usage: $script <payload folder> <version> <arch> <out folder>" >&2; exit 2; }
  payload=$(cd "$1" && pwd)
  version=$2
  arch=$3
  mkdir -p "$4"
  out=$(cd "$4" && pwd)
  [[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version $version is not MAJOR.MINOR.PATCH" >&2; exit 1; }
  deb_arch "$arch" >/dev/null
  local f
  for f in mcbench-lite mcbench-build-child mcbench-sandbox-agent; do
    [ -x "$payload/$f" ] || { echo "$payload has no program $f: make the payload folder with native/packaging/build/linux.sh" >&2; exit 1; }
  done
  if [ "$arch" = x86_64 ]; then
    [ -f "$payload/libvk_swiftshader.so" ] && [ -d "$payload/licenses/swiftshader" ] || { echo "$payload has no libvk_swiftshader.so and licenses/swiftshader: make the payload folder with native/packaging/build/linux.sh" >&2; exit 1; }
  fi
  here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  repo=$(cd "$here/../../.." && pwd)
  icons=$repo/native/packaging/icon
}

# install_payload <folder> <route>: the payload folder's three programs, on
# x86_64 the software Vulkan driver libvk_swiftshader.so and its licences in
# licenses/swiftshader/ (build/swiftshader.sh), install-source holding
# <route>, and version holding $version (the app compares it with its
# compiled-in version; the updater with the release's).
install_payload() {
  install -d -m 755 "$1"
  install -m 755 "$payload/mcbench-lite" "$payload/mcbench-build-child" "$payload/mcbench-sandbox-agent" "$1/"
  if [ "$arch" = x86_64 ]; then
    install -m 644 "$payload/libvk_swiftshader.so" "$1/"
    install -d -m 755 "$1/licenses/swiftshader"
    install -m 644 "$payload/licenses/swiftshader/"* "$1/licenses/swiftshader/"
  fi
  printf '%s\n' "$2" > "$1/install-source"
  printf '%s\n' "$version" > "$1/version"
  chmod 644 "$1/install-source" "$1/version"
}

# install_share <share folder>: the desktop entry, the AppStream metainfo
# and the hicolor icons.
install_share() {
  install -D -m 644 "$here/mcbench-lite.desktop" "$1/applications/mcbench-lite.desktop"
  install -D -m 644 "$here/io.github.adonis_singh.mcbench_lite.metainfo.xml" "$1/metainfo/io.github.adonis_singh.mcbench_lite.metainfo.xml"
  local size
  for size in 16 24 32 48 64 128 256 512; do
    install -D -m 644 "$icons/mcbench-lite-$size.png" "$1/icons/hicolor/${size}x${size}/apps/mcbench-lite.png"
  done
  install -D -m 644 "$icons/mcbench-lite.svg" "$1/icons/hicolor/scalable/apps/mcbench-lite.svg"
}

# install_system <root> <route>: the layout of the apt, dnf and aur routes
# under <root>: /usr/lib/mcbench-lite, the /usr/bin symlink, /usr/share.
install_system() {
  install_payload "$1/usr/lib/mcbench-lite" "$2"
  install -d -m 755 "$1/usr/bin"
  ln -s ../lib/mcbench-lite/mcbench-lite "$1/usr/bin/mcbench-lite"
  install_share "$1/usr/share"
}

# signing_key: fails unless MCBENCH_LITE_SIGNING_KEY names the armoured
# OpenPGP secret key that signs the repositories and the RPMs.
signing_key() {
  [ -n "${MCBENCH_LITE_SIGNING_KEY:-}" ] || { echo "MCBENCH_LITE_SIGNING_KEY is not set: name the armoured OpenPGP secret key that signs the repositories" >&2; exit 1; }
  [ -f "$MCBENCH_LITE_SIGNING_KEY" ] || { echo "MCBENCH_LITE_SIGNING_KEY names $MCBENCH_LITE_SIGNING_KEY, which is not a file" >&2; exit 1; }
  key=$(cd "$(dirname "$MCBENCH_LITE_SIGNING_KEY")" && pwd)/$(basename "$MCBENCH_LITE_SIGNING_KEY")
}

# new_work: a fresh work folder inside <out>, removed when the script ends.
new_work() {
  work=$(mktemp -d "$out/.work.XXXXXX")
  trap '/bin/rm -rf "$work"' EXIT
}

# in_image <image> <script> [docker arguments]: runs <script> with bash in
# <image> as root, one CPU, in a container named mcbn-s5-linux-<script
# name>-<pid>, with /w the work folder $work, whose files are given back to
# the caller's user when it ends, failed or not.
in_image() {
  local image=$1 script=$2; shift 2
  docker run --rm --name "mcbn-s5-linux-$(basename "$0" .sh)-$$" --cpus 1 -v "$work:/w" -e SOURCE_DATE_EPOCH "$@" "$image" bash -euo pipefail -c \
    "trap 'chown -R $(id -u):$(id -g) /w' EXIT; export DEBIAN_FRONTEND=noninteractive; $script"
}

# A reproducible time for every file a package holds, and for tar and gzip;
# modes that do not depend on the caller's umask.
export SOURCE_DATE_EPOCH=0
umask 022

# The address of the apt repository, which every .deb's source file names:
# the release assets themselves (a flat repository among them).
apt_address() { echo "$MCBENCH_LITE_RELEASES/"; }

# The address of the dnf repository's metadata (repodata/ on the site), and
# where its packages are: the assets of the release v<version>, read from
# MCBENCH_LITE_RELEASES, which must end in /releases/latest/download.
dnf_address() { echo "$MCBENCH_LITE_SITE/rpm/"; }
release_download() {
  case $MCBENCH_LITE_RELEASES in
    */releases/latest/download) echo "${MCBENCH_LITE_RELEASES%/latest/download}/download/v$1/" ;;
    *) echo "MCBENCH_LITE_RELEASES ($MCBENCH_LITE_RELEASES) does not end in /releases/latest/download" >&2; exit 1 ;;
  esac
}
