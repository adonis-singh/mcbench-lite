# plain.sh: plain's desktop session for vm-test.sh and vm-first.sh, sourced
# after hold.sh and hold_boot, with OUT (the results folder) and ME (the
# script's name for its own lines) set. C:\mcb\vm-test.ps1 must be in the
# guest before any step runs.

# step USER STEP [arguments]: one step of guest/vm-test.ps1 over ssh.
step() { local user="$1"; shift; echo "$ME: $*"; bg_wait as "$user" "powershell -NoProfile -ExecutionPolicy Bypass -File C:\\mcb\\vm-test.ps1 $*"; }

# plain_desktop: makes plain the account Windows logs on at boot and reboots,
# twice at most; sets DESKTOP to 1 when plain's desktop runs, else 0.
plain_desktop() {
  # plain becomes the account Windows logs on at boot, so its desktop is the one
  # on the VM's display (QMP's screendump) and its scheduled tasks can run there.
  # bench and plain share the VM's one generated password (winvm/vm/creds.txt);
  # it goes to the guest on ssh's standard input and is never printed. Windows
  # 11 logs a user on automatically only with AutoAdminLogon, DefaultUserName,
  # DefaultDomainName and DefaultPassword set, no AutoLogonCount left, and
  # PasswordLess\Device's DevicePasswordLessBuildVersion 0; AutoLogonSID, when
  # set, can keep the earlier account (the two runs of 2026-11-18 kept bench).
  password="$(sed -n 's/^password=//p' "$WINVM/vm/creds.txt")"
  [ -n "$password" ] || fail "$WINVM/vm/creds.txt has no password= line."
  # Run in the foreground: bash gives a command started with & an empty
  # standard input, so bg_wait would send the script nothing.
  set_logon() {
    as bench 'powershell -NoProfile -Command -' <<PS
\$w = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty \$w AutoAdminLogon '1'
Set-ItemProperty \$w DefaultUserName 'plain'
Set-ItemProperty \$w DefaultDomainName \$env:COMPUTERNAME
Set-ItemProperty \$w DefaultPassword '$password'
Set-ItemProperty \$w LastUsedUsername 'plain'
foreach (\$v in 'AutoLogonSID', 'AutoLogonCount') { Remove-ItemProperty \$w \$v -ErrorAction SilentlyContinue }
\$d = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\PasswordLess\Device'
New-Item -Force \$d | Out-Null
Set-ItemProperty \$d DevicePasswordLessBuildVersion 0 -Type DWord
PS
  }
  # What decides the automatic logon, without the password itself.
  logon_state() {
    as bench 'quser 2>&1; $w = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon"; "AutoAdminLogon={0} DefaultUserName={1} DefaultDomainName={2} DefaultPassword set={3} AutoLogonSID={4} AutoLogonCount={5} LastUsedUsername={6}" -f $w.AutoAdminLogon, $w.DefaultUserName, $w.DefaultDomainName, [bool]$w.DefaultPassword, $w.AutoLogonSID, $w.AutoLogonCount, $w.LastUsedUsername; "DevicePasswordLessBuildVersion={0}" -f (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\PasswordLess\Device" -ErrorAction SilentlyContinue).DevicePasswordLessBuildVersion' || true
  }
  plain_on() { as bench '@(Get-Process explorer -IncludeUserName -ErrorAction SilentlyContinue | Where-Object UserName -like "*\plain").Count' 2>/dev/null | tr -d '\r' | grep -qx '[1-9]'; }
  # Two tries, each a reboot and up to 240 s for plain's explorer.exe. Without
  # plain's desktop the run does what needs none (install, files, entry,
  # uninstall, listing) and says that the first window as plain is not proven.
  DESKTOP=0
  for try in 1 2; do
    set_logon
    logon_state
    as bench 'shutdown /r /t 0' || true
    sleep 15
    wait_ssh 900
    t0=$SECONDS
    until plain_on; do (( SECONDS - t0 < 240 )) || break; sleep 3; done
    if plain_on; then DESKTOP=1; break; fi
    echo "$ME: try $try: plain's desktop did not start within 240 s of the reboot"
    screenshot "$OUT/0-no-plain-desktop-$try.png"
    logon_state
  done
  unset password
  if [ "$DESKTOP" = 1 ]; then
    sleep 20  # the first logon's animation and Explorer's start
    screenshot "$OUT/0-plain-desktop.png"
  else
    echo "$ME: NOT PROVEN: no interactive session of plain; the first window as plain, the refusals while it runs and the update are left out"
  fi
}

# settle_before: the "before" listing, taken on a settled profile.
settle_before() {
  # plain's first logon keeps writing its profile for minutes (OneDrive's
  # per-user install, the Store's packages, Start's tiles); the "before" listing
  # waits until two listings 60 s apart are equal (the digest that ends the
  # listing's line, over every path and key that is not Windows' own), for at
  # most 10 minutes, so that churn is not counted as the app's.
  t_settle=$SECONDS
  prev=""
  while :; do
    cur="$(step plain listing before | tail -1)"
    echo "$cur"
    if [ "${cur##* }" = "${prev##* }" ]; then echo "$ME: the before listing equals the one before it, $((SECONDS - t_settle)) s after the first"; break; fi
    (( SECONDS - t_settle + 60 <= 600 )) || { echo "$ME: plain's profile was still changing after $((SECONDS - t_settle)) s; the before listing is the last one"; break; }
    prev="$cur"
    sleep 60
  done
}

# shot NAME: the app's window given the focus (the Start menu closed first),
# then the screen.
shot() {
  step bench desktop front || echo "$ME: the app's window could not be given the focus; $1 shows what is in front"
  screenshot "$OUT/$1"
}
# start_window NAME [EXE]: the window step; when it fails, the screen is kept
# as no-window-NAME.png (the fatal screen's sentence is only on the screen).
# When the window is the fatal screen ("stopped" title), its screen and
# server.pid's content are kept first; then the test deletes the stale
# server.pid, says so, and starts once more as NAME-retry.
start_window() {
  local name="$1" out rc=0
  out="$(step bench desktop window "$@")" || rc=$?
  echo "$out"
  [ "$rc" = 0 ] && return 0
  screenshot "$OUT/no-window-$name.png"
  [[ "$out" == *"fatal screen"* ]] || return 1
  echo "$ME: STOPPED: start $name showed the fatal screen; the screen is no-window-$name.png; server.pid is deleted next so the test can go on"
  step bench desktop quit if-running || return 1
  step plain stale-lock || return 1
  step bench desktop window "$name-retry" "${@:2}"
}
