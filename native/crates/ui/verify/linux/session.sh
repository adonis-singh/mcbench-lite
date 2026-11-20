#!/usr/bin/env bash
# A private desktop session for the Linux checks: its own D-Bus session bus,
# its own accessibility bus with accessibility switched on, and a headless
# display, all private from the first process.
#
#   session.sh x11 <display number> <out dir> -- <command...>
#   session.sh wayland <socket name> <out dir> -- <command...>
#
# x11 starts `Xvfb :<n>` (1600x1000x24); wayland starts a headless `cage`
# (wlroots' headless backend) whose only client is the command, or with
# MCBENCH_COMPOSITOR=sway a headless sway that runs it. The command
# runs with DISPLAY or WAYLAND_DISPLAY set to the private display and
# DBUS_SESSION_BUS_ADDRESS to the private bus; the session ends when it
# does, and everything this script started is stopped.
#
# Private: every process starts with XDG_RUNTIME_DIR (a new folder under
# MCBENCH_RUNTIME_ROOT or <out dir>, removed at the end), XDG_CONFIG_HOME,
# XDG_CACHE_HOME and XDG_DATA_HOME under <out dir>, GSETTINGS_BACKEND=memory,
# DCONF_PROFILE naming a profile whose only database is in the private config
# folder,
# no display and no bus of the user's. The session bus and the accessibility
# bus are plain `dbus-daemon`s with this script's own configurations (no
# service activation on the session bus; the accessibility bus activates only
# the AT-SPI registry), listening in the private runtime folder.
# `at-spi-bus-launcher` is not used (a11y_bus.py says why). Nothing talks to
# the user's systemd manager.
#
# Proof, every run (privacy.py): the user's real `/run/user/<uid>/at-spi/`
# listing and dconf file's modification time before and after, compared,
# and, twice a second during the run, every process of the session checked
# for a socket bound under or connected to `/run/user/<uid>` and for open
# files there. The run fails if either finds anything; <out dir>/privacy.txt
# holds the verdict, privacy-before.txt, privacy-after.txt and
# privacy-watch.txt the evidence. The session runs as its own process group,
# ended by its id at the close; the run fails too if any process with its
# private XDG_RUNTIME_DIR is alive 2 s later (processes-before.txt and
# processes-after.txt list them, "none" when there are none).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
kind="$1"; name="$2"; out="$3"; shift 3
[ "$1" = "--" ] || { echo "session.sh: expected -- before the command" >&2; exit 2; }
shift
mkdir -p "$out"
out="$(cd "$out" && pwd)"
if [ -z "${MCBENCH_SESSION_INNER:-}" ]; then
  # The runtime folder holds the buses' and the display's sockets, whose
  # paths must stay under 108 bytes: MCBENCH_RUNTIME_ROOT names a short
  # private folder for it (default: <out dir>).
  rt_root="${MCBENCH_RUNTIME_ROOT:-$out}"
  mkdir -p "$rt_root"
  rt="$(mktemp -d "$(cd "$rt_root" && pwd)/rt.XXXXXX")"
  case "$rt" in /run/*) echo "session.sh: $rt is under /run" >&2; exit 2 ;; esac
  [ "${#rt}" -le 72 ] || { echo "session.sh: $rt is too long for socket paths; set MCBENCH_RUNTIME_ROOT" >&2; exit 2; }
  echo "$rt" >"$out/runtime-dir.txt"
  rm -rf "$out/xdg"
  mkdir -p "$out/xdg/config" "$out/xdg/cache" "$out/xdg/data" "$out/a11y-services"
  chmod 700 "$rt"
  # dconf, if anything reaches it despite GSETTINGS_BACKEND=memory, has one
  # database, in the private config folder.
  echo "user-db:mcbench-private" >"$out/xdg/dconf-profile"
  cat >"$out/session-bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$rt</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
  sed -e "s|<listen>unix:dir=/tmp</listen>|<listen>unix:dir=$rt</listen>|" \
      -e "s|<servicedir>[^<]*</servicedir>|<servicedir>$out/a11y-services</servicedir>|" \
      /usr/share/defaults/at-spi2/accessibility.conf >"$out/a11y-bus.conf"
  grep -q "unix:dir=$rt" "$out/a11y-bus.conf" && grep -q "<servicedir>$out/a11y-services" "$out/a11y-bus.conf" \
    || { echo "session.sh: could not make the accessibility bus configuration" >&2; exit 2; }
  printf '[D-BUS Service]\nName=org.a11y.atspi.Registry\nExec=/usr/lib/at-spi2-registryd\n' \
    >"$out/a11y-services/org.a11y.atspi.Registry.service"
  # The user's accessibility switch must read false before anything starts;
  # if it reads true someone else set it, and the run stops having changed
  # nothing.
  python3 "$here/privacy.py" switch || exit 2
  python3 "$here/privacy.py" snapshot "$out/privacy-before.txt"
  python3 "$here/privacy.py" procs "$rt" "$out/processes-before.txt" \
    || { echo "session.sh: processes of an earlier run with $rt are alive" >&2; exit 2; }
  stop="$out/privacy-stop"; rm -f "$stop"
  python3 "$here/privacy.py" watch $$ "$rt" "$out/privacy-before.txt" "$out/privacy-watch.txt" "$stop" &
  watcher=$!
  status=0
  # The whole session in its own session and process group (setsid without
  # forking: the background job's pid is the group's id), so it ends as one.
  setsid env -i HOME="$HOME" USER="$USER" LOGNAME="${LOGNAME:-$USER}" PATH="$PATH" LANG="${LANG:-C.UTF-8}" TERM=dumb \
    XDG_RUNTIME_DIR="$rt" XDG_CONFIG_HOME="$out/xdg/config" XDG_CACHE_HOME="$out/xdg/cache" \
    XDG_DATA_HOME="$out/xdg/data" GSETTINGS_BACKEND=memory DCONF_PROFILE="$out/xdg/dconf-profile" \
    ${MCBENCH_A11Y:+MCBENCH_A11Y="$MCBENCH_A11Y"} \
    ${MCBENCH_SCREEN:+MCBENCH_SCREEN="$MCBENCH_SCREEN"} \
    ${ORCA_PREFIX:+ORCA_PREFIX="$ORCA_PREFIX"} ${ORCA_PYTHONPATH:+ORCA_PYTHONPATH="$ORCA_PYTHONPATH"} \
    ${MCBENCH_UPDATE_EXPECTED:+MCBENCH_UPDATE_EXPECTED="$MCBENCH_UPDATE_EXPECTED"} \
    ${VERIFY:+VERIFY="$VERIFY"} \
    ${MCBENCH_COMPOSITOR:+MCBENCH_COMPOSITOR="$MCBENCH_COMPOSITOR"} \
    ${MCBENCH_WLR_RENDERER:+MCBENCH_WLR_RENDERER="$MCBENCH_WLR_RENDERER"} \
    MCBENCH_SESSION_INNER=1 \
    dbus-run-session --config-file="$out/session-bus.conf" --dbus-daemon=/usr/bin/dbus-daemon -- \
    "$0" "$kind" "$name" "$out" -- "$@" 2>>"$out/session.log" &
  group=$!
  echo "$group" >"$out/process-group.txt"
  wait "$group" || status=$?
  # Whatever of the group is left (the registry the accessibility bus
  # activated, say) ends with it, by the group's id.
  kill -TERM -- "-$group" 2>/dev/null || true
  sleep 2
  touch "$stop"; wait "$watcher" || true
  if ! python3 "$here/privacy.py" procs "$rt" "$out/processes-after.txt"; then
    echo "session.sh: processes of this run alive 2 s after its end:" >&2
    cat "$out/processes-after.txt" >&2
    status=98
  fi
  python3 "$here/privacy.py" snapshot "$out/privacy-after.txt"
  python3 "$here/privacy.py" verdict "$out/privacy-before.txt" "$out/privacy-after.txt" "$out/privacy-watch.txt" \
    >"$out/privacy.txt" || status=97
  cat "$out/privacy.txt" >&2
  rm -rf "$rt"
  exit "$status"
fi
case "$DBUS_SESSION_BUS_ADDRESS" in
  "unix:path=$XDG_RUNTIME_DIR/"*) ;;
  *) echo "session.sh: the session bus $DBUS_SESSION_BUS_ADDRESS is not in $XDG_RUNTIME_DIR" >&2; exit 2 ;;
esac
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT
exec 3>"$out/a11y-bus.address"
dbus-daemon --nofork --config-file="$out/a11y-bus.conf" --print-address=3 >"$out/a11y-bus.log" 2>&1 &
pids+=($!)
exec 3>&-
for _ in $(seq 50); do [ -s "$out/a11y-bus.address" ] && break; sleep 0.1; done
a11y_address="$(head -1 "$out/a11y-bus.address")"
case "$a11y_address" in
  "unix:path=$XDG_RUNTIME_DIR/"*) ;;
  *) echo "session.sh: the accessibility bus $a11y_address is not in $XDG_RUNTIME_DIR" >&2; exit 2 ;;
esac
# MCBENCH_A11Y=off leaves accessibility switched off (no client, no tree).
python3 "$here/a11y_bus.py" "$a11y_address" "$([ "${MCBENCH_A11Y:-on}" = off ] && echo off || echo on)" \
  >"$out/a11y-launcher.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
  dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus org.a11y.Bus.GetAddress >/dev/null 2>&1 && break
  sleep 0.1
done
export NO_AT_BRIDGE=0 GTK_A11Y=atspi
case "$kind" in
  x11)
    # MCBENCH_SCREEN gives the screen another size (check_first_window.py).
    Xvfb ":$name" -screen 0 "${MCBENCH_SCREEN:-1600x1000}x24" -nolisten tcp >"$out/xvfb.log" 2>&1 &
    pids+=($!)
    for _ in $(seq 50); do [ -S "/tmp/.X11-unix/X$name" ] && break; sleep 0.1; done
    export DISPLAY=":$name"
    "$@"
    ;;
  wayland)
    if [ "${MCBENCH_COMPOSITOR:-cage}" = sway ]; then
      # pane agent: sway (headless), which offers pointer constraints, for
      # the pointer lock (wayland_lock.sh runs it in a container). Its
      # configuration runs the command and ends sway with it; the command's
      # status comes back through a file.
      printf '%q ' "$@" >"$out/sway-command.txt"
      cat >"$out/sway-client.sh" <<CLIENT
#!/usr/bin/env bash
WAYLAND_SOCKET_NAME="$name" $(cat "$out/sway-command.txt")
echo \$? >"$out/sway-status.txt"
swaymsg exit
CLIENT
      chmod +x "$out/sway-client.sh"
      printf '%s\n' "output HEADLESS-1 resolution 1280x820" "default_border none" \
        "focus_follows_mouse no" "exec $out/sway-client.sh" >"$out/sway.conf"
      WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman WLR_HEADLESS_OUTPUTS=1 \
        sway -c "$out/sway.conf" >"$out/sway.log" 2>&1 || true
      exit "$(cat "$out/sway-status.txt")"
    fi
    # cage runs the command as its client; the command reads its socket name.
    # MCBENCH_WLR_RENDERER gives cage another renderer (vulkan, gles2: the
    # GPU's render node) for timings with the window on the GPU.
    WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER="${MCBENCH_WLR_RENDERER:-pixman}" WLR_HEADLESS_OUTPUTS=1 \
      cage -s -- env WAYLAND_SOCKET_NAME="$name" "$@" >"$out/cage.log" 2>&1
    ;;
  *) echo "session.sh: x11 or wayland, not $kind" >&2; exit 2 ;;
esac
