#!/usr/bin/env bash
# Every Linux check against one dev binary, on X11 and on Wayland.
#
#   all.sh <mcbench-ui-dev> <out dir>
#
# Uses display :97 and the Wayland socket name `gate`. Set MCBENCH_RUNTIME_ROOT
# to a short private folder (session.sh: socket paths). Orca runs when
# ORCA_PREFIX and ORCA_PYTHONPATH name a build of it (check_orca.py). The
# pointer checks (selection, window close) run on both, through wl_pointer.py
# on Wayland; Orca runs on X11 only. Prints
# one line per check and backend; each check's folder holds its logs,
# dumps and screenshots.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin="$1"; out="$2"
mkdir -p "$out"
# Files picked by path (`--pick`): the chat's and the quit's checks.
mkdir -p "$out/picks"
printf 'the base is stone' >"$out/picks/notes.txt"
cp "$here/../../fixtures/gate/lamp.png" "$out/picks/door.png"
run() {
  local backend="$1" check="$2"; shift 2
  local name; name="$(basename "$check" .py)-$backend${1:+-$1}"
  # MCBENCH_X and MCBENCH_WL name another agent's display and socket.
  local display=${MCBENCH_X:-97}; [ "$backend" = wayland ] && display=${MCBENCH_WL:-gate}
  rm -rf "${out:?}/$name"
  timeout 300 "$here/run.sh" "$backend" "$display" "$bin" "$out/$name" "${FIXTURE:-gate}" "$here/$check" "$@" >/dev/null 2>&1
  printf '%-28s %s | %s\n' "$name" "$(tr '\n' ' ' < "$out/$name/exit.txt")" "$(tail -1 "$out/$name/check.log")"
}
for backend in x11 wayland; do
  run "$backend" check_tree.py
  run "$backend" check_text.py
  run "$backend" check_actions.py
  # The streamed answer at 150 ms a word outlasts the Tab walk to Stop past
  # the footer's pickers (chat agent); a chat pulsing (the sidebar's dot)
  # while focus walks.
  MCBENCH_APP_ARGS="--rate-ms 150 --running chat-gate" run "$backend" check_keyboard.py
  run "$backend" check_selection.py
  # quit: the quit key and the close request from every state, each in a
  # run of its own (check_quit.py names the states).
  for state in landing streaming composer model palette settings lightbox confirm held drag; do
    case "$state" in
      streaming) args="--rate-ms 150 --running chat-gate --join chat-gate --simulate-ms 300" ;;
      composer|lightbox) args="--pick $out/picks/notes.txt --pick $out/picks/door.png" ;;
      held) args="--setup-pending --setup-ms 60000" ;;
      *) args="" ;;
    esac
    for how in key close; do
      MCBENCH_APP_ARGS="$args" run "$backend" check_quit.py "$state-$how"
    done
  done
  FIXTURE=long run "$backend" check_long.py
  run "$backend" check_layout.py
  MCBENCH_APP_ARGS=--gallery run "$backend" check_gallery.py
  # pickers: the composer's pickers in the gallery.
  MCBENCH_APP_ARGS=--gallery run "$backend" check_pickers.py
  # sidebar agent: the sidebar's lists, archive, search, filters, palette.
  MCBENCH_APP_ARGS="--running chat-gate" run "$backend" check_sidebar.py
done
if [ -n "${ORCA_PREFIX:-}" ]; then run x11 check_orca.py; fi
# chat-fix: the first window with no stored placement, on a screen larger
# and one smaller than 1280 by 820 (X11: Xvfb takes a screen size).
MCBENCH_PLACE=first MCBENCH_SCREEN=1600x1000 run x11 check_first_window.py 1600x1000
MCBENCH_PLACE=first MCBENCH_SCREEN=1024x700 run x11 check_first_window.py 1024x700
# chat (stage four's chat agent): the chat column with real input, files
# picked by path (the files are made at the top).
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--turn full --rate-ms 15 --running chat-gate --join chat-gate --refuse chat-farm --select --pick $out/picks/notes.txt --pick $out/picks/door.png" \
    run "$backend" check_chat.py
  # chat-fix: the held send while setup runs (the stand-in's pending setup).
  MCBENCH_APP_ARGS="--setup-pending --setup-ms 12000" run "$backend" check_chat.py held
