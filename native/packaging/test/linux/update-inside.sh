#!/bin/bash
# The update test's steps inside one container, run as root by update.sh
# (`docker exec`) after inside.sh's prep and install; the app runs as the
# user u on a private Xvfb display. <route> is script or appimage.
#
#   update-inside.sh start <route>          starts the app as u and waits up to 180 s for its window
#   update-inside.sh wait-update <route> <s> waits up to <s> seconds for .<name>.update and .update.json
#   update-inside.sh wait-log <route> <s>    waits up to <s> seconds for update.log to have a line
#   update-inside.sh quit <route>           SIGTERM to every app process of the install
#   update-inside.sh state <route> <sha256> the running app's program against <sha256>, its version file,
#                                           what lies beside the payload, update.log
#   update-inside.sh old-gone <route> <s>   waits up to <s> seconds for <name>.old to go
#   update-inside.sh hold <route>           starts `headless engine` of the install and keeps it running
#   update-inside.sh unhold                 ends it
set -euo pipefail
step=$1 route=${2:-}
as_u() { su - u -c "$1"; }
now() { date +%s%N; }
ms() { echo $(( ($2 - $1) / 1000000 )); }
load() { echo "load average $(cut -d" " -f1-3 /proc/loadavg)"; }
# The test user's home, as the system has it.
h=$(getent passwd u | cut -d: -f6)
case $route in
  script) dir=$h/.local/lib name=mcbench-lite cmd=$h/.local/bin/mcbench-lite ;;
  appimage) dir=$h name=mcbench-lite-x86_64.AppImage cmd=$h/mcbench-lite-x86_64.AppImage ;;
  "") ;;
  *) echo "no route $route: script or appimage" >&2; exit 2 ;;
esac
log=$h/.local/share/mcbench-lite/logs/update.log

# The pids of the install's app processes (not its headless children):
# every process of u whose program is mcbench-lite from the payload, the
# AppImage's mount or a payload moved aside, and whose arguments are not
# headless or sandbox-agent.
app_pids() {
  as_u 'for p in /proc/[0-9]*; do
    exe=$(readlink $p/exe 2>/dev/null) || continue
    case $exe in */mcbench-lite/mcbench-lite | */mcbench-lite.old/mcbench-lite | */usr/lib/mcbench-lite/mcbench-lite) ;; *) continue ;; esac
    tr "\0" " " < $p/cmdline | grep -qE " (headless|sandbox-agent|tool-bridge)" && continue
    echo ${p#/proc/}
  done'
}

