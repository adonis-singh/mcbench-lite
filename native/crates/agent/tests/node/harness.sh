#!/bin/bash
# mcbench-agent's harness differential: the cases of harness_cases.py
# through today's chat route (harness.js) and through mcbench_agent's Turns
# (tests/harness_diff.rs), each against mcbench-providers' fake CLI, each in
# an environment of only the variables below. The Rust side compares.
#
#   harness.sh <scratch dir> <checkout with server/ and node_modules> <rust test binary (deps/harness_diff-*)>
#
# Needs node and python3. Writes only under <scratch dir>. Starts no
# container: Codex runs on the host launcher on both sides.
set -euo pipefail
dir=$(mkdir -p "$1" && cd "$1" && pwd)
checkout=$(cd "$2" && pwd)
exe=$3
here=$(cd "$(dirname "$0")" && pwd)
node=$(node -e "process.stdout.write(process.execPath)")
python3 "$here/harness_cases.py" > "$dir/cases.json"
for side in node rust; do
  /bin/rm -rf "$dir/$side"
  mkdir -p "$dir/$side/home" "$dir/$side/tmp" "$dir/$side/codex-login"
  claims=$(printf '{"email":"person@example.com","https://api.openai.com/auth":{"chatgpt_plan_type":"pro"}}' | base64 | tr -d '=\n' | tr '/+' '_-')
  printf '{"tokens":{"id_token":"e30.%s.sig"}}' "$claims" > "$dir/$side/codex-login/auth.json"
done
env_for() {
  echo env -i "HOME=$dir/$1/home" "TMPDIR=$dir/$1/tmp" "PATH=/usr/bin:/bin" "LANG=C.UTF-8" "USER=fake" "LC_ALL=C.UTF-8"
}
$(env_for node) "MCBENCH_NODE_DIFF=$checkout" "MCBENCH_DATA_DIR=$dir/node/data" "MCBENCH_SETTINGS_FILE=$dir/node/settings.json" "$node" -r "$here/det.js" "$here/harness.js" "$checkout" "$dir/cases.json" "$dir/node"
$(env_for node) "MCBENCH_NODE_DIFF=$checkout" "$node" "$here/dump.js" "$dir/node/data" "$dir/node/dump.json"
$(env_for rust) "MCBENCH_HARNESS_DIFF=$dir" "MCBENCH_NODE=$node" "MCBENCH_NODE_DIFF=$checkout" "$exe"
