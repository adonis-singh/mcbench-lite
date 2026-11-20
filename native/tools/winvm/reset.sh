#!/bin/bash
# reset.sh BASE: throw away the run overlay and start fresh from bases/BASE (installed | tools).
source "$(dirname "$0")/env.sh"
b=${1:?usage: reset.sh installed|tools}
[ -f "$BASES/$b.qcow2" ] || { echo "no base $b" >&2; exit 1; }
vm_running && { echo "VM running: ./stop.sh first" >&2; exit 1; }
t0=$(date +%s%N)
rm -rf "$RUN"; mkdir -p "$RUN"
qemu-img create -q -f qcow2 -b "$BASES/$b.qcow2" -F qcow2 "$RUN/disk.qcow2"
cp "$BASES/$b.vars.fd" "$RUN/vars.fd"
cp -r "$BASES/$b.tpm" "$RUN/tpm"
echo "$b" > "$RUN/base"
echo "reset to $b in $(( ($(date +%s%N) - t0) / 1000000 )) ms"
