#!/usr/bin/env bash
# The Linux archive that install.sh and the in-app updater of the script
# route read:
#
#   native/packaging/linux/archive.sh <payload folder> <version> <arch> <out folder>
#
# writes <out>/mcbench-lite-linux-<arch>.tar.gz holding one folder,
# mcbench-lite/: the payload folder's three programs, install-source holding
# "script", version holding <version>, and share/ (the desktop entry, the metainfo, the hicolor icons,
# the licence in licenses/mcbench-lite/),
# which install.sh places under ${XDG_DATA_HOME:-~/.local/share}. Made with
# GNU tar and gzip in DEBIAN_IMAGE, sorted, every time 0, owner and group 0
# and no name or time in gzip's header, so one payload gives one archive.
# The updater's `mcbench-feed pack` replaces this script when it lands.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
. "$here/pins.sh"
. "$here/lib.sh"
packaging_args native/packaging/linux/archive.sh "$@"
new_work

install_payload "$work/root/mcbench-lite" script
install_share "$work/root/mcbench-lite/share"
install -D -m 644 "$repo/LICENSE" "$work/root/mcbench-lite/share/licenses/mcbench-lite/LICENSE"
name=mcbench-lite-linux-$arch.tar.gz
in_image "$DEBIAN_IMAGE" "cd /w/root && tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --format=gnu -cf - mcbench-lite | gzip -9 -n > /w/$name"
mv "$work/$name" "$out/$name"
echo "$out/$name: $(stat -c %s "$out/$name") bytes"
