#!/usr/bin/env bash
# Runs one check against the dev binary in a private session (session.sh).
#
#   run.sh <x11|wayland> <display number or socket name> <mcbench-ui-dev> <out dir> \
#          <fixture> <check.py> [check arguments...]
#
# The binary starts with `--fixture <fixture> --size 1280x820 --origin 0,0
# --log <out>/actions.jsonl --a11y-dump <out>/a11y.json` (no dump with
# MCBENCH_DUMP=off); the check runs
# with MCBENCH_APP_PID, MCBENCH_OUT and MCBENCH_BACKEND set, then the
# binary is asked to quit (SIGTERM if it is still running 5 s later) and
# its exit status goes to <out>/exit.txt.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
backend="$1"; name="$2"; bin="$3"; out="$4"; fixture="$5"; check="$6"; shift 6
mkdir -p "$out"
out="$(cd "$out" && pwd)"
inner="$out/inner.sh"
# MCBENCH_APP_ARGS adds arguments (--gallery). MCBENCH_DUMP=off leaves the tree dump out (the dump reads the whole tree
# every 100 ms on the interface thread).
# MCBENCH_PLACE=first opens the window with no --size or --origin, where
# the app places a first window (check_first_window.py).
size="${MCBENCH_SIZE:-1280x820}"
place="--size $size --origin 0,0"
[ "${MCBENCH_PLACE:-given}" = first ] && place=""
dump="--a11y-dump $out/a11y.json"
[ "${MCBENCH_DUMP:-on}" = off ] && dump=""
cat >"$inner" <<INNER
#!/usr/bin/env bash
set -u
[ -n "\${WAYLAND_SOCKET_NAME:-}" ] && [ -z "\${WAYLAND_DISPLAY:-}" ] && export WAYLAND_DISPLAY="\$(ls "\$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*\$' | head -1)"
export MCBENCH_OUT="$out" MCBENCH_BACKEND="$backend"
# pane agent: session.sh starts from an empty environment (env -i), so a
# scale factor the caller asks for (GPUI_X11_SCALE_FACTOR) is carried here.
export GPUI_X11_SCALE_FACTOR="${GPUI_X11_SCALE_FACTOR:-}"
# pane agent: real_numbers.py's length.
export MCBENCH_REAL_NUMBERS="${MCBENCH_REAL_NUMBERS:-}"
# The dev binary's scratch (its picture, crash logs) goes under the run's
# folder, never the machine's /tmp.
export TMPDIR="$out/tmp"; mkdir -p "\$TMPDIR"
# cage draws its one client at the output's size: make it the window size.
[ "$backend" = wayland ] && python3 "$here/wl_pointer.py" output $size
"$bin" --fixture "$fixture" $place --scratch "$out/dev" --log "$out/actions.jsonl" $dump ${MCBENCH_APP_ARGS:-} >"$out/app.log" 2>&1 &
export MCBENCH_APP_PID=\$!
python3 "$check" $(printf '%q ' "$@") >"$out/check.log" 2>"$out/check.err"
status=\$?
if kill -0 \$MCBENCH_APP_PID 2>/dev/null; then
  for _ in \$(seq 50); do kill -0 \$MCBENCH_APP_PID 2>/dev/null || break; sleep 0.1; done
  kill \$MCBENCH_APP_PID 2>/dev/null
fi
wait \$MCBENCH_APP_PID; echo "app exit \$?" >"$out/exit.txt"
echo "check exit \$status" >>"$out/exit.txt"
exit \$status
INNER
chmod +x "$inner"
"$here/session.sh" "$backend" "$name" "$out/session" -- "$inner"
