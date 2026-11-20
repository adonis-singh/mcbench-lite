#!/bin/bash
# Start the slot's VM (WINVM_SLOT) on its run overlay ($RUN, made by reset.sh). Extra args go to qemu.
source "$(dirname "$0")/env.sh"
[ -f "$RUN/disk.qcow2" ] || { echo "no run overlay: ./reset.sh <base> first" >&2; exit 1; }
t0=$SECONDS
vm_start "$RUN/disk.qcow2" "$RUN/vars.fd" "$RUN/tpm" "$@"
wait_ssh 600
echo "boot to ssh: $((SECONDS - t0))s"
# The guest-side helpers (gui.ps1, bench.ps1, ...) always come from this checkout.
scp -q $(ssh_opts | sed 's/-p /-P /') "$HERE"/guest/*.ps1 bench@127.0.0.1:C:/winvm/
