#!/usr/bin/env bash
# One run of the dev binary inside a distros.sh container: a private Xvfb,
# the binary on the gate fixture, its frame buffer after SETTLE seconds, its
# window's WM_CLASS, whether it is still up, then SIGTERM. Results in /out:
# facts.txt (what the system has), app.log, actions.jsonl, screen.xwd,
# wm_class.txt, result.txt.
set -u
out=/out
mkdir -p "$HOME" /tmp/rt /tmp/fb /tmp/dev
chmod 700 /tmp/rt
. /etc/os-release
{
  echo "system: $PRETTY_NAME"
  echo "font files: $(find /usr/share/fonts /usr/local/share/fonts -type f 2>/dev/null | wc -l)"
  echo "font families: $(find /usr/share/fonts -type f \( -name '*.ttf' -o -name '*.otf' -o -name '*.ttc' \) 2>/dev/null | sed 's|.*/||' | sort | tr '\n' ' ')"
  echo "libfontconfig: $(ls /usr/lib*/libfontconfig.so.1 /usr/lib/*/libfontconfig.so.1 2>/dev/null | head -1)"
  echo "fontconfig configuration: $(ls /etc/fonts/fonts.conf 2>/dev/null)"
  echo "fc-match: $(command -v fc-match)"
  echo "Vulkan drivers: $(ls /usr/share/vulkan/icd.d 2>/dev/null | tr '\n' ' ')"
  echo "environment: VK_ICD_FILENAMES=${VK_ICD_FILENAMES:-} __EGL_VENDOR_LIBRARY_FILENAMES=${__EGL_VENDOR_LIBRARY_FILENAMES:-}"
  echo "missing libraries: $(ldd /app/mcbench-ui-dev | grep -c 'not found')"
  ldd /app/mcbench-ui-dev | grep 'not found'
} >"$out/facts.txt" 2>&1
Xvfb :96 -screen 0 1280x820x24 -nolisten tcp -fbdir /tmp/fb >"$out/xvfb.log" 2>&1 &
xvfb=$!
for _ in $(seq 100); do [ -S /tmp/.X11-unix/X96 ] && break; sleep 0.1; done
export DISPLAY=:96 XDG_RUNTIME_DIR=/tmp/rt
start=$(date +%s%N)
/app/mcbench-ui-dev --fixture gate --open /new --size 1280x820 --origin 0,0 --scratch /tmp/dev \
  --log "$out/actions.jsonl" >"$out/app.log" 2>&1 &
app=$!
# Up to 60 s for the renderer line, failing at once when the app ends.
ended=""
for _ in $(seq 600); do
  grep -q '"action":"renderer"' "$out/actions.jsonl" 2>/dev/null && break
  kill -0 $app 2>/dev/null || { ended=yes; break; }
  sleep 0.1
done
if [ -n "$ended" ]; then
  wait $app; status=$?
  echo "up: no; the app ended with status $status before drawing ($(( ($(date +%s%N) - start) / 1000000 )) ms)" >"$out/result.txt"
  find /tmp/dev/logs -type f -size +0 2>/dev/null | sed 's/^/crash log: /' >>"$out/result.txt"
  kill $xvfb 2>/dev/null; wait $xvfb 2>/dev/null
  cat "$out/result.txt"
  exit 0
fi
echo "renderer line after $(( ($(date +%s%N) - start) / 1000000 )) ms" >"$out/timing.txt"
sleep "$SETTLE"
cp /tmp/fb/Xvfb_screen0 "$out/screen.xwd"
if command -v xprop >/dev/null; then
  window=$(xwininfo -root -children | awk '/"mcbench-lite"/ {print $1; exit}')
  if [ -n "$window" ]; then xprop -id "$window" WM_CLASS >"$out/wm_class.txt" 2>&1
  else echo "no window named mcbench-lite" >"$out/wm_class.txt"; fi
fi
if kill -0 $app 2>/dev/null; then
  kill -TERM $app
  for _ in $(seq 100); do kill -0 $app 2>/dev/null || break; sleep 0.1; done
  kill -0 $app 2>/dev/null && kill -KILL $app
  wait $app; status=$?
  echo "up: yes after ${SETTLE} s; status $status after SIGTERM" >"$out/result.txt"
else
  wait $app; status=$?
  echo "up: no; the app ended with status $status after drawing" >"$out/result.txt"
fi
kill $xvfb 2>/dev/null; wait $xvfb 2>/dev/null
find /tmp/dev/logs -type f -size +0 2>/dev/null | sed 's/^/crash log: /' >>"$out/result.txt"
cat "$out/result.txt"
