#!/bin/bash
# The clean-machine test's steps inside one system's container, run as root
# by machine.sh (`docker exec`); every command a user types runs as the
# fresh user "u" through `su - u`. Results go to /results.
#
#   inside.sh prep <route> [harness]    the user u with sudo, what <route>'s page command needs, and with
#                                       "harness" Xvfb and xdotool for the window test
#   inside.sh listing <name>            /results/<name>.list: every path on the root file system but
#                                       /proc, /sys, /dev, /run and /tmp, with its type
#   inside.sh install <route>           the page's install command for <route>, timed, with the bytes received
#   inside.sh libraries <route>         each installed program's libraries as the loader resolves them
#   inside.sh window <route>            starts the app as u through its command on a private Xvfb display,
#                                       waits for its window and its first painted frame (the window's log,
#                                       MCBENCH_UI_LOG), takes the screenshot, quits it by its pid
#   inside.sh helpers <route>           a headless build call through the same command (the build child's lookup)
#   inside.sh upgrade <route>           the system's update command, timed
#   inside.sh uninstall <route>         the system's removal command, timed
set -euo pipefail
. /etc/os-release
family=$ID
case $ID in ubuntu) family=debian ;; esac
REL=$MCBENCH_LITE_RELEASES
step=$1 route=${2:-}
as_u() { su - u -c "$1"; }
received() { cat /sys/class/net/eth0/statistics/rx_bytes; }
now() { date +%s%N; }
# The host's load beside every timing (uptime is not in every image).
load() { echo "load average $(cut -d" " -f1-3 /proc/loadavg)"; }
ms() { echo $(( ($2 - $1) / 1000000 )); }

# The command the user types to start the app, for each route.
app() {
  case $route in
    apt | apt-bare | dnf | aur) echo /usr/bin/mcbench-lite ;;
    script) echo '$HOME/.local/bin/mcbench-lite' ;;
    appimage) echo '$HOME/mcbench-lite-x86_64.AppImage' ;;
  esac
}

