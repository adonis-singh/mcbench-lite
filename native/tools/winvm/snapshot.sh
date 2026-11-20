#!/bin/bash
# snapshot.sh NAME: turn the stopped run overlay into a new base (backed by the run's base).
source "$(dirname "$0")/env.sh"
n=${1:?usage: snapshot.sh NAME}
vm_running && { echo "VM running: ./stop.sh first" >&2; exit 1; }
[ -f "$BASES/$n.qcow2" ] && { echo "base $n exists" >&2; exit 1; }
mv "$RUN/disk.qcow2" "$BASES/$n.qcow2"
qemu-img rebase -u -b "$BASES/$(cat "$RUN/base").qcow2" -F qcow2 "$BASES/$n.qcow2"
cp "$RUN/vars.fd" "$BASES/$n.vars.fd"; cp -r "$RUN/tpm" "$BASES/$n.tpm"
rm -rf "$RUN"
qemu-img info --backing-chain "$BASES/$n.qcow2" | grep -E "^(image|backing file|disk size)"
