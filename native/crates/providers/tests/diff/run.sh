#!/bin/bash
# The CLI harnesses' differential: today's Node modules and the Rust
# providers run every case of cases.py against the same fake CLI, each in
# an environment made from nothing but the variables below, and
# compare.py requires the same output (README in compare.py).
#
#   run.sh <scratch dir> <rust test binary (target/debug/deps/cli-*)> [case filter]
#
# Needs node and python3. Writes only under <scratch dir>.
set -euo pipefail
dir=$(cd "$1" && pwd)
exe=$2
filter=${3:-}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../../../.." && pwd)
node=$(node -e "process.stdout.write(process.execPath)")
python3 "$here/cases.py" > "$dir/cases.json"
mkdir -p "$dir/codex-login"
claims=$(printf '{"email":"person@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro"}}' | base64 | tr -d '=\n' | tr '/+' '_-')
printf '{"tokens":{"id_token":"e30.%s.sig"}}' "$claims" > "$dir/codex-login/auth.json"
for side in node rust; do
  /bin/rm -rf "$dir/$side"
  mkdir -p "$dir/$side/home" "$dir/$side/tmp"
done
# MCBENCH_DIFF_SANDBOXED=1 runs Codex boxed on both sides (today's Docker
# container, the Rust box): the user session's bus for systemd-run and the
# sandbox's files go through, which the CLIs' allow-list never passes on.
env_for() {
  local boxed=""
  if [ "${MCBENCH_DIFF_SANDBOXED:-}" = 1 ]; then
    boxed="MCBENCH_DIFF_SANDBOXED=1 XDG_RUNTIME_DIR=$XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS MCBENCH_SANDBOX_ROOTFS=$MCBENCH_SANDBOX_ROOTFS MCBENCH_SANDBOX_ROOTFS_SHA256=$MCBENCH_SANDBOX_ROOTFS_SHA256 MCBENCH_SANDBOX_AGENT_STATIC=$MCBENCH_SANDBOX_AGENT_STATIC"
  fi
  echo env -i "HOME=$dir/$1/home" "TMPDIR=$dir/$1/tmp" "PATH=/usr/bin:/bin" "LANG=C.UTF-8" "USER=fake" "LC_ALL=C.UTF-8" $boxed
}
# Boxed, the Node side runs today's Codex container, whose names are fixed in
# server/codex-cli.js and server/sandbox.js: never on the owner's machines
# (RESULTS.md beside this file keeps the last results).
if [ "${MCBENCH_DIFF_SANDBOXED:-}" = 1 ] && [ "${MCBENCH_I_OWN_THIS_DOCKER:-}" != 1 ]; then
  echo "run.sh refused with MCBENCH_DIFF_SANDBOXED=1: it runs today's server/codex-cli.js container (named mcbench-codex-..., from the image localhost/mcbench-sandbox:<hash>), names that cannot be changed without editing server/; it is for a machine that is not the owner's (CI): set MCBENCH_I_OWN_THIS_DOCKER=1 there" >&2
  exit 2
fi
names=$(python3 -c 'import json,sys; [print(c["name"]) for c in json.load(open(sys.argv[1]))]' "$dir/cases.json")
start=$(date +%s)
for name in $names; do
  if [ -n "$filter" ] && [[ "$name" != *"$filter"* ]]; then continue; fi
  $(env_for node) "$node" "$here/node-driver.js" "$repo" "$dir/cases.json" "$name" "$dir/node" "$dir/codex-login" || echo "node: $name failed"
done
echo "node side: $(( $(date +%s) - start )) s"; start=$(date +%s)
$(env_for rust) "MCBENCH_DIFF_HARNESS=$dir" "$exe" $filter > "$dir/rust.log" 2>&1 || { echo "rust side failed"; tail -20 "$dir/rust.log"; }
echo "rust side: $(( $(date +%s) - start )) s"
python3 "$here/compare.py" "$dir" "$repo" "$exe" "$filter"
