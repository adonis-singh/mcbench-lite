#!/bin/bash
# Copies files into the guest's /usr/local/rig over tart exec's stdin (files
# replaced in the shared folder were served stale to the guest).
# usage: install.sh <file>...
set -euo pipefail
. "$(dirname "$0")/env.sh"
gx sh -c 'sudo mkdir -p /usr/local/rig && sudo chown admin /usr/local/rig'
for f in "$@"; do
  gxi sh -c "cat > /usr/local/rig/$(basename "$f").new && chmod +x /usr/local/rig/$(basename "$f").new && mv /usr/local/rig/$(basename "$f").new /usr/local/rig/$(basename "$f")" < "$f"
done
