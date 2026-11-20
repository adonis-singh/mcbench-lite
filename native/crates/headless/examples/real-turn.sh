#!/bin/sh
# ONE real turn through `mcbench-lite headless turn` with Claude Code
# (provider claude-code, model claude-haiku-5-5), for the stage-three
# session to run by hand; nothing runs it automatically.
#
#   native/crates/headless/examples/real-turn.sh <mcbench-lite> <tables folder> <blocks.json> <data folder> [prompt]
#
# <mcbench-lite>: a built binary (it is the engine children and the CLI's
# tool bridge too; the build child is MCBENCH_BUILD_CHILD or
# mcbench-build-child beside it). <tables folder>: Minecraft's tables
# (core/minecraft/1.21.10: blocks.txt, loot.json, datafix.txt).
# <blocks.json>: the block registry build programs are checked against
# (the shipped data's blocks.json), copied into the data folder as
# assets/1.21.10/blocks.json. <data folder>: a new, empty folder; it is
# kept, so its rows can be read afterwards (today's Node server reads it
# with mcbench-lite's tests/core_reads.rs), and its caller removes it.
# [prompt]: the user's text (a small build by default).
#
# What it needs on the machine:
#   - Claude Code's CLI, the program the app runs: MCBENCH_CLAUDE_BINARY when
#     set, else `claude` on PATH, else the tools folder's
#     (<data>/tools/claude-2.1.311/claude, or $MCBENCH_TOOLS_DIR's; the
#     pinned version is 2.1.311, `POST /api/cli-installs` installs it).
#   - Its sign-in, where the CLI keeps it: CLAUDE_CONFIG_DIR when set, else
#     the home folder (the account in .claude.json, the token in the CLI's
#     own store: ~/.claude/.credentials.json on Linux, the login keychain on
#     macOS). The app reads only whether .claude.json names an account; the
#     CLI reads its token itself.
#   - python3 (to print the stored transcript from the data folder's
#     database, read only).
set -eu
program=$1
tables=$2
blocks=$3
data=$4
prompt=${5:-Build a small 3 by 3 stone floor with one call to the build tool, then say done.}
[ ! -e "$data" ] || [ -z "$(ls -A "$data")" ] || { echo "real-turn.sh: $data is not empty" >&2; exit 2; }
mkdir -p "$data/assets/1.21.10"
cp "$blocks" "$data/assets/1.21.10/blocks.json"
export MCBENCH_TABLES="$tables"

created=$(printf '%s\n' "{\"id\":1,\"op\":\"create\",\"data\":\"$data\",\"provider\":\"claude-code\",\"model\":\"claude-haiku-5-5\"}" \
  | "$program" headless turn)
echo "create: $created"
chat=$(printf '%s' "$created" | python3 -c 'import json,sys; print(json.load(sys.stdin)["chat"])')

echo "--- events and answer"
request=$(python3 -c 'import json,sys; print(json.dumps({"id":2,"op":"turn","data":sys.argv[1],"chat":sys.argv[2],"prompt":sys.argv[3],"effort":None,"placement":"updates","networkAccess":False,"turnChat":True}))' "$data" "$chat" "$prompt")
printf '%s\n' "$request" | "$program" headless turn

echo "--- the stored transcript"
python3 - "$data/mcbench.sqlite3" "$chat" <<'EOF'
import sqlite3, sys
db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
for (payload,) in db.execute("SELECT payload_json FROM messages WHERE conversation_id = ? ORDER BY position", (sys.argv[2],)):
    print(payload)
EOF
echo "chat: $chat"
