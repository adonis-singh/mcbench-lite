#!/usr/bin/env bash
# The real app's 3D pane in a private session (pane agent): the
# `mcbench-lite` binary (real core, real renderer, the window) on a scratch
# data folder, a chat with a build opened, its first real frame waited for,
# then driven by real_pane.py (or MCBENCH_REAL_DRIVER, real_lever.py) with a
# whole-window screenshot after each step.
#
#   real_pane.sh <x11|wayland> <display number or socket name> <mcbench-lite> \
#                <data folder> <out dir> <chat id>
#
# The data folder is a scratch copy holding no settings.json and no
# auth-token; this script refuses one that holds either. It writes the
# folder's preferences.json so the app opens /chat/<chat id> beside the pane
# (the light theme off, every other field the default), or the place
# MCBENCH_REAL_PLACE names (`/bench/<id>`). The app logs to
# <out>/actions.jsonl through MCBENCH_UI_LOG (its renderer line, the pane's
# costs with each toolbar press). Nothing is downloaded by this script; a
# data folder whose setup is not done makes the app's setup fetch what it
# needs, so run it only on a folder setup has finished in.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
backend="$1"; name="$2"; bin="$3"; data="$4"; out="$5"; chat="$6"
for secret in settings.json auth-token; do
  if [ -e "$data/$secret" ]; then echo "real_pane.sh: $data holds $secret; use a copy without it" >&2; exit 2; fi
done
mkdir -p "$out"
out="$(cd "$out" && pwd)"
data="$(cd "$data" && pwd)"
python3 - "$data/preferences.json" "$chat" "${MCBENCH_REAL_PLACE:-}" "${MCBENCH_REAL_SIZE:-1280x820}" <<'PY'
import json, sys
json.dump({"light": False, "sidebarOpen": True, "sidebarWidth": 248.0, "chatWidth": None, "binds": {},
           "benchMode": False, "lastModel": None, "lastResolution": 1, "place": sys.argv[3] or "/chat/" + sys.argv[2],
           # At the screen's corner, as the dev binary's --origin 0,0: the
           # checks give the pointer window coordinates.
           "window": {"display": "00000000-0000-0000-0000-000000000000", "x": 0.0, "y": 0.0,
                      "width": float(sys.argv[4].split("x")[0]), "height": float(sys.argv[4].split("x")[1])}, "openBenches": [], "promptsOpen": True, "night": False,
           "benchModels": [], "benchBlind": True, "benchResolution": 1, "promptAiModel": None,
           "improveSource": "text"}, open(sys.argv[1], "w"))
PY
inner="$out/inner.sh"
cat >"$inner" <<INNER
#!/usr/bin/env bash
set -u
[ -n "\${WAYLAND_SOCKET_NAME:-}" ] && [ -z "\${WAYLAND_DISPLAY:-}" ] && export WAYLAND_DISPLAY="\$(ls "\$XDG_RUNTIME_DIR" | grep '^wayland-[0-9]*\$' | head -1)"
export MCBENCH_APP_NAME=mcbench-lite MCBENCH_OUT="$out" MCBENCH_BACKEND="$backend" MCBENCH_DATA_DIR="$data" MCBENCH_UI_LOG="$out/actions.jsonl"
export TMPDIR="$out/tmp"; mkdir -p "\$TMPDIR"
# The driver's choice (real_quit.py), carried past session.sh's empty environment.
export MCBENCH_QUIT="${MCBENCH_QUIT:-}"
export MCBENCH_KEY_EVERY="${MCBENCH_KEY_EVERY:-}"
# The window's size in pixels of the window's scale (MCBENCH_REAL_SIZE, in
# preferences.json above), its X11 scale, and the numbers' length.
${GPUI_X11_SCALE_FACTOR:+export GPUI_X11_SCALE_FACTOR="$GPUI_X11_SCALE_FACTOR"}
export MCBENCH_REAL_NUMBERS="${MCBENCH_REAL_NUMBERS:-}"
# The hardware hidden for the software floor (VK_ICD_FILENAMES naming no
# driver, glvnd's EGL vendors Mesa's alone), carried the same way when set.
${VK_ICD_FILENAMES:+export VK_ICD_FILENAMES="$VK_ICD_FILENAMES" VK_DRIVER_FILES="$VK_ICD_FILENAMES"}
${__EGL_VENDOR_LIBRARY_FILENAMES:+export __EGL_VENDOR_LIBRARY_FILENAMES="$__EGL_VENDOR_LIBRARY_FILENAMES"}
[ "$backend" = wayland ] && python3 "$here/wl_pointer.py" output ${MCBENCH_REAL_SIZE:-1280x820}
"$bin" >"$out/app.log" 2>&1 &
export MCBENCH_APP_PID=\$!
python3 "$here/${MCBENCH_REAL_DRIVER:-real_pane.py}" >"$out/check.log" 2>"$out/check.err"
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