done
# settings agent: Settings and the setup screen, each stand-in play its own run.
for backend in x11 wayland; do
  run "$backend" check_settings.py
  MCBENCH_APP_ARGS="--settings claude-sign-in --settings-step-ms 600" run "$backend" check_settings.py claude
  MCBENCH_APP_ARGS="--settings chatgpt-sign-in --settings-step-ms 600" run "$backend" check_settings.py chatgpt
  MCBENCH_APP_ARGS="--settings sign-in-fails --settings save-refused --settings remove-fails --settings clear-fails --settings votes-1 --settings-step-ms 600" \
    run "$backend" check_settings.py failures
  MCBENCH_APP_ARGS="--settings claude-sign-in --settings install-fails --settings-step-ms 400" run "$backend" check_settings.py install-fails
  MCBENCH_APP_ARGS="--settings claude-sign-in --settings install-refused" run "$backend" check_settings.py install-refused
  MCBENCH_APP_ARGS="--settings read-fails" run "$backend" check_settings.py read-fails
  MCBENCH_APP_ARGS="--settings shares-failed --settings votes-unreadable" run "$backend" check_settings.py shares
  MCBENCH_APP_ARGS="--settings setup-fails --settings no-ai --open /new --settings-step-ms 700" run "$backend" check_setup.py fails
  MCBENCH_APP_ARGS="--settings setup-optional-fails --open /new --settings-step-ms 700" run "$backend" check_setup.py optional
  MCBENCH_APP_ARGS="--settings setup-wsl --open /new --settings-step-ms 1500" run "$backend" check_setup.py wsl
  MCBENCH_APP_ARGS="--settings setup-wsl-restart --open /new" run "$backend" check_setup.py wsl-restart
  MCBENCH_APP_ARGS="--settings setup-wsl-refused --open /new --settings-step-ms 1500" run "$backend" check_setup.py wsl-refused
  MCBENCH_APP_ARGS="--settings setup-wsl-optional-refused --open /new --settings-step-ms 1500" run "$backend" check_setup.py needs-empty
  MCBENCH_APP_ARGS="--settings setup-wsl-optional --open /chat/chat-gate --settings-step-ms 1500" run "$backend" check_setup.py needs-chat
  MCBENCH_APP_ARGS="--settings setup-wsl-optional-restart --open /bench" run "$backend" check_setup.py needs-bench
  MCBENCH_SCREEN=1280x820 run "$backend" check_settings.py bounds-1280
done
# settings agent: the panel at the least and a wide window (X11: Xvfb
# takes a screen size).
MCBENCH_SCREEN=640x420 MCBENCH_APP_ARGS="--size 640x420" run x11 check_settings.py bounds-640
MCBENCH_SCREEN=1920x1080 MCBENCH_APP_ARGS="--size 1920x1080" run x11 check_settings.py bounds-1920
# chat-fix-2: the macOS pass's findings (check_reach.py): actions on rows out
# of view, set sizes, tips, the rows nothing showed, a failed search said.
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--archive-ms 3000 --unreadable chat-farm" run "$backend" check_reach.py
  MCBENCH_APP_ARGS="--search-fails" run "$backend" check_reach.py fails
done
# narrow agent: the narrow window (980 px and below), resized while it runs:
# Build with a turn running, Bench, and the setup line over the composer
# (the dark theme: the light pictures take a preferences file).
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--running chat-gate --join chat-gate --rate-ms 150" run "$backend" check_narrow.py dark
  FIXTURE=bench MCBENCH_APP_ARGS="--open /bench/bench-castle" run "$backend" check_narrow.py dark-bench
  MCBENCH_APP_ARGS="--settings setup-wsl-optional --open /chat/chat-gate" run "$backend" check_narrow.py dark-setup
done
# start-place agent: the window opens at its start place before the core's
# first lists (--lists-ms holds them, as the real core's cold start does): a
# chat and a bench's comparison, typed text kept, no second open (the dark
# theme: the light pictures take a preferences file).
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--open /chat/chat-gate --lists-ms 4000" run "$backend" check_start.py dark
  FIXTURE=bench MCBENCH_APP_ARGS="--open /bench/bench-castle --lists-ms 4000" run "$backend" check_start.py dark-bench
