#!/usr/bin/env bash
# Start the app in the background; Minecraft is managed by the app itself.
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$DIR"
export PORT="${PORT:-3020}"
URL="http://127.0.0.1:$PORT"
mkdir -p data
LOG="$DIR/data/mcb-server.log"

# Any answer means the server is up (without its token the answer is a 401).
up() { [ "$(curl -s -o /dev/null --max-time 2 -w '%{http_code}' "$URL/api/config")" != "000" ]; }

if up; then
  echo "mcbench already up: $(npm run --silent url)"
  exit 0
fi

setsid nohup node server/index.js > "$LOG" 2>&1 < /dev/null &
pid=$!

for ((attempt = 0; attempt < 90; attempt++)); do
  if up; then
    echo "mcbench up: $(npm run --silent url)"
    echo "Log: $LOG"
    exit 0
  fi
  if ! kill -0 "$pid" 2>/dev/null; then break; fi
  sleep 1
done

echo "mcbench did not become ready; log: $LOG" >&2
tail -20 "$LOG" >&2
exit 1