case $step in
  prep)
    case $family in
      debian)
        export DEBIAN_FRONTEND=noninteractive
        apt-get -qq update
        need="sudo wget"
        [ "$route" != script ] || need="$need curl"
        [ "$route" != appimage ] || need="$need fuse3"
        [ "${3:-}" != harness ] || need="$need xvfb xdotool"
        # The script and AppImage routes install no system libraries: a
        # desktop has them, a container does not, so these routes' systems
        # get what the .deb depends on (install.sh refuses a system without
        # the libraries the app links).
        case $route in script | appimage)
          need="$need libxkbcommon-x11-0 libegl1 libwayland-egl1 libvulkan1 mesa-vulkan-drivers bubblewrap" ;;
        esac
        apt-get -qq install -y --no-install-recommends $need >/dev/null ;;
      fedora)
        # util-linux for su, which the image lacks and the test runs as.
        need="sudo util-linux"
        [ "${3:-}" != harness ] || need="$need xorg-x11-server-Xvfb xdotool"
        dnf -q -y install $need >/dev/null ;;
      arch)
        need="sudo base-devel git"
        [ "${3:-}" != harness ] || need="$need xorg-server-xvfb xdotool"
        pacman -Syu --noconfirm --needed -q $need >/dev/null 2>&1 ;;
    esac
    useradd -m -s /bin/bash u
    echo 'u ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/u
    echo "$PRETTY_NAME; prepared for $route: $need" ;;

  listing)
    find / -xdev \( -path /proc -o -path /sys -o -path /dev -o -path /run -o -path /tmp \) -prune -o -printf '%y %p\n' \
      | LC_ALL=C sort > "/results/$2.list"
    echo "/results/$2.list: $(wc -l < "/results/$2.list") paths" ;;

  install)
    case $route in
      apt) cmd="wget -qO /tmp/mcbench-lite.deb $REL/mcbench-lite_\$(dpkg --print-architecture).deb && sudo apt install -y /tmp/mcbench-lite.deb" ;;
      apt-bare) cmd="wget -qO /tmp/mcbench-lite.deb $REL/mcbench-lite_\$(dpkg --print-architecture).deb && sudo apt install -y --no-install-recommends /tmp/mcbench-lite.deb" ;;
      dnf) cmd="sudo dnf install -y $REL/mcbench-lite.\$(uname -m).rpm" ;;
      aur) cmd="git clone $MCBENCH_LITE_SITE/aur/mcbench-lite-bin.git && cd mcbench-lite-bin && makepkg -si --noconfirm" ;;
      script) cmd="curl -fsSL $MCBENCH_LITE_SITE/install.sh | MCBENCH_LITE_RELEASES=$REL sh" ;;
      appimage) cmd="wget -qO mcbench-lite-x86_64.AppImage $REL/mcbench-lite-x86_64.AppImage && chmod +x mcbench-lite-x86_64.AppImage" ;;
    esac
    echo "command: $cmd"
    b0=$(received) t0=$(now)
    as_u "$cmd" > /results/install.log 2>&1 || { tail -30 /results/install.log; echo "install failed"; exit 1; }
    t1=$(now) b1=$(received)
    tail -4 /results/install.log
    echo "fonts on the system: $(find /usr/share/fonts /usr/local/share/fonts -type f 2>/dev/null | wc -l) files; fontconfig's library: $(ls /usr/lib*/libfontconfig.so.1 /usr/lib/*/libfontconfig.so.1 2>/dev/null | head -1 || true); fc-match: $(command -v fc-match || echo none)"
    echo "install: $(ms "$t0" "$t1") ms, $((b1 - b0)) bytes received; $(load)" ;;

  libraries)
    for f in $(as_u "dirname \$(readlink -f $(app))")/mcbench-*; do
      missing=$(ldd "$f" 2>&1 | grep -c 'not found' || true)
      echo "$f: $missing libraries not found"
      ldd "$f" 2>&1 | grep 'not found' || true
    done ;;

  window)
    as_u "mkdir -p -m 700 /tmp/xdg-u /tmp/shots"
    as_u "Xvfb :99 -screen 0 1280x800x24 -nolisten tcp -fbdir /tmp/shots > /tmp/xvfb.log 2>&1 & echo \$! > /tmp/xvfb.pid"
    # The display answers a request before the app starts: its socket alone
    # can exist before it serves, or after Xvfb has ended.
    ready=
    for _ in $(seq 300); do
      as_u "DISPLAY=:99 xdotool getdisplaygeometry" >/dev/null 2>&1 && { ready=1; break; }
      sleep 0.1
    done
    [ -n "$ready" ] || { tail -40 /tmp/xvfb.log; echo "Xvfb :99 did not answer within 30 s"; exit 1; }
    t0=$(now)
    as_u "cd; DISPLAY=:99 XDG_RUNTIME_DIR=/tmp/xdg-u MCBENCH_UI_LOG=/tmp/ui.log RUST_BACKTRACE=1 $(app) > /tmp/app.log 2>&1 & echo \$! > /tmp/app.pid"
    # Up to 180 s for a visible window named mcbench-lite, failing at once
    # when the app ends first.
    for _ in $(seq 1800); do
      as_u "DISPLAY=:99 xdotool search --onlyvisible --name 'mcbench-lite\$'" > /tmp/window.id 2>/dev/null && break
      kill -0 "$(cat /tmp/app.pid)" 2>/dev/null || { tail -40 /tmp/app.log; echo "the app ended before its window appeared"; exit 1; }
      sleep 0.1
    done
    [ -s /tmp/window.id ] || { tail -40 /tmp/app.log; echo "no window named mcbench-lite within 180 s"; exit 1; }
    t1=$(now)
    echo "window $(cat /tmp/window.id) mapped $(ms "$t0" "$t1") ms after start; $(load)"
    # The first painted frame, from the window's log (MCBENCH_UI_LOG): the
    # app's own time since its start, and its wall clock against the instant
    # just before the start.
    for _ in $(seq 600); do grep -q '"action":"first_frame"' /tmp/ui.log 2>/dev/null && break; sleep 0.1; done
    frame=$(grep -m1 '"action":"first_frame"' /tmp/ui.log) || { echo "no first_frame line in the window's log within 60 s"; exit 1; }
    since=$(echo "$frame" | sed 's/.*"since_start_ms":\([0-9.]*\).*/\1/')
    epoch=$(echo "$frame" | sed 's/.*"epoch_ms":\([0-9]*\).*/\1/')
    echo "first frame: since_start_ms $since; epoch_ms $((epoch - t0 / 1000000)) ms after the instant before the start; $(load)"
    grep -E '"action":"(renderer|fonts)"' /tmp/ui.log || true
    cp /tmp/ui.log /results/ui.log
    as_u "DISPLAY=:99 xdotool getwindowname $(head -1 /tmp/window.id); DISPLAY=:99 xdotool getwindowgeometry $(head -1 /tmp/window.id)"
    sleep 20
    cp /tmp/shots/Xvfb_screen0 /results/screen.xwd
    pid=$(cat /tmp/app.pid)
    # Read as u: root in a container may not read another user's /proc/<pid>/exe.
    exe=$(as_u "readlink /proc/$pid/exe")
    echo "the app runs as pid $pid: /proc/$pid/exe -> $exe"
    as_u "ls -l '$(dirname "$exe")'"
    echo "shared libraries mapped: $(as_u "grep -o '[^/]*\.so[.0-9]*\$' /proc/$pid/maps" | sort -u | tr '\n' ' ')"
    t0=$(now)
    kill -TERM "$pid"
    for _ in $(seq 100); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    if kill -0 "$pid" 2>/dev/null; then echo "the app did not quit 10 s after SIGTERM"; kill -KILL "$pid"; else echo "the app quit $(ms "$t0" "$(now)") ms after SIGTERM"; fi
    kill -TERM "$(cat /tmp/xvfb.pid)"
    tail -5 /tmp/app.log
    cp /tmp/app.log /results/app.log
    echo "quit" ;;

  helpers)
    # The build child's lookup through the same command, with neither
    # MCBENCH_BUILD_CHILD nor MCBENCH_SANDBOX_AGENT_STATIC set: a headless
    # build call, on the tables the app unpacked at its first start (a turn
    # that only builds needs no textures).
    cat > /tmp/helpers.sh <<'EOF'
