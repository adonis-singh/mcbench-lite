#!/usr/bin/env bash
# Stop only app servers belonging to this checkout, including npm start.
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd -P)"

is_our_server() {
  local pid="$1"
  local -a args=()
  [[ -O "/proc/$pid" ]] || return 1
  [[ "$(readlink "/proc/$pid/cwd" 2>/dev/null)" == "$DIR" ]] || return 1
  mapfile -d '' -t args 2>/dev/null < "/proc/$pid/cmdline" || return 1
  ((${#args[@]} >= 2)) || return 1
  [[ "${args[0]##*/}" == node ]] || return 1
  [[ "${args[1]:-}" == server/index.js || "${args[1]:-}" == ./server/index.js || "${args[1]:-}" == "$DIR/server/index.js" ]]
}

pids=()
for proc in /proc/[0-9]*; do
  pid="${proc##*/}"
  if is_our_server "$pid"; then pids+=("$pid"); fi
done

if ((${#pids[@]} == 0)); then
  echo "mcbench not running"
  exit 0
fi

for pid in "${pids[@]}"; do
  if is_our_server "$pid"; then kill -TERM "$pid" 2>/dev/null || true; fi
done

# SIGTERM lets the app close its database and stop Minecraft and verification.
for ((attempt = 0; attempt < 90; attempt++)); do
  remaining=()
  for pid in "${pids[@]}"; do
    if is_our_server "$pid"; then remaining+=("$pid"); fi
  done
  if ((${#remaining[@]} == 0)); then
    echo "mcbench stopped"
    exit 0
  fi
  pids=("${remaining[@]}")
  sleep 1
done

echo "mcbench is still shutting down (PID ${pids[*]}); inspect its log before forcing a stop" >&2
exit 1