case $step in
  start)
    if [ ! -e /tmp/.X11-unix/X99 ]; then
      as_u "mkdir -p -m 700 /tmp/xdg-u; Xvfb :99 -screen 0 1280x800x24 -nolisten tcp > /tmp/xvfb.log 2>&1 &"
      for _ in $(seq 50); do [ -e /tmp/.X11-unix/X99 ] && break; sleep 0.1; done
    fi
    t0=$(now)
    as_u "cd; DISPLAY=:99 XDG_RUNTIME_DIR=/tmp/xdg-u RUST_BACKTRACE=1 $cmd >> /tmp/app.log 2>&1 &"
    for _ in $(seq 1800); do
      as_u "DISPLAY=:99 xdotool search --onlyvisible --name 'mcbench-lite\$'" > /tmp/window.id 2>/dev/null && break
      sleep 0.1
    done
    [ -s /tmp/window.id ] || { tail -30 /tmp/app.log; echo "no window named mcbench-lite within 180 s"; exit 1; }
    echo "window mapped $(ms "$t0" "$(now)") ms after start; app pids $(app_pids | tr '\n' ' '); $(load)" ;;

  wait-update)
    t0=$(now)
    for _ in $(seq $(( $3 * 10 ))); do
      [ -e "$dir/.$name.update" ] && [ -e "$dir/.$name.update.json" ] && break
      sleep 0.1
    done
    if [ -e "$dir/.$name.update.json" ]; then
      echo "the download waits beside the install $(ms "$t0" "$(now)") ms after the wait began: $(ls -la "$dir" | grep -F ".$name." | awk '{print $5, $9}' | tr '\n' ';')"
    else
      echo "no .$name.update and .$name.update.json within $3 s: $(ls -la "$dir" | grep -F "$name" | awk '{print $9}' | tr '\n' ' ')"
    fi ;;

  wait-log)
    for _ in $(seq $(( $3 * 10 ))); do [ -s "$log" ] && break; sleep 0.1; done
    if [ -s "$log" ]; then echo "update.log:"; sed 's/^/  /' "$log"; else echo "update.log has no line after $3 s"; fi ;;

  quit)
    pids=$(app_pids)
    [ -n "$pids" ] || { echo "no app process to quit"; exit 0; }
    t0=$(now)
    kill -TERM $pids
    for _ in $(seq 100); do [ -z "$(app_pids)" ] && break; sleep 0.1; done
    [ -z "$(app_pids)" ] || { echo "the app did not quit 10 s after SIGTERM"; kill -KILL $(app_pids); }
    rm -f /tmp/window.id
    echo "quit (pids $(echo $pids)) in $(ms "$t0" "$(now)") ms" ;;

  state)
    pid=$(app_pids | head -1)
    exe=$(as_u "readlink /proc/$pid/exe")
    sum=$(as_u "sha256sum /proc/$pid/exe" | cut -c1-64)
    echo "running: pid $pid, $exe, sha256 $sum ($([ "$sum" = "$3" ] && echo "the expected program" || echo "NOT the expected program $3"))"
    echo "its version file: $(as_u "cat '$(dirname "$exe")/version'")"
    echo "beside the payload in $dir: $(ls -A "$dir" | grep -F "$name" | tr '\n' ' ')"
    if [ -e "$log" ]; then echo "update.log:"; sed 's/^/  /' "$log"; else echo "update.log: absent"; fi ;;

  old-gone)
    t0=$(now)
    for _ in $(seq $(( $3 * 10 ))); do [ -e "$dir/$name.old" ] || break; sleep 0.1; done
    if [ -e "$dir/$name.old" ]; then echo "$name.old is still there $3 s later"; else echo "$name.old is gone ($(ms "$t0" "$(now)") ms after the check began)"; fi ;;

  hold)
    # The engine reads requests from a pipe that a sleep holds open, so it
    # runs until it is ended. (A fifo opened by the shell ends it at once.)
    as_u "cd; (sleep 100000 | $cmd headless engine \$(ls -d \$HOME/.local/share/mcbench-lite/minecraft/1.21.10-*) > /tmp/hold.log 2>&1 &); sleep 2"
    : > /tmp/hold.pids
    for p in /proc/[0-9]*; do
      case $(tr '\0' ' ' < "$p/cmdline" 2>/dev/null) in *" headless engine "* | "sleep 100000 ") echo "${p#/proc/}" >> /tmp/hold.pids ;; esac
    done
    engine=$(for p in $(cat /tmp/hold.pids); do grep -q "headless engine" "/proc/$p/cmdline" 2>/dev/null && echo "$p"; done | tail -1)
    [ -n "$engine" ] || { cat /tmp/hold.log; echo "the headless engine ended at once"; exit 1; }
    echo "headless engine running as pid $engine: $(as_u "readlink /proc/$engine/exe") $(tr '\0' ' ' < /proc/$engine/cmdline)"
    ino=$(stat -c %i "$dir/.$name.lock")
    echo "holds on .$name.lock: $(grep -c ":$ino " /proc/locks)" ;;

  unhold)
    alive=""
    for p in $(cat /tmp/hold.pids); do kill -0 "$p" 2>/dev/null && alive="$alive $p"; done
    echo "the held processes still running:${alive:- none}"
    [ -z "$alive" ] || kill $alive
    echo "the engine's output: $(head -c 300 /tmp/hold.log)" ;;
esac