set -eu
unset MCBENCH_BUILD_CHILD MCBENCH_SANDBOX_AGENT_STATIC
app=$1
tables=$(ls -d "$HOME"/.local/share/mcbench-lite/minecraft/1.21.10-*)
data=/tmp/headless-data
rm -rf "$data"; mkdir -p "$data/assets/1.21.10"
cp "$tables/blocks.json" "$data/assets/1.21.10/blocks.json"
export MCBENCH_TABLES=$tables
printf '{"id":1,"op":"create","data":"%s","provider":"claude-code","model":"claude-haiku-5-5"}\n' "$data" \
  | "$app" headless turn > /tmp/create.json
chat=$(sed 's/.*"chat":"\([^"]*\)".*/\1/' /tmp/create.json)
program='module.exports = (mc) => { mc.fill([0, 0, 0], [2, 0, 2], \"stone\"); };'
printf '{"id":2,"op":"call","data":"%s","chat":"%s","name":"build","arguments":{"summary":"floor","program":"%s"},"imageInput":false,"turnChat":true}\n' \
  "$data" "$chat" "$program" | "$app" headless turn
EOF
    chmod 644 /tmp/helpers.sh
    echo "the build call through $(app), MCBENCH_BUILD_CHILD and MCBENCH_SANDBOX_AGENT_STATIC unset:"
    as_u "bash /tmp/helpers.sh $(app)" > /tmp/helpers.out 2>&1 || { cut -c1-700 /tmp/helpers.out; echo "the build call failed"; exit 1; }
    cut -c1-700 /tmp/helpers.out
    # The tool's own result, inside the reply's content: the reply is ok
    # even when the build failed.
    grep -qF '"content":"{\"ok\":true' /tmp/helpers.out || { echo "the build call failed"; exit 1; } ;;

  upgrade)
    case $route in
      apt | apt-bare) cmd="sudo apt update && sudo apt upgrade -y" ;;
      dnf) cmd="sudo dnf upgrade -y" ;;
    esac
    echo "command: $cmd"
    b0=$(received) t0=$(now)
    as_u "$cmd" > /results/upgrade.log 2>&1 || { tail -30 /results/upgrade.log; echo "upgrade failed"; exit 1; }
    t1=$(now) b1=$(received)
    grep -iE 'mcbench|key|signature|import' /results/upgrade.log | head -12
    echo "upgrade: $(ms "$t0" "$t1") ms, $((b1 - b0)) bytes received; $(load)"
    as_u "cat \$(dirname \$(readlink -f $(app)))/install-source"
    case $route in apt | apt-bare) dpkg-query -W mcbench-lite ;; dnf) rpm -q mcbench-lite ;; esac ;;

  uninstall)
    case $route in
      apt | apt-bare) cmd="sudo apt purge --autoremove -y mcbench-lite" ;;
      dnf) cmd="sudo dnf remove -y mcbench-lite" ;;
      aur) cmd="sudo pacman -Rns --noconfirm mcbench-lite-bin" ;;
      script) cmd="curl -fsSL $MCBENCH_LITE_SITE/install.sh | sh -s -- --uninstall" ;;
      appimage) cmd="rm \$HOME/mcbench-lite-x86_64.AppImage" ;;
    esac
    echo "command: $cmd"
    t0=$(now)
    as_u "$cmd" > /results/uninstall.log 2>&1 || { tail -30 /results/uninstall.log; echo "uninstall failed"; exit 1; }
    t1=$(now)
    tail -3 /results/uninstall.log
    echo "uninstall: $(ms "$t0" "$t1") ms" ;;
esac
