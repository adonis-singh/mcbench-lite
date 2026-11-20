#!/bin/bash
# Shut Windows down cleanly over SSH (falls to ACPI power button if SSH is gone), wait, then make
# sure qemu and swtpm (by pid) are gone.
source "$(dirname "$0")/env.sh"
if vm_running; then
  pid=$(cat "$SLOTDIR/qemu.pid")
  ssh $(ssh_opts) bench@127.0.0.1 'shutdown /s /t 0' 2>/dev/null || qmp raw '{"execute":"system_powerdown"}'
  for i in $(seq 1 180); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
  if kill -0 "$pid" 2>/dev/null; then echo "qemu $pid did not stop in 180 s; killing" >&2; kill "$pid"; sleep 2; fi
  echo "VM stopped"
fi
if [ -f "$SLOTDIR/swtpm.pid" ]; then
  sp=$(cat "$SLOTDIR/swtpm.pid")
  if kill -0 "$sp" 2>/dev/null; then sleep 2; kill -0 "$sp" 2>/dev/null && kill "$sp"; fi
  rm -f "$SLOTDIR/swtpm.pid"
fi
rm -f "$SLOTDIR/qemu.pid"
