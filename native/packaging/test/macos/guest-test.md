# The macOS install test in the guest

This is the run that `host-proof.sh` cannot make on the owner's Mac: the page's `curl | sh` command in a fresh macOS guest with Gatekeeper's assessments on, the app started by Launch Services through `open` with no dialog, a screenshot of its first window, an update from a staging feed, `--uninstall`, and the home folder listed before and after. It runs in the rig's guest (`native/crates/ui/verify/macos/`, owned by the stage-four session), in a throwaway clone named `rig-s5` of `rig-base-2`, on mains power, in a turn agreed with stage four. Nothing here starts on the host's screen.

## What it needs first

| Input | Where it comes from |
|---|---|
| Release A | `native/packaging/build/macos.sh <tree> aarch64-apple-darwin <out>` on a Mac on mains, with `MCBENCH_UPDATE_KEY_FILE` the staging `minisign.pub` and `MCBENCH_UPDATE_FEED=http://127.0.0.1:3190/releases/latest/download/latest.json`, at the workspace's version, `A` below, then `native/packaging/release.sh <out> $A <release A> --base http://127.0.0.1:3190 --site http://127.0.0.1:3191 --only macos-aarch64` with `MCBENCH_SIGNING_KEY_FILE` the staging `minisign.key`. The guest is Apple silicon, so only the aarch64 archive is run |
| Release B | The same on a scratch copy of the tree after `native/packaging/set-version.sh $B`, where `B` is the next patch version (2.0.0 gives 2.0.1), so the update has something to install |
| The staging folder | What `native/packaging/test/macos/server.py` serves: `releases/download/v$A/` (release A's `release/`), `releases/download/v$B/` (release B's), `releases/LATEST` (the tag the latest links go to, `v$A` at first), `site/` (release A's `site/`, whose `install.sh` names the staging address) |
| The rig | `RIG_HOME` the rig folder (`mcbench-native-rig` beside the repository), `OUT` a scratch folder for this run, `VM=rig-s5` |

The staging server runs inside the guest, on the guest's own `127.0.0.1:3190` (releases) and `127.0.0.1:3191` (the site), with the staging folder and `server.py` copied in over `tart exec -i`. It runs there because builds A and B have `http://127.0.0.1:3190/releases/latest/download/latest.json` compiled in as their feed and release.sh wrote `http://127.0.0.1:3190` into the staging `install.sh`, and inside the guest that address is the guest itself. A server on the host would be reachable from the guest at the host's side of the guest network (`192.168.64.1`, present while a guest runs; TN3179 lists listening for and accepting incoming TCP connections as needing no local network access, and the Mac's application firewall is off), and the first run's trial fetched from it, but the app would not ask it. The bytes the guest fetches are the release folder's bytes either way, and `curl` in the guest still writes them as a download from a network address.

The walk ran once, on 2026-11-18 in one slice of 4 minutes 22 seconds (rig-s5 started with `--suspendable`; suspend and resume held, see the session's notes): install, first window, the two refusals, the update and the uninstall all passed.

## Steps

All commands run from `native/crates/ui/verify/macos/` on the host. `gx` runs a command in the guest's desktop session as the user `admin`.

```sh
export RIG_HOME=<rig folder> OUT=<scratch>/guest-run VM=rig-s5
A=$(awk '/^\[workspace.package\]/ { p = 1; next } /^\[/ { p = 0 } p && $1 == "version" { gsub(/"/, "", $3); print $3 }' ../../../../Cargo.toml)
B=$(echo "$A" | awk -F. '{ print $1 "." $2 "." $3 + 1 }')
pmset -g batt | head -1                      # must say AC Power
./build-tools.sh "$OUT/bin"
vm/hostcheck.sh s5-before
vm/up.sh
vm/install.sh "$OUT/bin/guestctl"
. vm/env.sh
```

Turn Gatekeeper's assessments back on (the image has them off) and record the state:

```sh
gx sudo spctl --global-enable
gx spctl --status                            # must print: assessments enabled
gx sw_vers
```

Copy the staging folder and the server in and start it:

```sh
tar -C <staging folder> -cf - . | gxi sh -c 'mkdir -p /tmp/s5/staging && tar -C /tmp/s5/staging -xf -'
gxi sh -c 'cat > /tmp/s5/server.py' < ../../../../packaging/test/macos/server.py
gx sh -c 'cd /tmp/s5 && nohup python3 -I server.py /tmp/s5/staging 3190 3191 > /tmp/s5/server.log 2>&1 & echo $! > /tmp/s5/server.pid'
gx cat /tmp/s5/staging/releases/LATEST       # v$A
```

`server.py` reads `releases/LATEST` on every request, so writing another tag into it offers that release to a running app without restarting the server.

The home folder before anything of the app exists:

```sh
gx sh -c 'find "$HOME" -xdev 2> /dev/null | LC_ALL=C sort' > "$OUT/home-before.txt"
gx sh -c 'ls -la "$(getconf DARWIN_USER_TEMP_DIR)" "$(getconf DARWIN_USER_CACHE_DIR)"' > "$OUT/tmp-before.txt"
gx sh -c '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -dump | grep -c io.github.adonis-singh.mcbench-lite' > "$OUT/ls-before.txt" || true
```

The install, as the page shows it. The page's command is `curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh | sh`; here the script comes from the staging site, and release.sh wrote the staging address into it:

```sh
gx sh -c 'cd ~ && curl -fsSL http://127.0.0.1:3191/install.sh | sh' > "$OUT/install.log" 2>&1
gx sh -c 'xattr -lr ~/Applications/mcbench-lite.app' > "$OUT/xattr-installed.txt"
gx sh -c 'codesign --verify --deep --strict --verbose=2 ~/Applications/mcbench-lite.app' > "$OUT/codesign-installed.txt" 2>&1
gx sh -c 'spctl --assess --type execute -vv ~/Applications/mcbench-lite.app' > "$OUT/spctl-installed.txt" 2>&1 || true
gx sh -c 'cat ~/Applications/mcbench-lite.app/Contents/Resources/install-source'
gx sh -c 'cat ~/Applications/mcbench-lite.app/Contents/Resources/version'
```

`install.sh` ends by starting the app with `open`. If its run used `--no-open`, start it the same way: `gx sh -c 'open ~/Applications/mcbench-lite.app'`.

The first window and the absence of a dialog. The window can take 10 to 60 s to draw in the guest (the rig's frame-pacing note), so wait for content in the AX tree, not for a fixed time:

```sh
pid=$(gx pgrep -x mcbench-lite)
for i in $(seq 120); do gx /usr/local/rig/guestctl ax "$pid" > "$OUT/ax-first.jsonl" && [ "$(wc -l < "$OUT/ax-first.jsonl")" -gt 5 ] && break; sleep 1; done
vm/shot.sh s5-first-window
gx sh -c 'pgrep -x CoreServicesUIAgent && /usr/local/rig/guestctl ax "$(pgrep -x CoreServicesUIAgent)"' > "$OUT/coreservicesuiagent.txt" || true
gx sh -c 'log show --last 5m --predicate "process == \"syspolicyd\"" --style compact' > "$OUT/syspolicyd.log"
```

It passes when the screenshot shows the app's window and no "Not Opened" or "Apple could not verify" dialog, `coreservicesuiagent.txt` holds no window, and `syspolicyd.log` shows the launch allowed. `install.log` must show the checksum line `OK`.

Quit the first window with Cmd-Q and wait for the process to end before going on:

```sh
gx /usr/local/rig/guestctl key 12 cmd
for i in $(seq 60); do gx pgrep -x mcbench-lite > /dev/null || break; sleep 1; done
```

The refusals come first, while release A is installed, because a refused update needs a newer version on offer and release B is the only newer one. For each, a tag folder is made from B's, one byte is changed, `LATEST` names it, A is started, and A must make no `.mcbench-lite.app.update` and write the refusal to `update.log`:

```sh
log='~/Library/Application\ Support/mcbench-lite/logs/update.log'
gx sh -c "cd /tmp/s5/staging/releases/download && cp -R v$B v$B-feed && printf x >> v$B-feed/latest.json && cp -R v$B v$B-asset && printf x >> v$B-asset/mcbench-lite-macos-aarch64.tar.gz"
for bad in v$B-feed v$B-asset; do
  gx sh -c "echo $bad > /tmp/s5/staging/releases/LATEST"
  gx sh -c 'open ~/Applications/mcbench-lite.app'
  for i in $(seq 120); do gx sh -c "grep -q . $log" 2> /dev/null && break; sleep 2; done
  sleep 30
  gx sh -c "ls -A ~/Applications; cat $log" > "$OUT/refused-$bad.txt"
  vm/shot.sh "s5-refused-$bad"
  gx /usr/local/rig/guestctl key 12 cmd
  for i in $(seq 60); do gx pgrep -x mcbench-lite > /dev/null || break; sleep 1; done
done
```

Each passes when `refused-<bad>.txt` lists no `.mcbench-lite.app.update` and `update.log` says why the release was refused: a changed `latest.json` does not verify against the compiled-in key (`native/crates/updater/LATEST.md`), and a changed archive does not have the size and sha256 that the signed `latest.json` names.

The update. There is no button: the app checks the feed in the background after its core starts, downloads and verifies the archive, keeps it beside the app, and swaps it in at the next start. Offer release B, start A, and wait for the download and the `latest.json` it was checked against:

```sh
gx sh -c "echo v$B > /tmp/s5/staging/releases/LATEST"
gx sh -c 'open ~/Applications/mcbench-lite.app'
for i in $(seq 300); do gx sh -c 'test -f ~/Applications/.mcbench-lite.app.update && test -f ~/Applications/.mcbench-lite.app.update.json' && break; sleep 2; done
gx sh -c 'ls -lA ~/Applications' > "$OUT/waiting.txt"
vm/shot.sh s5-update-waiting
gx /usr/local/rig/guestctl key 12 cmd
for i in $(seq 60); do gx pgrep -x mcbench-lite > /dev/null || break; sleep 1; done
```

Start it again. This start swaps B in before any window and executes B in its own place:

```sh
gx sh -c 'open ~/Applications/mcbench-lite.app'
pid=$(gx pgrep -x mcbench-lite)
for i in $(seq 120); do gx /usr/local/rig/guestctl ax "$pid" > "$OUT/ax-updated.jsonl" && [ "$(wc -l < "$OUT/ax-updated.jsonl")" -gt 5 ] && break; sleep 1; done
vm/shot.sh s5-after-update
gx sh -c 'cat ~/Applications/mcbench-lite.app/Contents/Resources/version' > "$OUT/version-after-update.txt"
gx sh -c "ps -o pid=,command= -p $pid; shasum -a 256 ~/Applications/mcbench-lite.app/Contents/MacOS/mcbench-lite" > "$OUT/running.txt"
for i in $(seq 60); do gx sh -c 'test ! -e ~/Applications/mcbench-lite.app.old' && break; sleep 2; done
gx sh -c 'ls -lA ~/Applications' > "$OUT/after-update.txt"
gx sh -c 'xattr -lr ~/Applications/mcbench-lite.app' > "$OUT/xattr-updated.txt"
gx sh -c 'codesign --verify --deep --strict --verbose=2 ~/Applications/mcbench-lite.app' > "$OUT/codesign-updated.txt" 2>&1
gx sh -c "cat $log" > "$OUT/update-log.txt"
```

It passes when `version-after-update.txt` holds B's version; `running.txt` shows the running program at `~/Applications/mcbench-lite.app/Contents/MacOS/mcbench-lite` with the sha256 of release B's `mcbench-lite` (from B's archive); `mcbench-lite.app.old`, `.mcbench-lite.app.update` and `.mcbench-lite.app.update.json` are gone a short while after the window; the bundle verifies and carries no `com.apple.quarantine`; `install-source` is still `script`; `update-log.txt` holds no failure; and the window shows with no dialog. Quit it with Cmd-Q as above.

The uninstall, after quitting the app with Cmd-Q:

```sh
gx /usr/local/rig/guestctl key 12 cmd
gx sh -c 'curl -fsSL http://127.0.0.1:3190/install.sh | sh -s -- --uninstall' > "$OUT/uninstall.log" 2>&1
gx sh -c 'find "$HOME" -xdev 2> /dev/null | LC_ALL=C sort' > "$OUT/home-after.txt"
gx sh -c 'ls -la "$(getconf DARWIN_USER_TEMP_DIR)" "$(getconf DARWIN_USER_CACHE_DIR)"' > "$OUT/tmp-after.txt"
gx sh -c '/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -dump | grep -c io.github.adonis-singh.mcbench-lite' > "$OUT/ls-after.txt" || true
diff "$OUT/home-before.txt" "$OUT/home-after.txt" > "$OUT/home-diff.txt" || true
```

It passes when `home-diff.txt` shows only `~/Library/Application Support/mcbench-lite` and what is under it, plus what the uninstall names and leaves on purpose (the person's Claude Code transcripts and Codex sign-in, if a provider was used), and `uninstall.log` names the data folder it left. The files macOS writes for the app outside the data folder (`~/Library/Preferences/io.github.adonis-singh.mcbench-lite.plist`, `~/Library/Saved Application State/io.github.adonis-singh.mcbench-lite.savedState`, the Metal cache under `DARWIN_USER_CACHE_DIR`, the Launch Services entry) must be gone or named in `uninstall.log`.


Stop and clean up:

```sh
gx sh -c 'kill "$(cat /tmp/s5/server.pid)"'
vm/down.sh
vm/hostcheck.sh s5-after
python3 -I vm/hostcompare.py "$OUT/hostprobe/s5-after.json"
```

Keep `$OUT/evidence/*.png` and the text records; delete the clone (`vm/down.sh` does) and tell stage four the turn has ended.
