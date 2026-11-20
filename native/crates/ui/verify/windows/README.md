# verify/windows: the accessibility checks in the Windows VM

These check mcbench-ui (and, until its dev binary exists, gpui-pre's own `examples/a11y.rs`) through UI Automation, real keys, NVDA and Narrator, inside the Windows 11 VM that the toolkit spike built (`native/spikes/winvm/README.md` on branch `claude/sharp-mendeleev-e55a37`). Nothing here runs on a Mac or Linux desktop; the host side only copies files and sends QMP keys.

## Parts

| Path | What |
|---|---|
| `winvm.sh` | host side: run a guest script, run the probe, push a source folder, build it, collect screenshots, call any winvm script (`key.sh`, `type.sh`, `click.sh`, `screenshot.sh`) |
| `uia-probe/` | a UI Automation COM client (Rust, `windows` 0.62): `dump` (raw, control or content view, every property and pattern value the contract lists), `text` (Text pattern: document, walk by character, word, line or paragraph, selection), `act` (Invoke, Toggle, Expand, Collapse, SelectionItem.Select, Value.SetValue, RangeValue.SetValue, SetFocus, ScrollIntoView), `focused`, `events` (focus changed, property changed, structure changed, TextChanged, TextSelectionChanged, LiveRegionChanged, Invoked, ElementSelected, menu and window events, notifications), `keys` (SendInput each key, print the focused element after each, optional screenshot per key), `shot`, `launch` (ms to a visible window and to its first drawn pixels), `keylat` (ms from a key to the composer's pixels changing) |
| `guest/` | PowerShell run in the guest: `probe.ps1` (the probe in the desktop session), `start-dev.ps1` (the dev binary with `--scratch C:\s4\scratch`, at origin 40,40 unless `-NoOrigin`, with `--log C:\s4\ui-actions.jsonl`; `-Extra` takes `+` for a space, since `winvm.sh ps` passes arguments through two shells), `actions.ps1` (that log), `start-app.ps1`, `tail.ps1`, `clip.ps1`, `idle-cpu.ps1`, `rate.ps1` (presents per second from a traced build), `crash-check.ps1` (`--crash` from the desktop session: exit, crash logs in the scratch folder's `logs`, Windows Error Reporting, Application log), `clip-image.ps1` (a PNG onto the desktop session's clipboard, for paste), `scale.ps1`, `nvda-fetch.ps1`, `nvda-portable.ps1`, `nvda.ps1`, `narrator-setup.ps1` |
| `evidence/final/` | the runs on s4-gate 66448881 (UIA, NVDA, Narrator, crashes, fatal screen, Tab stop) and 74e3c796 (announcements, selection, IME, window, 150%), and the idle and latency measurements on 99278336; `evidence/s4gate-7e552b62/` and `s4gate-99278336/` are the earlier runs |
| `evidence/chat-screen/` | the runs against `mcbench-ui-dev --fixture gate`; `expected-dump-raw-100pct.txt` is the committed expected raw dump (rectangles relative to the window) |
| `evidence/shell-chat-057edbae/` | the pass over the shell, sidebar, chat and pickers at s4-chat 057edbae: UIA dumps, Tab walks, NVDA speech logs, Narrator phrases, timings with the host's load |
| `evidence/` | small text evidence of the runs this folder's results come from |

UI Automation does not cross Windows sessions and an SSH session has no desktop, so everything that touches a window runs through the VM's `C:\winvm\gui.ps1` (a one-shot interactive scheduled task as `bench`).

## Setup (once per overlay)

1. On the VM's host, make a work folder beside `winvm/` with a `scripts/` holding copies of `env.sh`, `start.sh`, `stop.sh` and `qmp.py` from `winvm/scripts/`, its `env.sh` pointing `RUN=` at an overlay folder of your own (so `reset.sh win11-tools` makes your own overlay and never touches the base or the shared `run/`) and starting QEMU and swtpm under the host's CPU budget (on adipc: `taskset -c 24-31 nice -n 19 ionice -c3`, `WINVM_CPUS=6`). Only start and stop use these copies (`winvm.sh start`, `winvm.sh stop`): the shared `env.sh` fixes `RUN` to the shared `run/` and pins nothing; every other call goes to the shared runners in `winvm/scripts/`.
2. Take the VM with `winvm/scripts/lock.sh take "$line" 3600` (the line names you, your overlay and a pid that runs while you hold it), then `export WINVM_SSH=<host> WINVM_WORK=<work folder on the host> WINVM_LOCK_LINE="$line"`. `winvm.sh` and `push-tree.sh` send nothing unless the LOCK holds exactly that line, and export it to the shared runners. After `winvm.sh stop`, `lock.sh release "$line"`.
3. Probe: `./winvm.sh push uia-probe uia-probe && ./winvm.sh build uia-probe` (about 25 s cold, 2 s warm).
4. NVDA: `./winvm.sh ps guest/nvda-fetch.ps1` (downloads NVDA 2026.3.1 from download.nvaccess.org and checks the SHA-256 published in its GitHub release notes, `6f80a5b0...1cd3f052`, and its NV Access signature), then `./winvm.sh ps guest/nvda-portable.ps1` (a portable copy in `C:\s4\nvda`, nothing installed, with a config that skips the welcome and usage dialogs and uses the silence synthesizer: speech is still logged).
5. Narrator: `./winvm.sh ps guest/narrator-setup.ps1` (Narrator Home does not open at start).

## Runs

| Check | Commands |
|---|---|
| Build the dev binary | `git archive HEAD core renderer native` into `C:\s4\repo` (tar.exe in the guest), then in `C:\s4\repo\native`: `cargo build -p mcbench-ui --bin mcbench-ui-dev --release --locked` with `CARGO_TARGET_DIR=C:\t\s4-native` (3 min 15 s cold) |
| Start the program | `./winvm.sh ps guest/start-app.ps1 -Exe C:\t\<name>\release\<bin>.exe -Proc <bin> -Log info`; then click its client area once (`./winvm.sh vm click.sh X Y`): a program started from a scheduled task is not in the foreground |
| Tree | `./winvm.sh probe dump <proc> raw --stable` (and `control`, `content`); `--stable` prints rectangles relative to the window, so dumps diff across runs |
| Text | `./winvm.sh probe text <proc> "type=Edit" word` (selectors: `id=`, `name=`, `type=`, joined by `&`, `#n` for the n-th match) |
| Actions | `./winvm.sh probe act <proc> "name=Reset counter" invoke`; `... toggle`, `expand`, `collapse`, `select`, `setvalue TEXT`, `focus` |
| Events | `./winvm.sh probe events <proc> 15` in the background, then the keys or actions |
| Keyboard | `./winvm.sh probe keys <proc> 500 tab tab shift+tab enter "text:hello" --shots C:\s4\shots`, then `./winvm.sh shots shots <name>`; or QMP keys from the host with `./winvm.sh vm key.sh tab` and `probe focused` after each |
| NVDA | `./winvm.sh ps guest/nvda.ps1 start`, keys through `key.sh` (`insert-...` is the NVDA key), `./winvm.sh ps guest/nvda.ps1 log` (lines `Speaking [...]` and `Input: ...`), `... stop` |
| Measurements | `./winvm.sh probe launch C:\t\s4-native\release\mcbench-ui-dev.exe --fixture gate`; `./winvm.sh probe keylat mcbench-ui-dev 30` (click the composer first); `./winvm.sh ps guest/idle-cpu.ps1` |
| 150% | `./winvm.sh ps guest/scale.ps1 -Percent 150` signs the desktop out; restart the VM (`stop.sh`, `start.sh`) so autologon signs it in at the new scale; `-Percent 100` and another restart to undo |
| Crash | `./winvm.sh ps guest/crash-check.ps1 -Kind panic` (and `abort`, `segv`), then `./winvm.sh vm screenshot.sh OUT.png` |
| Narrator | `./winvm.sh vm key.sh ctrl-meta_l-ret` starts and stops it; after each step `key.sh insert-ctrl-x` (copy last phrase) and `./winvm.sh ps guest/clip.ps1`; `screenshot.sh` shows its highlight |

## Known behaviour of the VM

A program started through `gui.ps1` opens behind the desktop (Windows' foreground lock): it is its thread's active window but not the foreground window until clicked. Before patch 3 in `native/vendor/README.md`, UI Automation then never reported a focused element inside a GPUI window (`evidence/a11y-example/keys-walk-stock.txt` against `keys-walk-patched.txt`). Direct3D runs on WARP; there is no audio device, so screen readers are checked by their logs, not by ear.
