#!/usr/bin/env bash
# The RPM package:
#
#   MCBENCH_LITE_SIGNING_KEY=<secret key> native/packaging/linux/rpm.sh <payload folder> <version> <arch> <out folder>
#
# writes <out>/mcbench-lite.<arch>.rpm, signed with the key: the payload
# folder in /usr/lib/mcbench-lite with install-source "dnf" and version,
# /usr/bin/mcbench-lite, the desktop entry, metainfo and icons, the licence,
# and the dnf repository's file (%config(noreplace)) with the key's public
# part, so later versions arrive with the system's updates. Made with
# rpmbuild and rpmsign in FEDORA_IMAGE. The programs are packed as built (no
# strip, no debuginfo); rpm finds the libraries they link itself (the
# payload folder's software Vulkan driver is private, so it provides nothing
# to other packages), and the
# spec adds what they load at run time, bubblewrap, and hicolor-icon-theme,
# which owns the icon folders; fontconfig's library, which the app loads only
# when present, is a weak dependency. The repository's
# address is MCBENCH_LITE_SITE (pins.sh).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
packaging_args native/packaging/linux/rpm.sh "$@"
signing_key
new_work

root=$work/root
install_system "$root" dnf
install -D -m 644 "$repo/LICENSE" "$root/usr/share/licenses/mcbench-lite/LICENSE"
install -d -m 755 "$root/etc/yum.repos.d" "$root/etc/pki/rpm-gpg"
cat > "$root/etc/yum.repos.d/mcbench-lite.repo" <<EOF
[mcbench-lite]
name=mcbench-lite
baseurl=$(dnf_address)
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///etc/pki/rpm-gpg/RPM-GPG-KEY-mcbench-lite
EOF
cat > "$work/mcbench-lite.spec" <<EOF
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none
%global __provides_exclude_from ^/usr/lib/mcbench-lite/
Name: mcbench-lite
Version: $version
Release: 1
Summary: AI models build in Minecraft
License: MIT
URL: https://adonis-singh.github.io/mcbench-lite/
Requires: libglvnd-egl, libwayland-client, libwayland-egl, vulkan-loader, bubblewrap, hicolor-icon-theme
Recommends: fontconfig
%description
mcbench-lite is a desktop app in which AI models build in Minecraft 1.21.10:
a model writes build programs, tests its redstone in Minecraft's own engine
and looks at pictures of what it built.
%install
cp -a /w/root/. %{buildroot}/
%files
/usr/lib/mcbench-lite
/usr/bin/mcbench-lite
/usr/share/applications/mcbench-lite.desktop
/usr/share/metainfo/io.github.adonis_singh.mcbench_lite.metainfo.xml
/usr/share/icons/hicolor/*/apps/mcbench-lite.*
%dir /usr/share/licenses/mcbench-lite
%license /usr/share/licenses/mcbench-lite/LICENSE
%config(noreplace) /etc/yum.repos.d/mcbench-lite.repo
/etc/pki/rpm-gpg/RPM-GPG-KEY-mcbench-lite
EOF
name=mcbench-lite.$arch.rpm
in_image "$FEDORA_IMAGE" "
dnf -q -y install rpm-build rpm-sign gnupg2 >/dev/null 2>&1
export GNUPGHOME=\$(mktemp -d)
gpg --batch --quiet --import /key.asc
gpg --batch --armor --export > /w/root/etc/pki/rpm-gpg/RPM-GPG-KEY-mcbench-lite
chmod 644 /w/root/etc/pki/rpm-gpg/RPM-GPG-KEY-mcbench-lite
fpr=\$(gpg --batch --list-secret-keys --with-colons | awk -F: '/^fpr/ {print \$10; exit}')
rpmbuild -bb --quiet --target $arch --define '_topdir /tmp/rpm' --define '_buildhost reproducible' \
  --define 'use_source_date_epoch_as_buildtime 1' --define 'clamp_mtime_to_source_date_epoch 1' /w/mcbench-lite.spec
f=\$(ls /tmp/rpm/RPMS/$arch/mcbench-lite-$version-1.*.rpm)
GPG_TTY= rpmsign --addsign --define \"_openpgp_sign_id \$fpr\" --define \"_gpg_name \$fpr\" \$f >/dev/null
cp \$f /w/$name
rpm -qp --qf 'Name: %{NAME}\nVersion: %{VERSION}-%{RELEASE}\nArch: %{ARCH}\nSignature: %{RSAHEADER:pgpsig}%{OPENPGP:pgpsig}\n' /w/$name 2>/dev/null | sed 's/(none)//g'
echo Requires: \$(rpm -qp --requires /w/$name | grep -v '^rpmlib' | tr '\n' ' ')" \
  -v "$key:/key.asc:ro"
mv "$work/$name" "$out/$name"
echo "$out/$name: $(stat -c %s "$out/$name") bytes"