done
# par-bench: Bench and the prompt library (check_bench with a picked
# picture for a round's file), and their key-to-paint runs (debug build:
# the numbers are the run's, not the release figures).
for backend in x11 wayland; do
  FIXTURE=bench MCBENCH_APP_ARGS="--open /bench/bench-castle --pick $out/picks/door.png" run "$backend" check_bench.py dark
  FIXTURE=bench MCBENCH_APP_ARGS="--open /bench" run "$backend" check_prompts.py dark
  FIXTURE=bench MCBENCH_APP_ARGS="--open /bench --bench prompts-fail" run "$backend" check_prompts.py failed
  for m in compare rounds leaderboard; do
    MCBENCH_DUMP=off FIXTURE=bench MCBENCH_APP_ARGS="--open /bench/bench-castle" run "$backend" check_bench_perf.py "$m"
  done
  MCBENCH_DUMP=off FIXTURE=bench MCBENCH_APP_ARGS="--open /bench --prompts 200" run "$backend" check_bench_perf.py palette
  MCBENCH_DUMP=off FIXTURE=bench MCBENCH_APP_ARGS="--open /bench --prompts 1 --prompt-chars 20000" run "$backend" check_bench_perf.py editor
done
# pane agent: the 3D pane (fake pane): its walk (the pointer lock on X11
# among its steps), its speed (an orbit, an idle minute), the first frame's
# line; the pointer lock on Wayland through wayland_lock.sh (sway, which
# offers pointer constraints, in a container).
for backend in x11 wayland; do
  mkdir -p "$out/saved-pane-$backend"
  MCBENCH_APP_ARGS="--save-to $out/saved-pane-$backend" run "$backend" check_pane.py dark "$out/saved-pane-$backend"
  run "$backend" check_pane_perf.py orbit
  run "$backend" check_pane_perf.py idle
  run "$backend" check_first_frame.py
done
"$here/wayland_lock.sh" "$out/check_pane_lock-wayland" >/dev/null 2>&1
printf '%-28s %s | %s\n' check_pane_lock-wayland "$(tr '\n' ' ' < "$out/check_pane_lock-wayland/exit.txt")" "$(tail -1 "$out/check_pane_lock-wayland/check.log")"
# par-shell: the error screen after a panic (check_fatal.py): on the
# window's thread while drawing, on a thread of its own, and in the error
# screen's own process; both themes.
for backend in x11 wayland; do
  for m in draw draw-light panic screen; do
    MCBENCH_APP_ARGS="--crash ${m%-light} --crash-after-ms 6000" run "$backend" check_fatal.py "$m"
  done
done
# share agent: the Share panel, by keyboard and pointer, its failure and the
# real core's not-joined answer, in both themes (the light pictures take a
# preferences file: MCBENCH_PREFS_LIGHT).
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--open /chat/chat-gate --share-step-ms 2500" run "$backend" check_share.py dark
  MCBENCH_APP_ARGS="--open /chat/chat-gate --share fails-publishing --share-step-ms 600" run "$backend" check_share.py fails-dark
  MCBENCH_APP_ARGS="--open /chat/chat-gate --share not-joined" run "$backend" check_share.py not-joined-dark
  if [ -n "${MCBENCH_PREFS_LIGHT:-}" ]; then
    MCBENCH_APP_ARGS="--open /chat/chat-gate --share-step-ms 2500 --preferences $MCBENCH_PREFS_LIGHT" run "$backend" check_share.py light
    MCBENCH_APP_ARGS="--open /chat/chat-gate --share not-joined --preferences $MCBENCH_PREFS_LIGHT" run "$backend" check_share.py not-joined-light
  fi
done
# quit-ask: the question before a quit while builds run, by keyboard and
# pointer, one build and two (a chat and a bench model), in both themes
# (the light picture takes a preferences file: MCBENCH_PREFS_LIGHT).
for backend in x11 wayland; do
  MCBENCH_APP_ARGS="--running chat-gate" run "$backend" check_quit_ask.py dark
  if [ -n "${MCBENCH_PREFS_LIGHT:-}" ]; then
    MCBENCH_APP_ARGS="--running chat-gate --preferences $MCBENCH_PREFS_LIGHT" run "$backend" check_quit_ask.py light
  fi
  FIXTURE=bench MCBENCH_APP_ARGS="--running chat-gate" run "$backend" check_quit_ask.py two
done
# end of quit-ask's block
