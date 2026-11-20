# The macOS checks: the chat screen's gate and the shell and chat pass

Everything here runs in a macOS guest VM under Tart (the rig), never on the host: nothing appears on the host's display. The host only builds binaries and reads probes.

## Environment

| Variable | What |
|---|---|
| `RIG_HOME` | the rig folder: `tools/tart-2.41.1/`, `vm/` (Tart's `TART_HOME`) |
| `OUT` | a scratch folder for this run: `bin/`, `share/`, `logs/`, `hostprobe/`, `evidence/` |
| `VM` | the throwaway clone (default `rig-s4`) |
| `BASE` | the image it is cloned from (default `rig-base-2`) |

## Tools

| File | What |
|---|---|
| `build-tools.sh <dir>` | builds `hoststate`, `displaylog` (host probes, read only) and `guestctl`, `libguestactivate.dylib` (guest only; they refuse to run outside a VM) |
| `guest/guestctl.m` | real HID keys and pointer in the guest session; the AX tree (`ax`, `axall` with every attribute, settable flags, parameterised attribute names and actions, element paths); `node`, `text` (AXStringForRange, AXAttributedStringForRange, AXLineForIndex, AXRangeForLine, AXBoundsForRange, AXRangeForPosition, AXRangeForIndex, AXStyleRangeForIndex), `actions`, `act` (AXUIElementPerformAction), `set`, `observe` (an AXObserver on the app, one JSON line per notification), `focused` (the app's AXFocusedUIElement), `axfocus` (every element whose own AXFocused is true), `pb`/`pbclear` (the guest pasteboard), `trust`; elements are `app`, `focused`, a child-index path `0.0.3`, or `find:<AXRole>:<label>` |
| `vm/up.sh`, `vm/down.sh` | clone `$BASE` to `$VM` and boot it headless (`--no-graphics`, no audio, no clipboard); stop and delete it. Both take a host probe; `up.sh` refuses on battery and starts `vm/powerwatch.sh` |
| `vm/powerwatch.sh` | while the guest runs, looks at `pmset -g batt` every 10 minutes and on battery writes `$OUT/logs/$VM-on-battery`; `gate.py` and `checks.py` read it before and after each check and then stop the guest (`vm/down.sh`) and end the run |
| `vm/install.sh <file>...` | copies binaries into the guest's `/usr/local/rig` over `tart exec -i` |
| `vm/shot.sh <name> [x y w h]` | `screencapture` inside the guest, copied back to `$OUT/evidence/<name>.png` |
| `vm/hostcheck.sh <label>`, `vm/hostcompare.py` | host probe record and its check: no tart window on screen, tart never frontmost, every tart process `prohibited` |
| `keywalk.py <pid> <name> <key>...` | real keys; after each, the focused node (`axfocus`), the app's AXFocusedUIElement and a screenshot |
| `vo.sh <name> <step>...` | VoiceOver driven by real keys (`"124 ctrl,alt"` is VO-Right); after each step the last spoken phrase through AppleScript and a caption-panel screenshot |
| `axsum.py` | one line per element of an `ax` dump |

Japanese input: in a clone of `rig-base-2` (still true at 057edbae: click the input menu at (1183, 13) in the guest, then "Japanese" at (1234, 72), with the app frontmost), `guestctl input-select` marks the Japanese source selected but the input method process never starts and romaji arrives as plain letters (TextEdit too). Selecting "Japanese" from the input menu in the menu bar with real clicks starts it; after that `input-select` works for the session.

`checks.py <run> <part>...` runs the shell and chat milestone's rows (helpers in `macpass.py`): each part launches `mcbench-ui-dev --scratch /tmp/s4 --log /tmp/ui.log` with its own flags, waits until the window's content is in the AX tree (the first AX question turns AccessKit on; with `--running chat-gate` it took up to about 18 s), selects the US layout, and writes `$OUT/evidence/<run>/<part>/checks.json` (one record per check with what was read, notes, shots). `pass.sh <run> <part>...` is one guest session (boot, install, the input method started, the parts, the guest stopped and deleted); its header lists the four commands that rerun the whole pass. Parts: `sidebar` (the Build list with `--running chat-gate`: names with status, the open row's `open` description, the running row's disabled Archive build, walking without opening, Space and Enter, search and Escape, archiving the open chat), `archive` (the archived section, Restore, the two-press delete), `filters` (the menu's focus rules, chips), run in the guest at 057edbae; `palette`, `bench`, `chat` (a full streamed turn, the lightbox, Stop, edit and branch, the selection chip, drafts, joining, a refused send), `attach` (the open panel by real keys, paste, the limits), `pickers` and `gallery` (`--gallery`), `keys` (shortcuts, Escape's order, typing to the composer), `window` (drags, full screen, minimise, quit and restore with `--preferences`), `settings`, `fatal`, `crash`, `lag` (open question (e)), and `vo-sidebar`, `vo-chat`, `vo-pickers` (VoiceOver's phrases); `frame` (the first window inside the visible area, three restarts), `focus` (focus never lost, with VoiceOver), `held` (`--setup-pending`), `hidden` (rows outside the transcript's viewport), `sets` (set sizes spoken, the notice's Tab place, `--search-fails`), `pulse` (the running dot rests while the window is inactive), `frames` (seconds from an AXPress to the screen), `shots` (the four screenshots). `collect.sh <run> <dest>` copies a run's text records into `evidence/`.

Frame pacing in the guest: with the default `CAMetalDisplayLink` frame source the window can stop drawing (and the AX tree stop changing) for 10 to 60 s after launch and now and then later; parts that read the tree right after an action then fail. Over 12 launches the content reached AX after 9 to 58 s with it and 7.4 to 7.6 s (the harness's own waits) with the older `CVDisplayLink` path (an uncommitted experiment switch, `EXPERIMENT_CVDISPLAYLINK`, in a scratch build; `APP_ENV` passes it). The report says which parts ran on which. On the Mac's real hardware (the rig's virtual display, 12 launches with each source and a three-minute streaming turn) neither source ever left a change unpainted for over 1 s: the freezes are the guest's paravirtual display, not the app. Speed rows on the host's virtual display are started by `launchctl bootstrap gui/<uid>` of a transient plist with `ProcessType = Interactive` and ended with `launchctl bootout`, because a `launchctl submit` job runs at priority 20 with its timers coalesced by 50 to 200 ms (`ps -M`: 20 for the job and the app under submit, 31 for the job and 46 for the app's main thread under the plist).

`gate.py <run> <check>...` runs the contract's checks against `mcbench-ui-dev --fixture gate` (launch, tree, text, actions, events, keys, select, ime, cache, window, vo); `guestctl search` runs AXUIElementsForSearchPredicate as VoiceOver does.

Start VoiceOver with `guestctl key 96 cmd,fn` (Cmd-F5 needs the fn flag); wait about 8 s before the first AppleScript question, which blocks while VoiceOver starts. Phrases read about 0.5 s after a key are the item; after about 1.2 s VoiceOver has replaced them with its hint.

## The base image `rig-base-2`

Made from `rig-base` (never modified) on 2026-11-17. Steps, in a clone booted with `vm/up.sh` (`BASE=rig-base`), then shut down from inside (`sudo /sbin/shutdown -h now`; `tart stop` lost unflushed preferences once) and renamed to `rig-base-2`:

| Step | How |
|---|---|
| Guest tools | `vm/install.sh $OUT/bin/guestctl $OUT/bin/libguestactivate.dylib` (reinstalled every session anyway) |
| Dock hidden | `defaults write com.apple.dock autohide -bool true; killall Dock` |
| Japanese input | `guestctl input-enable com.apple.inputmethod.Kotoeri.RomajiTyping` and `...RomajiTyping.Japanese` |
| No window restore at login | `defaults write NSGlobalDomain NSQuitAlwaysKeepsWindows -bool false`, `defaults write com.apple.loginwindow TALLogoutSavesState -bool false`, quit Terminal |
| Display 1440x900 at scale 2 | System Settings > Displays, a real click on "1440 x 900 (Default)" (the clone booted at 1024x768 at scale 2); `guestctl modes` shows the current mode |
| Screen capture confirmation | the first `screencapture` raised "tart-guest-agent is requesting to bypass the system private window picker"; a real click on Allow |
| VoiceOver scripting | `defaults write com.apple.VoiceOver4/default SCREnableAppleScript -bool true` and `sudo touch /private/var/db/Accessibility/.VoiceOverAppleScriptEnabled` (SIP is off in the image) |
| VoiceOver welcome | Cmd-F5, a real click on "Do not show this message again", then "Use VoiceOver"; Cmd-F5 again to turn it off before shutdown (`com.apple.universalaccess voiceOverOnOffKey` 0) |
| Automation for VoiceOver | the first `osascript` to VoiceOver raised "tart-guest-agent wants access to control VoiceOver"; a real click on Allow (user TCC row `kTCCServiceAppleEvents`, `com.apple.VoiceOver`, allowed) |

Proof that a clone has it: `vm/up.sh`, then in the guest `guestctl modes` (current 1440x900, 2880x1800 pixels), `defaults read com.apple.dock autohide` (1), the three Kotoeri sources enabled, VoiceOver off at boot, no Terminal, and Cmd-F5 followed by the AppleScript phrase and a `screencapture` with no prompt.

## A session

```sh
export RIG_HOME=... OUT=... VM=rig-s4
./build-tools.sh "$OUT/bin"
vm/up.sh && vm/install.sh "$OUT/bin/guestctl" "$OUT/bin/libguestactivate.dylib" <app binary>
. vm/env.sh; gx sh -c '/usr/local/rig/<app> > /tmp/app.log 2>&1 &'
gx /usr/local/rig/guestctl ax <pid> > "$OUT/evidence/ax.jsonl"; python3 -I axsum.py "$OUT/evidence/ax.jsonl"
python3 -I keywalk.py <pid> walk tab tab shift+tab space
vm/down.sh
```

The guest runs only while a check uses it; host builds run with the guest stopped.

## Changes for the shell and chat milestone (057edbae)

`gate.py` against the new shell: `--scratch` (now required) and `APP_ARGS`; the activating click lands in the sidebar's empty middle (Settings sits at its foot); the Tab walk is 15 stops each way (composer, Attach file, Resolution, Chat model, Reasoning effort, mcbuild, New chat, Collapse sidebar, Search chats, Filter, the current row, Archive build, Show archived, Settings, the current message); `actions` closes the lightbox its image press opens and brings the second message (with the `Worked` fold) into view before pressing it; chat rows are AXGroup; `events` observes for 40 s; `select` reads the AXStaticText runs on screen and clicks the last visible message for Cmd-A; `vo` tabs over Attach file and the three pickers to `Send message`, then `Stop generating`; crash logs are under `/tmp/s4/logs`. The gate fixture lists no chat running unless `APP_ARGS=--running chat-gate`.
