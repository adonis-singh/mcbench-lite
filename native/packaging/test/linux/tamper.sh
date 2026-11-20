#!/usr/bin/env bash
# The refusal releases of the update test, beside the staged ones:
#
#   MCBENCH_SIGNING_KEY_FILE=<staging minisign.key> native/packaging/test/linux/tamper.sh <staging folder> <version A> <version B>
#
# with mcbench-feed on PATH. Each is a copy of a staged release under a tag
# of its own in <staging>/releases/download/:
#
#   v<B>-asset      B with one byte of the Linux archive and the AppImage changed (same names)
#   v<B>-json       B with latest.json's sha256 of both Linux targets changed
#   v0.0.9-older    A's assets with latest.json signed by the staging key for version 0.0.9
#   v<B>-otherkey   B with latest.json signed by a new key, not the one compiled in
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
[ $# -eq 3 ] || { echo "usage: tamper.sh <staging folder> <version A> <version B>" >&2; exit 2; }
staging=$(cd "$1" && pwd) a=$2 b=$3
[ -n "${MCBENCH_SIGNING_KEY_FILE:-}" ] || { echo "MCBENCH_SIGNING_KEY_FILE is not set: name the staging minisign secret key" >&2; exit 1; }
command -v mcbench-feed >/dev/null || { echo "mcbench-feed is not on PATH" >&2; exit 1; }
rel=$staging/releases/download
arch=$(uname -m)
targets="linux-$arch linux-$arch-appimage"
for tag in "v$b-asset" "v$b-json" v0.0.9-older "v$b-otherkey"; do
  [ ! -e "$rel/$tag" ] || { echo "$rel/$tag exists" >&2; exit 1; }
done

# One byte changed in the middle of each file, its name kept.
flip() { local size=$(stat -c %s "$1"); local at=$((size / 2)); local byte=$(od -An -tu1 -j "$at" -N1 "$1" | tr -d ' ');
  printf "$(printf '\\%03o' $(( (byte + 1) % 256 )))" | dd of="$1" bs=1 seek="$at" conv=notrunc status=none; }
cp -R "$rel/v$b" "$rel/v$b-asset"
flip "$rel/v$b-asset/mcbench-lite-linux-$arch.tar.gz"
flip "$rel/v$b-asset/mcbench-lite-$arch.AppImage"

cp -R "$rel/v$b" "$rel/v$b-json"
python3 -I - "$rel/v$b-json/latest.json" <<'EOF'
import json, sys
path = sys.argv[1]
feed = json.load(open(path))
for entry in feed["assets"].values():
    entry["sha256"] = "0" * 64
open(path, "w").write(json.dumps(feed, indent=2) + "\n")
EOF

cp -R "$rel/v$a" "$rel/v0.0.9-older"
rm "$rel/v0.0.9-older/latest.json"
# shellcheck disable=SC2086 # one argument per target
mcbench-feed sign "$rel/v0.0.9-older" 0.0.9 $targets

other=$(mktemp -d)
trap '/bin/rm -rf "$other"' EXIT
mcbench-feed keygen "$other" >/dev/null
cp -R "$rel/v$b" "$rel/v$b-otherkey"
rm "$rel/v$b-otherkey/latest.json"
# shellcheck disable=SC2086
MCBENCH_SIGNING_KEY_FILE=$other/minisign.key mcbench-feed sign "$rel/v$b-otherkey" "$b" $targets
for tag in "v$b-asset" "v$b-json" v0.0.9-older "v$b-otherkey"; do echo "$rel/$tag: $(python3 -I -c 'import json,sys; f=json.load(open(sys.argv[1])); print(f["version"], {k: v["sha256"][:12] for k, v in f["assets"].items()})' "$rel/$tag/latest.json")"; done
