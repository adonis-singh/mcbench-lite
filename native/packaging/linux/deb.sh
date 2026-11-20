#!/usr/bin/env bash
# The Debian package:
#
#   MCBENCH_LITE_SIGNING_KEY=<secret key> native/packaging/linux/deb.sh <payload folder> <version> <arch> <out folder>
#
# writes <out>/mcbench-lite_<amd64|arm64>.deb: the payload folder in
# /usr/lib/mcbench-lite with install-source "apt" and version, /usr/bin/mcbench-lite, the
# desktop entry, metainfo and icons, the licence as
# /usr/share/doc/mcbench-lite/copyright, and the apt repository's source file
# (a conffile; `apt purge` removes it) with the signing key's public part as
# its keyring, so later versions arrive with the system's updates, and the
# AppArmor profile of apparmor/ with the postinst and prerm that install,
# load and remove it on Ubuntu 24.04 and later. Made with
# dpkg-deb --root-owner-group in DEBIAN_IMAGE, every time SOURCE_DATE_EPOCH.
# The repository's address is MCBENCH_LITE_RELEASES (pins.sh).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
packaging_args native/packaging/linux/deb.sh "$@"
signing_key
new_work
debarch=$(deb_arch "$arch")

# What the programs need (README.md, The programs' libraries): the
# libraries they link, the ones they load at run time (EGL, Vulkan,
# wayland's client and EGL), and bubblewrap for both sandboxes. The app
# embeds its fonts and loads fontconfig's library only when it is there (for
# the system's fonts), so that library is a recommendation.
depends='libc6 (>= 2.28), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libegl1, libwayland-client0, libwayland-egl1, libvulkan1, bubblewrap'
recommends='libfontconfig1'

root=$work/root
install_system "$root" apt
install -D -m 644 "$repo/LICENSE" "$root/usr/share/doc/mcbench-lite/copyright"
install -d -m 755 "$root/etc/apt/sources.list.d" "$root/usr/share/keyrings" "$root/DEBIAN"
cat > "$root/etc/apt/sources.list.d/mcbench-lite.sources" <<EOF
Types: deb
URIs: $(apt_address)
Suites: ./
Signed-By: /usr/share/keyrings/mcbench-lite.gpg
EOF
echo /etc/apt/sources.list.d/mcbench-lite.sources > "$root/DEBIAN/conffiles"
# The AppArmor profile Ubuntu 24.04 and later need for the bash sandbox
# (apparmor/mcbench-lite says why), which postinst installs and loads only
# where the kernel restricts unprivileged user namespaces, and prerm removes.
install -D -m 644 "$here/apparmor/mcbench-lite" "$root/usr/share/mcbench-lite/apparmor/mcbench-lite"
install -m 755 "$here/apparmor/postinst" "$here/apparmor/prerm" "$root/DEBIAN/"
(cd "$root" && find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums)
cat > "$root/DEBIAN/control" <<EOF
Package: mcbench-lite
Version: $version
Architecture: $debarch
Maintainer: mcbench-lite <releases@mcbench-lite.invalid>
Installed-Size: $(du -sk --exclude=DEBIAN "$root" | cut -f1)
Depends: $depends
Recommends: $recommends
Section: games
Priority: optional
Homepage: https://adonis-singh.github.io/mcbench-lite/
Description: AI models build in Minecraft
 mcbench-lite is a desktop app in which AI models build in Minecraft
 1.21.10: a model writes build programs, tests its redstone in Minecraft's
 own engine and looks at pictures of what it built.
EOF
chmod 644 "$root/DEBIAN/control" "$root/DEBIAN/conffiles" "$root/DEBIAN/md5sums"
name=mcbench-lite_$debarch.deb
in_image "$DEBIAN_IMAGE" "
apt-get -qq update && apt-get -qq install -y --no-install-recommends gnupg >/dev/null
export GNUPGHOME=\$(mktemp -d)
gpg --batch --quiet --import /key.asc
gpg --batch --export > /w/root/usr/share/keyrings/mcbench-lite.gpg
chmod 644 /w/root/usr/share/keyrings/mcbench-lite.gpg
(cd /w/root && md5sum usr/share/keyrings/mcbench-lite.gpg >> DEBIAN/md5sums && LC_ALL=C sort -k2 -o DEBIAN/md5sums DEBIAN/md5sums)
dpkg-deb --root-owner-group -Zxz --build /w/root /w/$name >/dev/null
dpkg-deb --info /w/$name | sed -n 's/^ \(Package\|Version\|Architecture\|Installed-Size\): /\1: /p'" \
  -v "$key:/key.asc:ro"
mv "$work/$name" "$out/$name"
echo "$out/$name: $(stat -c %s "$out/$name") bytes"
