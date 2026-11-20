# C:\mcb\vm-test.ps1 STEP [arguments]: one step of vm-test.sh in the Windows guest.
# Windows PowerShell 5.1. Each step prints what it found and exits 0, or prints
# one sentence and exits 1. Results go to C:\mcb\results.
#
# | Step | Runs as | Does |
# |---|---|---|
# | listing NAME | plain over ssh | the user's profile (every path under %USERPROFILE%) and HKCU (every key path, from reg export) into NAME.files and NAME.keys; its line ends with a digest of the paths that are not Windows' own, so two equal lines are two equal listings |
# | compare BEFORE AFTER | plain over ssh | what AFTER has that BEFORE has not and the reverse, every line into compare-full.txt; prints the lines whose path holds mcbench (in any case) and a count of the others by class; fails on a path left that is not the data folder %LOCALAPPDATA%\mcbench-lite or Windows' own (the table $WindowsOwn) |
# | install INSTALLER EXPECT | plain over ssh | runs the installer with /S, timed; checks the install folder holds exactly EXPECT's files with EXPECT's sha256 plus install-source ("direct") and uninstall.exe, that the payload's version file reads DisplayVersion, the Start menu shortcut and the uninstall entry |
# | refused INSTALLER | plain over ssh, app running | the installer and the uninstaller each stop with exit code 2 and a sentence, and the install folder is unchanged |
# | copy-direct | plain over ssh | copies the installed payload into C:\mcb\copy\mcbench-lite with install-source "direct": a second copy of the app outside the installer's folder |
# | as-is EXPECT DIR VERSION | plain over ssh | the payload in DIR is EXPECT's, DIR\version reads VERSION, and no download waits beside it (no .mcbench-lite.update or .mcbench-lite.update.json in DIR's parent); prints the .mcbench-lite* entries that are there |
# | wait-download DIR | plain over ssh (DIR: a folder, or installed for the installer's) | waits up to 120 s for the updater's waiting download beside the payload folder DIR (.mcbench-lite.update and .mcbench-lite.update.json in DIR's parent) |
# | updated EXPECT DIR VERSION | plain over ssh, after the start that swapped | the payload in DIR is EXPECT's (the newer release), DIR\version reads VERSION, a running mcbench-lite.exe is DIR\mcbench-lite.exe and its file's sha256 is EXPECT's; then waits up to 60 s until no .old entry is left in DIR; no .mcbench-lite.update* entry may be left in DIR's parent |
# | display-version same|VERSION | plain over ssh | the uninstall entry's DisplayVersion is the one the install wrote (same, read after 10 s) or VERSION (waited for up to 60 s) |
# | log-mark NAME | plain over ssh | records how many lines the data folder's logs\update.log has, as the mark NAME |
# | log-new NAME value|byte | plain over ssh | waits up to 120 s for a line after the mark NAME in update.log that holds the updater's sentence for that refusal ($Refusals), and prints the new lines |
# | log-none NAME | plain over ssh | update.log has no line after the mark NAME |
# | uninstall | plain over ssh | runs the uninstall entry's QuietUninstallString, timed; checks the install folder, shortcut and entry are gone and the data folder stays |
# | desktop STEP [arguments] | bench over ssh | runs STEP (window, wsl-screen, front, quit or ui-log) in plain's desktop session through a one-shot scheduled task (an ssh session has no desktop) and prints its log |
# | window NAME [EXE] | plain's desktop | prints what the data folder's server.pid holds, starts the app from its Start menu shortcut (or EXE) and waits for its first window, timed into window-NAME.time; fails when that window is the fatal screen (its title ends with "stopped") |
# | wsl-screen | plain's desktop | finds the setup screen's WSL sentence in the app's window through UI Automation |
# | front | plain's desktop | before a screenshot: presses Escape while another program has the keyboard focus (the Start menu, StartMenuExperienceHost, closes on Escape), then gives the app's window the focus through UI Automation and checks that it has it |
# | quit [if-running] | plain's desktop | closes the app's window and waits up to 30 s for the app to exit; then stops it by pid and says so; prints what server.pid holds after the quit; with if-running, no running app is not a failure |
# | stale-lock | plain over ssh | after a fatal screen only: prints what server.pid holds, copies it to results\stale-server.pid and deletes it from the data folder |
# | ui-log | plain's desktop | starts the installed app with MCBENCH_UI_LOG naming a file in %TEMP%, waits for its window and 10 s more, closes it, then checks the file is JSON lines, prints its renderer, fonts and first_frame lines (or that it has no first_frame line), copies it to results\ui-log.jsonl and deletes it |
# | check-install EXPECT | plain over ssh | install's checks of the install folder, shortcut and entry, after an install a person ran |
# | first-start N keep|close | plain's desktop | a true first start: removes the data folder, starts the shortcut's target with MCBENCH_UI_LOG for that process, waits for the first_frame line and prints since_start_ms and epoch_ms minus the instant before the start, the renderer and fonts lines; then closes the app (close) and prints what server.pid holds, or leaves it running (keep) |
# | first-summary | plain over ssh | each first start's stderr [renderer] line, the median, lowest and highest of both times, the VM's processor count |
# | edge URL | plain's desktop | Microsoft Edge opens URL; the installer must arrive in Downloads within 90 s; prints the top-level windows and the file's mark of the web |
# | edge-close | plain's desktop | closes Edge |
# | motw INSTALLER | plain over ssh | SIMULATED download: copies INSTALLER into Downloads with a Zone.Identifier of ZoneId 3 |
# | open-download | plain's desktop | starts the downloaded installer by ShellExecute, as a double-click does, and prints the top-level windows 8 s later |
# | find KEY SECONDS | plain's desktop | waits for an element whose name is like one of $UiText[KEY] |
# | click KEY SECONDS | plain's desktop | invokes the enabled button, link or menu item named like one of $UiText[KEY]; when there is none, prints every control's name |
# | raise KEY | plain's desktop | gives the focus to the top-level window whose name is like one of $UiText[KEY] and prints its texts |
# | escape | plain's desktop | before a screenshot: Escape while the Start menu or search has the focus, then the focus to the desktop, which closes the Start menu |
# | downloaded | plain over ssh | Edge's download is in Downloads under its own name; prints its mark of the web |
# | open-explorer | plain's desktop | Explorer opens the downloaded installer, as a double-click does, and the top-level windows 8 s later are printed |
# | ps-install NAME BASE CODE | plain's desktop | `irm BASE/install.ps1 \| iex` in a PowerShell window of its own with MCBENCH_LITE_RELEASES=BASE, its transcript printed; the exit code it leaves must be CODE; the window stays open |
# | ps-close NAME | plain's desktop | stops that PowerShell window by its recorded pid |
# | not-installed | plain over ssh | no install folder, shortcut, entry or temporary folder of install.ps1 |
# | forget-download | plain over ssh | deletes the downloaded installer |
# | consoles | plain's desktop | every console window with its title and process, and every top-level window |
# | wsl-turn | plain's desktop | headless setup, a chat, one build and two bash calls through the installed program's headless turn on the app's data folder, each answer printed, the first bash timed; the files of sandbox-assets and wsl --list |
# | finish-setup SECONDS | plain's desktop | waits for the install's uninstall entry, then presses Enter (Close, the default button) while the installer has the focus, and the installer must exit |
# | console-text | plain's desktop | every console window's text (Windows Terminal or conhost): its first 40 lines and how many hold [renderer] |
# | close-console | plain's desktop | closes every console window (Windows Terminal or conhost) as its close button does |
# | app-alive | plain's desktop | a window of mcbench-lite.exe is shown |
# | ps-direct | plain's desktop | a PowerShell window of its own runs the installed mcbench-lite.exe directly; the app's window must show and the prompt come back within 60 s |
# | app-logs | plain over ssh | copies the data folder's logs\ to results\app-logs and prints update.log |
param([Parameter(Mandatory, Position = 0)][string]$Step, [Parameter(ValueFromRemainingArguments)][string[]]$Rest = @())
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Root = 'C:\mcb'
$Results = "$Root\results"
$InstDir = "$env:LOCALAPPDATA\Programs\mcbench-lite"
$DataDir = "$env:LOCALAPPDATA\mcbench-lite"
$Shortcut = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\mcbench-lite.lnk"
$UninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\mcbench-lite'
$CopyDir = "$Root\copy\mcbench-lite"
# What Windows itself writes in a profile and in HKCU while a user is logged on
# and runs any program; a path under one of these left after uninstall is
# printed as Windows' own and does not fail the comparison. From the stand-in
# run of 2026-11-17: Explorer's and PowerShell's caches, the print spooler's
# per-user Devices and PrinterPorts keys, and the Internet zone keys that
# Windows' URL security manager makes the first time a program of the user
# asks for a file's zone (who asked was not traced). Start's TileProperties
# keys, which name a program's path, are written by the Start menu when a
# program is launched from it; neither the app nor the installer makes them
# (the coordinator's decision of 2026-11-18), and a path holding mcbench is
# still printed with its class.
$WindowsOwn = @(
  'NTUSER.DAT', 'ntuser.dat.LOG', 'ntuser.ini',
  'AppData\Local\Microsoft\Windows\',
  'AppData\Roaming\Microsoft\Windows\Recent\',
  'AppData\Local\D3DSCache\',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Explorer\',
  'HKEY_CURRENT_USER\Software\Classes\Local Settings\',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\Devices',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\PrinterPorts',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Lockdown_Zones',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings\ZoneMap',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Internet Settings\Zones',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Start\TileProperties\',
  # Windows Terminal's package files and its activity key: the app is still a
  # console program, so its console window opens in Terminal; they go when
  # mcbench-lite.exe is a window program (the run of 2026-11-18 07:24).
  'AppData\Local\Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\HostActivityManager\',
  'HKEY_CURRENT_USER\Console\%%Startup',
  # Windows' own background work in any session (the same run): the shell's
  # web view caches, the token broker's cache, Storage Sense, the rules
  # engine and Defender's notification settings.
  'AppData\Local\Packages\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\',
  'AppData\Local\Microsoft\TokenBroker\',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\StorageSense',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RulesEngine',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\Windows.Defender.SecurityCenter',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\PushNotifications\Backup\Windows.Defender.SecurityCenter',
  # What a new account's setup keeps adding after the before-listing settled
  # (the run of 2026-11-18 20:23, none of it naming the app): Microsoft's
  # per-user packages (Widgets, the Start experiences, the web experience)
  # and their class, COM and background-task registrations, notifications,
  # the cloud store, capability and background access, and the certificate
  # cache.
  'AppData\Local\Packages\Microsoft.',
  'AppData\Local\Packages\MicrosoftWindows.',
  'AppData\LocalLow\Microsoft\CryptnetUrlCache\',
  'HKEY_CURRENT_USER\Software\Classes\AppX',
  'HKEY_CURRENT_USER\Software\Classes\PackagedCom\Package\Microsoft.',
  'HKEY_CURRENT_USER\Software\Classes\Extensions\ContractId\Windows.BackgroundTasks',
  'HKEY_CURRENT_USER\Software\Classes\ms-startfeeds',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\Shell\Associations',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CloudStore',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Notifications',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\PushNotifications',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\AppHost',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\RunNotification',
  'HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Dsh'
)
# What the test's own use of Microsoft Edge leaves (vm-first.sh downloads the
# installer with it): Edge's profile and settings, printed as the test's.
$TestOwnStart = [regex]'^(?:AppData\\Local\\Microsoft\\Edge\\|AppData\\Local\\Microsoft\\Edge$|HKEY_CURRENT_USER\\Software\\Microsoft\\Edge)'
# The NSIS uninstaller runs from a copy of itself in %TEMP%\~nsu<n>.tmp\Un.exe,
# which it cannot delete while it runs; it is the uninstaller's own leftover,
# printed as such.
$NsisCopy = '^AppData\\Local\\Temp\\~nsu[0-9A-Za-z]*\.tmp(\\Un\.exe)?$'
# A path is Windows' own when it begins with one of $WindowsOwn (case kept),
# as one regular expression: the listing tests every path of the profile.
$WindowsOwnStart = [regex]('^(?:' + (($WindowsOwn | ForEach-Object { [regex]::Escape($_) }) -join '|') + ')')
function Is-WindowsOwn([string]$Path) { $WindowsOwnStart.IsMatch($Path) }
# The updater's sentences for the two refusals vm-test.sh serves while A is
# installed (native/crates/updater/src/latest.rs): a latest.json whose
# entry's size differs from its signed comment, and an archive with one byte
# changed, whose sha256 is not the one latest.json gives.
$Refusals = @{
  'value' = 'does not match its signed comment'
  'byte'  = 'it is not the released file'
}
$UpdateLog = "$DataDir\logs\update.log"
function Update-Lines { if (Test-Path -LiteralPath $UpdateLog) { @(Get-Content -LiteralPath $UpdateLog) } else { @() } }
function Read-Mark([string]$Name) { [int](Get-Content -LiteralPath "$Results\update-$Name.mark") }
# The data folder's one-instance lock (native/crates/setup/src/instance.rs):
# server.pid holds the pid of the app that took it; a start refuses while a
# live process has that pid.
function Lock-State {
  $file = "$DataDir\server.pid"
  if (-not (Test-Path -LiteralPath $file)) { return 'the data folder has no server.pid' }
  $holder = ([IO.File]::ReadAllText($file)).Trim()
  $p = Get-Process -Id ([int]$holder) -ErrorAction SilentlyContinue
  if ($p) { "server.pid holds $holder, the pid of the running process $($p.ProcessName)" } else { "server.pid holds $holder, which no running process has" }
}

# A payload folder argument: the word installed names the installer's folder.
function Payload-Dir([string]$Arg) { if ($Arg -eq 'installed') { $InstDir } else { $Arg } }
function Fail([string]$Sentence) { Write-Output "vm-test.ps1 $Step`: $Sentence"; exit 1 }
trap { Fail $_.Exception.Message }
New-Item -ItemType Directory -Force $Results | Out-Null

# sha256 and name of every file directly in a folder, sorted by name.
function Folder-Hashes([string]$Path) {
  Get-ChildItem -LiteralPath $Path -Force -File | Sort-Object Name | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
  }
}
# EXPECT files: sha256sum lines "<sha256>  <name>" of the payload, made on the host.
function Read-Expect([string]$Path) { @(Get-Content -LiteralPath $Path | Where-Object { $_ } | Sort-Object { ($_ -split '\s+', 2)[1] }) }
# The install folder holds exactly EXPECT's files with EXPECT's sha256 plus
# install-source ("direct") and uninstall.exe; the payload's version file
# reads DisplayVersion; the Start menu shortcut and the uninstall entry are
# the installer's. Answers what it found, in words.
function Check-Install([string]$Expect) {
  $want = @(Read-Expect $Expect) + @(
    '{0}  install-source' -f (Get-FileHash -InputStream ([IO.MemoryStream]::new([Text.Encoding]::ASCII.GetBytes("direct`n"))) -Algorithm SHA256).Hash.ToLower())
  $have = @(Folder-Hashes $InstDir | Where-Object { -not $_.EndsWith('  uninstall.exe') })
  $diff = Compare-Object ($want | Sort-Object { ($_ -split '\s+', 2)[1] }) $have
  if ($diff) { Fail "the install folder differs from the payload: $(($diff | ForEach-Object { "$($_.SideIndicator) $($_.InputObject)" }) -join '; ')" }
  if (-not (Test-Path -LiteralPath "$InstDir\uninstall.exe")) { Fail 'uninstall.exe is missing.' }
  $dirs = @(Get-ChildItem -LiteralPath $InstDir -Force -Directory)
  if ($dirs.Count) { Fail "the install folder holds folders: $(($dirs | ForEach-Object { $_.Name }) -join ', ')." }
  $lnk = (New-Object -ComObject WScript.Shell).CreateShortcut($Shortcut)
  if (-not (Test-Path -LiteralPath $Shortcut) -or $lnk.TargetPath -ne "$InstDir\mcbench-lite.exe") { Fail "the Start menu shortcut $Shortcut is missing or does not start $InstDir\mcbench-lite.exe." }
  $desktopLnk = "$([Environment]::GetFolderPath('Desktop'))\mcbench-lite.lnk"
  if (Test-Path -LiteralPath $desktopLnk) { Fail "a desktop shortcut exists ($desktopLnk)." }
  $e = Get-ItemProperty -LiteralPath $UninstallKey
  foreach ($pair in @(@('DisplayName', 'mcbench-lite'), @('Publisher', 'adonis-singh'), @('InstallLocation', $InstDir),
      @('UninstallString', "`"$InstDir\uninstall.exe`""), @('QuietUninstallString', "`"$InstDir\uninstall.exe`" /S"),
      @('DisplayIcon', "$InstDir\uninstall.exe,0"), @('NoModify', 1), @('NoRepair', 1))) {
    if ($e.($pair[0]) -ne $pair[1]) { Fail "the uninstall entry's $($pair[0]) is '$($e.($pair[0]))', not '$($pair[1])'." }
  }
  if (-not $e.DisplayVersion -or -not $e.EstimatedSize) { Fail 'the uninstall entry has no DisplayVersion or EstimatedSize.' }
  $e.DisplayVersion | Set-Content "$Results\installed-version.txt"
  $fileVersion = [IO.File]::ReadAllText("$InstDir\version")
  if ($fileVersion -ne "$($e.DisplayVersion)`n") { Fail "the payload's version file holds '$fileVersion', not DisplayVersion $($e.DisplayVersion) and a newline." }
  "{0} files; DisplayVersion {1}, EstimatedSize {2} KB; shortcut and entry present" -f (@($have).Count + 1), $e.DisplayVersion, $e.EstimatedSize
}
# The texts the find and click steps look for, by a key without spaces (a
# step's arguments are split at spaces on their way to the guest).
$UiText = @{
  'smartscreen' = @('*Windows protected your PC*')
  'unreachable' = @('*be reached*')
  'more-info'   = @('More info')
  'run-anyway'  = @('Run anyway')
  'run'         = @('Run', '&Run')
  'close'       = @('Close')
  'warning'     = @('*Security Warning*')
  'downloads'   = @('Downloads*')
  'more'        = @('More actions*', 'More options*')
  'keep'        = @('Keep')
  'show-more'   = @('Show more')
  'keep-anyway' = @('Keep anyway')
}
function Load-Automation { Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes }
# Every top-level window: its name, process and class.
function Top-Windows {
  Load-Automation
  $A = [System.Windows.Automation.AutomationElement]
  foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
    $p = Get-Process -Id $w.Current.ProcessId -ErrorAction SilentlyContinue
    "'{0}' ({1}, {2})" -f $w.Current.Name, $p.ProcessName, $w.Current.ClassName
  }
}
# The first element of any top-level window whose name is like one of the
# PATTERNS (-like); with $Control, only an enabled button, link or menu item
# that is not a title bar's own button (AutomationId Close, Minimize,
# Maximize).
function Find-Element([string[]]$Patterns, [bool]$Control) {
  Load-Automation
  $A = [System.Windows.Automation.AutomationElement]
  $T = [System.Windows.Automation.ControlType]
  foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
    foreach ($e in @($w) + @($w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition))) {
      $c = $e.Current
      if (-not $c.Name -or -not @($Patterns | Where-Object { $c.Name -like $_ }).Count) { continue }
      if (-not $Control) { return $e }
      if ($c.IsEnabled -and ($c.ControlType -eq $T::Button -or $c.ControlType -eq $T::Hyperlink -or $c.ControlType -eq $T::MenuItem -or $c.ControlType -eq $T::SplitButton) -and $c.AutomationId -notin 'Close', 'Minimize', 'Maximize') { return $e }
    }
  }
  $null
}
# The names of every enabled button, link and menu item of every top-level
# window, each once: what a click step could have pressed.
function Control-Names {
  Load-Automation
  $A = [System.Windows.Automation.AutomationElement]
  $seen = @{}
  foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
    foreach ($e in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
      $c = $e.Current
      $k = "$($c.ControlType.ProgrammaticName -replace 'ControlType.', ''): $($c.Name) (in '$($w.Current.Name)')"
      if ($c.Name -and $c.ControlType.ProgrammaticName -match 'Button|Hyperlink|MenuItem|Text' -and -not $seen.ContainsKey($k)) { $seen[$k] = 1; $k }
    }
  }
}
function App-Processes { @(Get-Process -Name 'mcbench-lite' -ErrorAction SilentlyContinue) }
# consent.exe is Windows' administrator prompt (UAC); it must never run during install or start.
function No-Admin-Prompt { if (@(Get-Process -Name consent -ErrorAction SilentlyContinue).Count) { Fail 'Windows is asking for an administrator (consent.exe runs).' } }
# The owner's sentence on the setup screen when WSL is not installed (docs/native-app.md, Sentences approved by the owner).
$WslSentence = 'To use bash and build, Windows needs WSL.'
function Run-Timed([string]$File, [string[]]$Arguments, [string]$ErrFile) {
  $watch = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process -FilePath $File -ArgumentList $Arguments -Wait -PassThru -RedirectStandardError $ErrFile
  $watch.Stop()
  @{ Code = $p.ExitCode; Ms = $watch.ElapsedMilliseconds; Err = (Get-Content -Raw -LiteralPath $ErrFile) }
}

switch ($Step) {
  'listing' {
    $name = $Rest[0]
    $profileDir = $env:USERPROFILE
    # Junctions such as "Application Data" deny listing; they are Windows' and are skipped.
    Get-ChildItem -LiteralPath $profileDir -Recurse -Force -ErrorAction SilentlyContinue |
      ForEach-Object { $_.FullName.Substring($profileDir.Length + 1) } | Sort-Object |
      Set-Content -Encoding UTF8 "$Results\$name.files"
    $reg = "$Results\$name.reg"
    & reg.exe export HKCU $reg /y | Out-Null
    if ($LASTEXITCODE -ne 0) { Fail "reg export HKCU failed (exit code $LASTEXITCODE)." }
    Get-Content -LiteralPath $reg | Where-Object { $_ -match '^\[(.+)\]$' } | ForEach-Object { $Matches[1] } | Sort-Object |
      Set-Content -Encoding UTF8 "$Results\$name.keys"
    Remove-Item -LiteralPath $reg
    $files = @(Get-Content "$Results\$name.files")
    $keys = @(Get-Content "$Results\$name.keys")
    # vm-test.sh waits for two equal listings 60 s apart before it installs;
    # Windows' own places change on every logon and are left out of the digest,
    # as the comparison leaves them out of its verdict.
    $bytes = [Text.Encoding]::UTF8.GetBytes((@($files + $keys | Where-Object { -not (Is-WindowsOwn $_) }) -join "`n"))
    $digest = (Get-FileHash -InputStream ([IO.MemoryStream]::new($bytes)) -Algorithm SHA256).Hash.ToLower().Substring(0, 16)
    Write-Output ("listing {0}: {1} paths in the profile, {2} keys in HKCU; the paths that are not Windows' own have the digest {3}" -f $name, $files.Count, $keys.Count, $digest)
  }
  'compare' {
    $before, $after = $Rest[0], $Rest[1]
    $failed = 0
    $failedNamed = 0
    $named = 0
    $others = @{}
    $full = New-Object System.Collections.Generic.List[string]
    foreach ($kind in 'files', 'keys') {
      $diff = Compare-Object @(Get-Content "$Results\$before.$kind") @(Get-Content "$Results\$after.$kind")
      foreach ($d in $diff) {
        $path = $d.InputObject
        $side = if ($d.SideIndicator -eq '=>') { 'left after uninstall' } else { 'gone after uninstall' }
        if ($kind -eq 'files' -and ($path -eq 'AppData\Local\mcbench-lite' -or $path.StartsWith('AppData\Local\mcbench-lite\'))) { $class = 'the data folder' }
        elseif (Is-WindowsOwn $path) { $class = "Windows' own" }
        elseif ($TestOwnStart.IsMatch($path)) { $class = "the test's Edge" }
        elseif ($kind -eq 'files' -and $path -match $NsisCopy) { $class = "the uninstaller's copy" }
        elseif ($d.SideIndicator -eq '<=') { $class = 'removed by Windows or the test' }
        else { $class = 'NOT ALLOWED'; $failed++ }
        $line = "{0,-30} {1,-22} {2}: {3}" -f $class, $side, $kind, $path
        $full.Add($line)
        # -match ignores case.
        if ($path -match 'mcbench') {
          Write-Output $line
          $named++
          if ($class -eq 'NOT ALLOWED') { $failedNamed++ }
        } else {
          $others[$class] = 1 + [int]$others[$class]
        }
      }
    }
    $full | Set-Content -Encoding UTF8 "$Results\compare-full.txt"
    $counts = if ($others.Count) { ($others.GetEnumerator() | Sort-Object Name | ForEach-Object { '{0} {1}' -f $_.Value, $_.Name }) -join ', ' } else { 'none' }
    Write-Output ("compare: {0} differing paths hold mcbench (above); the {1} others, by class: {2}; every line is in compare-full.txt" -f $named, ($full.Count - $named), $counts)
    if (-not (Test-Path -LiteralPath $DataDir)) { Fail "the data folder $DataDir is gone; the uninstaller keeps it." }
    if ($failed) { Fail "$failed paths are left after uninstall that are neither the data folder, Windows' own, the test's Edge nor the uninstaller's copy, $failedNamed of them holding mcbench; compare-full.txt lists them all." }
    Write-Output 'compare: only the data folder, Windows'' own paths and the uninstaller''s copy differ'
  }
  'install' {
    $installer, $expect = $Rest[0], $Rest[1]
    $r = Run-Timed $installer @('/S') "$Results\install.err"
    No-Admin-Prompt
    if ($r.Code -ne 0) { Fail "the installer exited with $($r.Code): $($r.Err)" }
    $c = Check-Install $expect
    "install-ms $($r.Ms)" | Set-Content "$Results\install.time"
    Write-Output ("install: exit 0 in {0} ms; {1}" -f $r.Ms, $c)
  }
  'check-install' {
    No-Admin-Prompt
    Write-Output "check-install: $(Check-Install $Rest[0])"
  }
  'refused' {
    $installer = $Rest[0]
    if (-not (App-Processes).Count) { Fail 'the app is not running, so the refusal cannot be tested.' }
    $was = @(Folder-Hashes $InstDir)
    $r = Run-Timed $installer @('/S') "$Results\refused-install.err"
    if ($r.Code -ne 2) { Fail "the installer over a running app exited with $($r.Code), not 2." }
    Write-Output "installer over the running app: exit 2: $($r.Err.Trim())"
    # _?= runs the uninstaller in place, so its own exit code is the one seen.
    $r = Run-Timed "$InstDir\uninstall.exe" @('/S', "_?=$InstDir") "$Results\refused-uninstall.err"
    if ($r.Code -ne 2) { Fail "the uninstaller of a running app exited with $($r.Code), not 2." }
    Write-Output "uninstaller of the running app: exit 2: $($r.Err.Trim())"
    $now = @(Folder-Hashes $InstDir)
    if (Compare-Object $was $now) { Fail 'the install folder changed although both refused.' }
    if (-not (Test-Path -LiteralPath $UninstallKey) -or -not (Test-Path -LiteralPath $Shortcut)) { Fail 'the uninstall entry or the shortcut is gone although the uninstaller refused.' }
    Write-Output 'refused: the install folder, shortcut and entry are unchanged'
  }
  'copy-direct' {
    New-Item -ItemType Directory -Force $CopyDir | Out-Null
    Get-ChildItem -LiteralPath $InstDir -Force -File | Where-Object { $_.Name -notin 'install-source', 'uninstall.exe' } |
      ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $CopyDir }
    [IO.File]::WriteAllBytes("$CopyDir\install-source", [Text.Encoding]::ASCII.GetBytes("direct`n"))
    Write-Output "copy-direct: $(@(Get-ChildItem -LiteralPath $CopyDir).Count) files in $CopyDir"
  }
  'as-is' {
    $want = @(Read-Expect $Rest[0])
    $dir = Payload-Dir $Rest[1]
    $version = $Rest[2]
    $have = @(Folder-Hashes $dir | Where-Object { $_ -notmatch '  (install-source|uninstall\.exe)$' })
    if (Compare-Object $want $have) { Fail "the payload in $dir is not the one it was: $($have -join '; ')" }
    $fileVersion = [IO.File]::ReadAllText("$dir\version")
    if ($fileVersion -ne "$version`n") { Fail "the version file in $dir holds '$fileVersion', not $version and a newline." }
    $parent = Split-Path -Parent $dir
    $waiting = @(Get-ChildItem -LiteralPath $parent -Force | Where-Object { $_.Name -in '.mcbench-lite.update', '.mcbench-lite.update.json' })
    if ($waiting.Count) { Fail "a download waits beside $dir ($(($waiting | ForEach-Object { $_.Name }) -join ', '))." }
    $beside = @(Get-ChildItem -LiteralPath $parent -Force -Filter '.mcbench-lite*' | ForEach-Object { $_.Name })
    Write-Output ("as-is: the payload in {0} is unchanged, version {1}, and no download waits; beside it: {2}" -f $dir, $version, ($beside -join ', '))
  }
  'wait-download' {
    $parent = Split-Path -Parent (Payload-Dir $Rest[0])
    $t = Get-Date
    do {
      Start-Sleep -Milliseconds 500
      $ready = (Test-Path -LiteralPath "$parent\.mcbench-lite.update") -and (Test-Path -LiteralPath "$parent\.mcbench-lite.update.json")
    } while (-not $ready -and ((Get-Date) - $t).TotalSeconds -lt 120)
    if (-not $ready) { Fail "no waiting download (.mcbench-lite.update and .mcbench-lite.update.json) appeared in $parent within 120 s." }
    Write-Output ("wait-download: the waiting download is in {0} after {1:N1} s" -f $parent, ((Get-Date) - $t).TotalSeconds)
  }
  'updated' {
    # The swap is made before the core and the window of the start that
    # takes it, so the payload, its version and the program are checked at
    # once; the .old entries are removed once the new program's core runs.
    $want = @(Read-Expect $Rest[0])
    $dir = Payload-Dir $Rest[1]
    $version = $Rest[2]
    $have = @(Folder-Hashes $dir | Where-Object { $_ -notmatch '  (install-source|uninstall\.exe)$' -and $_ -notmatch '\.old$' })
    if (Compare-Object $want $have) { Fail "the payload in $dir is not the newer release's: $($have -join '; ')" }
    $fileVersion = [IO.File]::ReadAllText("$dir\version")
    if ($fileVersion -ne "$version`n") { Fail "the version file in $dir holds '$fileVersion', not $version and a newline." }
    $exe = "$dir\mcbench-lite.exe"
    $running = @(App-Processes | Where-Object { $_.Path -eq $exe })
    if (-not $running.Count) { Fail "no running mcbench-lite.exe is $exe (running: $((App-Processes | ForEach-Object { '{0} {1}' -f $_.Id, $_.Path }) -join '; '))." }
    $wantExe = (@($want | Where-Object { $_.EndsWith('  mcbench-lite.exe') })[0] -split '\s+', 2)[0]
    $runExe = (Get-FileHash -LiteralPath $running[0].Path -Algorithm SHA256).Hash.ToLower()
    if ($runExe -ne $wantExe) { Fail "the running $exe (pid $($running[0].Id)) has sha256 $runExe, not the newer release's $wantExe." }
    $t = Get-Date
    do {
      $old = @(Get-ChildItem -LiteralPath $dir -Force -Filter '*.old')
      if ($old.Count) { Start-Sleep -Milliseconds 500 }
    } while ($old.Count -and ((Get-Date) - $t).TotalSeconds -lt 60)
    if ($old.Count) { Fail "60 s after the check began, $(($old | ForEach-Object { $_.Name }) -join ', ') is still in $dir." }
    $oldGone = ((Get-Date) - $t).TotalSeconds
    $parent = Split-Path -Parent $dir
    $left = @(Get-ChildItem -LiteralPath $parent -Force -Filter '.mcbench-lite.update*')
    if ($left.Count) { Fail "$(($left | ForEach-Object { $_.Name }) -join ', ') is still beside $dir." }
    Write-Output ("updated: the payload in {0} is the newer release's and its version file reads {1}; the running mcbench-lite.exe (pid {2}) is {3} with sha256 {4}; no .old entry was left after {5:N1} s; no .mcbench-lite.update* entry is beside it" -f $dir, $version, $running[0].Id, $exe, $runExe, $oldGone)
  }
  'display-version' {
    # The restarted program writes DisplayVersion once its core runs, a few
    # seconds after its window: a new value is waited for up to 60 s, and an
    # unchanged one is read after 10 s, when a wrong write would have happened.
    $same = $Rest[0] -eq 'same'
    $want = if ($same) { (Get-Content "$Results\installed-version.txt").Trim() } else { $Rest[0] }
    $wait = if ($same) { 10 } else { 60 }
    $t = Get-Date
    do {
      Start-Sleep -Milliseconds 500
      $have = (Get-ItemProperty -LiteralPath $UninstallKey).DisplayVersion
    } while (($same -or $have -ne $want) -and ((Get-Date) - $t).TotalSeconds -lt $wait)
    if ($have -ne $want) { Fail "the uninstall entry's DisplayVersion is '$have', not '$want'." }
    Write-Output ("display-version: {0}, read {1:N1} s after the step began" -f $have, ((Get-Date) - $t).TotalSeconds)
  }
  'log-mark' {
    $n = @(Update-Lines).Count
    "$n" | Set-Content "$Results\update-$($Rest[0]).mark"
    Write-Output "log-mark $($Rest[0]): update.log has $n lines"
  }
  'log-new' {
    $mark = Read-Mark $Rest[0]
    $sentence = $Refusals[$Rest[1]]
    if (-not $sentence) { Fail "$($Rest[1]) is neither value nor byte." }
    $t = Get-Date
    do {
      $new = @(Update-Lines | Select-Object -Skip $mark)
      $hit = @($new | Where-Object { $_.Contains($sentence) })
      if (-not $hit.Count) { Start-Sleep -Milliseconds 500 }
    } while (-not $hit.Count -and ((Get-Date) - $t).TotalSeconds -lt 120)
    $new | ForEach-Object { Write-Output "update.log: $_" }
    if (-not $hit.Count) { Fail "update.log got no line holding '$sentence' within 120 s." }
    Write-Output ("log-new: the refusal is in update.log after {0:N1} s" -f ((Get-Date) - $t).TotalSeconds)
  }
  'log-none' {
    $new = @(Update-Lines | Select-Object -Skip (Read-Mark $Rest[0]))
    if ($new.Count) { Fail "update.log has $($new.Count) new lines: $($new -join ' | ')" }
    Write-Output "log-none: update.log has no line after the mark $($Rest[0])"
  }
  'uninstall' {
    $quiet = (Get-ItemProperty -LiteralPath $UninstallKey).QuietUninstallString
    if ($quiet -notmatch '^"([^"]+)" (.+)$') { Fail "QuietUninstallString '$quiet' is not '`"<program>`" <arguments>'." }
    # The NSIS uninstaller copies itself to %TEMP% and runs from there;
    # Start-Process -Wait waits for that copy too (it waits for descendants).
    $r = Run-Timed $Matches[1] @($Matches[2]) "$Results\uninstall.err"
    if ($r.Code -ne 0) { Fail "the uninstaller exited with $($r.Code): $($r.Err)" }
    foreach ($gone in $InstDir, $Shortcut, $UninstallKey) { if (Test-Path -LiteralPath $gone) { Fail "$gone is still there after uninstall." } }
    if (-not (Test-Path -LiteralPath $DataDir)) { Fail "the data folder $DataDir is gone; the uninstaller keeps it." }
    "uninstall-ms $($r.Ms)" | Set-Content "$Results\uninstall.time"
    Write-Output "uninstall: exit 0 in $($r.Ms) ms; install folder, shortcut and entry gone; the data folder stays"
  }
  'desktop' {
    $inner = $Rest[0]
    $args2 = @($Rest | Select-Object -Skip 1)
    $log = "$Results\desktop-$inner.log"
    Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue
    $cmd = "powershell -NoProfile -ExecutionPolicy Bypass -File $Root\vm-test.ps1 $inner $($args2 -join ' ') > `"$log`" 2>&1"
    $bat = "$Root\desktop-$inner.cmd"
    # The step's exit code is the log's last line (a task's LastTaskResult is conhost's).
    "@echo off`r`n$cmd`r`necho exit=%ERRORLEVEL% >> `"$log`"`r`n" | Set-Content -Encoding ascii $bat
    $name = "mcb-s5-$inner"
    # conhost --headless: the step's console is never shown over the app.
    $action = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument "--headless cmd.exe /c `"$bat`""
    $principal = New-ScheduledTaskPrincipal -UserId 'plain' -LogonType Interactive -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero)
    Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName $name
    # The step has ended when its log's last line is the exit line: a task
    # stays Running while a program it started runs (the app a step leaves
    # open), so the task's state alone would wait for the app.
    $t = Get-Date
    do {
      Start-Sleep -Milliseconds 300
      $state = (Get-ScheduledTask -TaskName $name).State
      $ended = (Test-Path -LiteralPath $log) -and (@(Get-Content -LiteralPath $log)[-1] -match '^exit=')
    } while (-not $ended -and $state -eq 'Running' -and ((Get-Date) - $t).TotalSeconds -lt 600)
    # A task whose app still runs stays registered (unregistering may end
    # it); the next step of that name replaces it.
    if ($state -ne 'Running') { Unregister-ScheduledTask -TaskName $name -Confirm:$false }
    if (-not $ended -and $state -eq 'Running') { Fail "the desktop step $inner ran for more than 600 s." }
    if (-not (Test-Path -LiteralPath $log)) { Fail "the desktop step $inner wrote no log; the task did not start in plain's session." }
    $lines = @(Get-Content -LiteralPath $log)
    $lines | Select-Object -SkipLast 1
    if ($lines[-1].Trim() -ne 'exit=0') { Fail "the desktop step $inner ended with $($lines[-1].Trim())." }
  }
  'window' {
    $name = $Rest[0]
    $start = if ($Rest.Count -gt 1) { $Rest[1] } else { $Shortcut }
    if ((App-Processes).Count) { Fail 'the app runs already.' }
    # A start refuses the data folder while server.pid names a live process;
    # the app leaves the file behind when it quits, and Windows gives pids out
    # again, so the state before each start is printed.
    Write-Output "window ${name}: before the start, $(Lock-State)"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    Start-Process -FilePath $start
    do {
      Start-Sleep -Milliseconds 20
      No-Admin-Prompt
      $w = @(App-Processes | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero })
    } while (-not $w.Count -and $watch.ElapsedMilliseconds -lt 120000)
    $watch.Stop()
    if (-not $w.Count) { Fail "no window of mcbench-lite.exe appeared within 120 s of starting $start." }
    "window-$name-ms $($watch.ElapsedMilliseconds)" | Set-Content "$Results\window-$name.time"
    Write-Output ("window {0}: pid {1}, {2}, title '{3}', {4} ms after {5} was started" -f $name, $w[0].Id, $w[0].Path, $w[0].MainWindowTitle, $watch.ElapsedMilliseconds, $start)
    # mcbench_ui::fatal titles its window "mcbench-lite — stopped" (native/crates/ui/src/lib.rs).
    if ($w[0].MainWindowTitle -match 'stopped$') { Fail "the window is the app's fatal screen (the screenshot shows its sentence); $(Lock-State)." }
  }
  'front' {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
    $A = [System.Windows.Automation.AutomationElement]
    $p = @(App-Processes | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero })
    if (-not $p.Count) { Fail 'no window of mcbench-lite.exe is open.' }
    function Focused { (Get-Process -Id ($A::FocusedElement.Current.ProcessId)).ProcessName }
    # Escape closes the Start menu (StartMenuExperienceHost) and the search
    # (SearchHost); it goes to whatever has the keyboard focus, so it is only
    # pressed while that is not the app.
    $closed = @()
    foreach ($try in 1..3) {
      $f = Focused
      if ($f -eq 'mcbench-lite') { break }
      $closed += $f
      [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
      Start-Sleep -Milliseconds 500
    }
    $win = $A::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $p[0].Id)))
    if (-not $win) { Fail "UI Automation finds no window of pid $($p[0].Id)." }
    $win.SetFocus()
    $t = Get-Date
    while ((Focused) -ne 'mcbench-lite' -and ((Get-Date) - $t).TotalSeconds -lt 5) { Start-Sleep -Milliseconds 200 }
    $f = Focused
    if ($f -ne 'mcbench-lite') { Fail "after Escape ($($closed -join ', ') had the focus) and SetFocus, $f has the keyboard focus, not the app." }
    $before = if ($closed.Count) { "Escape was pressed while $($closed -join ', then ') had the focus" } else { 'the app had the focus already' }
    Write-Output "front: the app's window (pid $($p[0].Id)) has the keyboard focus; $before"
  }
  'wsl-screen' {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $A = [System.Windows.Automation.AutomationElement]
    $p = @(App-Processes)[0]
    $win = $A::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition($A::ProcessIdProperty, $p.Id)))
    $t = Get-Date
    do {
      $hit = @($win.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.Name -like "*$WslSentence*" })
      if (-not $hit.Count) { Start-Sleep -Milliseconds 500 }
    } while (-not $hit.Count -and ((Get-Date) - $t).TotalSeconds -lt 60)
    if (-not $hit.Count) { Fail "the app's window shows no element whose name holds '$WslSentence' within 60 s." }
    No-Admin-Prompt
    Write-Output "wsl-screen: the setup screen says '$($hit[0].Current.Name)'"
  }
  'quit' {
    $p = @(App-Processes)
    if (-not $p.Count -and $Rest -contains 'if-running') { Write-Output 'quit: the app is not running'; exit 0 }
    if (-not $p.Count) { Fail 'the app is not running.' }
    foreach ($q in $p) { [void]$q.CloseMainWindow() }
    $t = Get-Date
    while ((App-Processes).Count -and ((Get-Date) - $t).TotalSeconds -lt 30) { Start-Sleep -Milliseconds 200 }
    if ((App-Processes).Count) {
      App-Processes | ForEach-Object { Stop-Process -Id $_.Id -Force }
      Write-Output ("quit: the app still ran 30 s after its window was closed; stopped by pid {0}" -f (($p | ForEach-Object { $_.Id }) -join ', '))
    } else {
      Write-Output ("quit: the app exited {0:N1} s after its window was closed" -f ((Get-Date) - $t).TotalSeconds)
    }
    # Whether the app left its lock behind (it does on Windows: the core's
    # lock is not dropped before the process exits).
    Write-Output "quit: after the quit, $(Lock-State)"
  }
  'stale-lock' {
    # Only after a start showed the fatal screen, whose screen vm-test.sh kept:
    # server.pid is kept as results\stale-server.pid, then deleted so the rest
    # of the test can run.
    $file = "$DataDir\server.pid"
    if (-not (Test-Path -LiteralPath $file)) { Fail 'the data folder has no server.pid to delete.' }
    $state = Lock-State
    Copy-Item -LiteralPath $file -Destination "$Results\stale-server.pid"
    Remove-Item -LiteralPath $file
    Write-Output "stale-lock: $state; it was copied to results\stale-server.pid and DELETED from the data folder by the test"
  }
  'ui-log' {
    if ((App-Processes).Count) { Fail 'the app runs already.' }
    $log = "$env:TEMP\mcbench-ui-log.jsonl"
    Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue
    $env:MCBENCH_UI_LOG = $log
    Start-Process -FilePath "$InstDir\mcbench-lite.exe"
    $t = Get-Date
    do { Start-Sleep -Milliseconds 200; $w = @(App-Processes | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero }) } while (-not $w.Count -and ((Get-Date) - $t).TotalSeconds -lt 120)
    if (-not $w.Count) { Fail 'no window of mcbench-lite.exe appeared within 120 s with MCBENCH_UI_LOG set.' }
    Start-Sleep -Seconds 10
    foreach ($q in App-Processes) { [void]$q.CloseMainWindow() }
    $t = Get-Date
    while ((App-Processes).Count -and ((Get-Date) - $t).TotalSeconds -lt 30) { Start-Sleep -Milliseconds 200 }
    if ((App-Processes).Count) { App-Processes | ForEach-Object { Stop-Process -Id $_.Id -Force }; Write-Output 'ui-log: the app was stopped by pid after 30 s' }
    Write-Output "ui-log: after the quit, $(Lock-State)"
    if (-not (Test-Path -LiteralPath $log)) { Fail "MCBENCH_UI_LOG named $log and the app wrote no file there." }
    $lines = @(Get-Content -LiteralPath $log)
    $bad = 0
    foreach ($line in $lines) { if ($line -eq '') { continue }; try { $null = $line | ConvertFrom-Json } catch { $bad++; Write-Output "ui-log: not JSON: $line" } }
    Write-Output ("ui-log: {0} lines, {1} not JSON" -f $lines.Count, $bad)
    $lines | Where-Object { $_ -match '"renderer"|"fonts"' } | ForEach-Object { Write-Output "ui-log: $_" }
    $firstFrame = @($lines | Where-Object { $_ -match 'first_frame' })
    if ($firstFrame.Count) { $firstFrame | ForEach-Object { Write-Output "ui-log: $_" } } else { Write-Output 'ui-log: no first_frame line, as expected: these builds were made before the app wrote one' }
    Copy-Item -LiteralPath $log -Destination "$Results\ui-log.jsonl"
    Remove-Item -LiteralPath $log
  }
  'first-start' {
    # A true first start: no app process, no data folder; the program the
    # Start menu shortcut names, with MCBENCH_UI_LOG for this process only and
    # its stderr in a file; timed to the app's first_frame line.
    $n = $Rest[0]
    $keep = $Rest[1] -eq 'keep'
    if ((App-Processes).Count) { Fail 'the app runs already.' }
    if (Test-Path -LiteralPath $DataDir) { Remove-Item -LiteralPath $DataDir -Recurse -Force; Write-Output "first-start ${n}: the data folder was removed" }
    $target = (New-Object -ComObject WScript.Shell).CreateShortcut($Shortcut).TargetPath
    $log = "$env:TEMP\mcbench-first-$n.jsonl"
    Remove-Item -LiteralPath $log -ErrorAction SilentlyContinue
    $env:MCBENCH_UI_LOG = $log
    $before = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    # No stderr is redirected: a redirected program's children inherit the
    # desktop step's own log handle and hold it after the app quits, and the
    # app is a window program.
    $p = Start-Process -FilePath $target -PassThru
    Remove-Item Env:\MCBENCH_UI_LOG
    $ff = $null
    $t = Get-Date
    do {
      Start-Sleep -Milliseconds 20
      No-Admin-Prompt
      if (Test-Path -LiteralPath $log) { $ff = @(Get-Content -LiteralPath $log | Where-Object { $_ -match '"action":"first_frame"' })[0] }
    } while (-not $ff -and ((Get-Date) - $t).TotalSeconds -lt 120)
    if (-not $ff) { Fail "no first_frame line in $log within 120 s of starting $target (pid $($p.Id))." }
    $f = $ff | ConvertFrom-Json
    $wall = [int64]$f.epoch_ms - $before
    '{0} {1:F1} {2}' -f $n, [double]$f.since_start_ms, $wall | Set-Content "$Results\first-$n.txt"
    Write-Output ("first-start {0}: pid {1}, {2}; since_start_ms {3:F1}; epoch_ms minus the instant before the start {4} ms" -f $n, $p.Id, $target, [double]$f.since_start_ms, $wall)
    @(Get-Content -LiteralPath $log) | Where-Object { $_ -match '"action":"(renderer|fonts)"' } | ForEach-Object { Write-Output "first-start ${n}: $_" }
    Copy-Item -LiteralPath $log -Destination "$Results\ui-first-$n.jsonl"
    if ($keep) { Write-Output "first-start ${n}: the app is left running for the screenshot"; exit 0 }
    foreach ($q in App-Processes) { [void]$q.CloseMainWindow() }
    $t = Get-Date
    while ((App-Processes).Count -and ((Get-Date) - $t).TotalSeconds -lt 30) { Start-Sleep -Milliseconds 200 }
    if ((App-Processes).Count) { App-Processes | ForEach-Object { Stop-Process -Id $_.Id -Force }; Write-Output "first-start ${n}: the app still ran 30 s after its window was closed; stopped by pid" }
    Write-Output "first-start ${n}: after the quit, $(Lock-State)"
  }
  'first-summary' {
    # Every first-start's stderr [renderer] line, then the median, lowest and
    # highest of since_start_ms and of epoch_ms minus the start instant.
    foreach ($e in Get-ChildItem -LiteralPath $Results -Filter 'first-*.err' | Sort-Object Name) {
      $r = @(Get-Content -LiteralPath $e.FullName | Where-Object { $_ -match '\[renderer\]' })
      if ($r.Count) { $r | ForEach-Object { Write-Output "first-summary: $($e.Name): $_" } } else { Write-Output ("first-summary: {0}: no [renderer] line in its stderr ({1} bytes)" -f $e.Name, $e.Length) }
    }
    $rows = @(Get-ChildItem -LiteralPath $Results -Filter 'first-*.txt' | ForEach-Object { , ((Get-Content -LiteralPath $_.FullName) -split ' ') })
    foreach ($col in @(@(1, 'since_start_ms'), @(2, 'epoch_ms minus the start instant'))) {
      $v = @($rows | ForEach-Object { [double]$_[$col[0]] } | Sort-Object)
      Write-Output ("first-summary: {0} over {1} runs: median {2:F1} ms, lowest {3:F1}, highest {4:F1}; all: {5}" -f $col[1], $v.Count, $v[[int][Math]::Floor($v.Count / 2)], $v[0], $v[-1], ($v -join ', '))
    }
    # WMI refuses plain over ssh (Access denied); the host prints its own load.
    Write-Output "first-summary: Windows sees $([Environment]::ProcessorCount) logical processors"
  }
  'edge' {
    # Microsoft Edge downloads URL as a person would; the file must arrive in
    # Downloads within 90 s. Prints the top-level windows either way and
    # leaves Edge open for the screenshot.
    $url = $Rest[0]
    $file = "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Start-Process -FilePath 'msedge.exe' -ArgumentList $url
    $t = Get-Date
    do { Start-Sleep -Milliseconds 500 } while (-not (Test-Path -LiteralPath $file) -and ((Get-Date) - $t).TotalSeconds -lt 90)
    Top-Windows | ForEach-Object { Write-Output "edge: window: $_" }
    if (-not (Test-Path -LiteralPath $file)) {
      # What Edge shows instead: the names of its windows' texts, buttons and
      # links, each once.
      Load-Automation
      $A = [System.Windows.Automation.AutomationElement]
      $ids = @(Get-Process msedge -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
      $seen = @{}
      foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
        if ($w.Current.ProcessId -notin $ids) { continue }
        foreach ($e in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
          $c = $e.Current
          if ($c.Name -and -not $seen.ContainsKey($c.Name) -and $c.ControlType.ProgrammaticName -match 'Text|Button|Hyperlink|MenuItem') { $seen[$c.Name] = 1; Write-Output "edge: shows: $($c.ControlType.ProgrammaticName -replace 'ControlType.', ''): $($c.Name)" }
        }
      }
      Get-ChildItem -LiteralPath "$env:USERPROFILE\Downloads" -Force | ForEach-Object { Write-Output "edge: in Downloads: $($_.Name) ($($_.Length) bytes)" }
      Fail "Edge left no $file within 90 s of opening $url."
    }
    Start-Sleep -Seconds 3
    Write-Output ("edge: {0} arrived, {1} bytes; its mark of the web: {2}" -f $file, (Get-Item -LiteralPath $file).Length, ((Get-Content -LiteralPath $file -Stream Zone.Identifier -ErrorAction SilentlyContinue) -join ' | '))
  }
  'edge-close' {
    foreach ($q in @(Get-Process msedge -ErrorAction SilentlyContinue)) { [void]$q.CloseMainWindow() }
    Start-Sleep -Seconds 5
    @(Get-Process msedge -ErrorAction SilentlyContinue) | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue }
    Write-Output 'edge-close: Edge is closed'
  }
  'motw' {
    # SIMULATED: the installer copied into Downloads with the mark of the web
    # Edge gives a download from the internet (ZoneId 3).
    $file = "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Copy-Item -LiteralPath $Rest[0] -Destination $file -Force
    Set-Content -LiteralPath $file -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3`r`nHostUrl=https://github.com/adonis-singh/mcbench-lite/releases/latest/download/mcbench-lite-setup-x64.exe"
    Write-Output "motw: SIMULATED: $file with Zone.Identifier $((Get-Content -LiteralPath $file -Stream Zone.Identifier) -join ' | ')"
  }
  'open-download' {
    # As a double-click in Explorer: ShellExecute of the downloaded file, so
    # Windows checks its mark of the web.
    $file = "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Start-Process -FilePath $file
    Start-Sleep -Seconds 8
    No-Admin-Prompt
    Top-Windows | ForEach-Object { Write-Output "open-download: window: $_" }
  }
  'find' {
    # Waits up to SECONDS for an element whose name holds the text of KEY in
    # any top-level window.
    $text = $UiText[$Rest[0]]
    $t = Get-Date
    do { $hit = Find-Element $text $false; if (-not $hit) { Start-Sleep -Milliseconds 500 } } while (-not $hit -and ((Get-Date) - $t).TotalSeconds -lt [int]$Rest[1])
    No-Admin-Prompt
    if (-not $hit) { Fail "no element's name is like '$($text -join "' or '")' within $($Rest[1]) s." }
    Write-Output "find: '$($hit.Current.Name)' in pid $($hit.Current.ProcessId)"
  }
  'click' {
    # Waits up to SECONDS for an enabled button or link named the text of KEY
    # (not a title bar's button) and invokes it, as a click would.
    $text = $UiText[$Rest[0]]
    $t = Get-Date
    do { $hit = Find-Element $text $true; if (-not $hit) { Start-Sleep -Milliseconds 500 } } while (-not $hit -and ((Get-Date) - $t).TotalSeconds -lt [int]$Rest[1])
    if (-not $hit) { Top-Windows | ForEach-Object { Write-Output "click: window: $_" }; Control-Names | ForEach-Object { Write-Output "click: there is $_" }; Fail "no enabled control named like '$($text -join "' or '")' within $($Rest[1]) s." }
    $name = (Get-Process -Id $hit.Current.ProcessId).ProcessName
    $pattern = $null
    if ($hit.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke() }
    else { $hit.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand() }
    Start-Sleep -Seconds 2
    No-Admin-Prompt
    Write-Output "click: '$($hit.Current.Name)' ($($hit.Current.ControlType.ProgrammaticName)) of $name was invoked"
  }
  'raise' {
    # Gives the focus to the top-level window whose name holds the text of
    # KEY (a dialog under the Start menu), for the screenshot, and prints the
    # names of its texts and buttons.
    Load-Automation
    $A = [System.Windows.Automation.AutomationElement]
    $text = $UiText[$Rest[0]]
    $w = @($A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object { $n = $_.Current.Name; @($text | Where-Object { $n -like $_ }).Count })[0]
    if (-not $w) { Fail "no top-level window's name is like '$($text -join "' or '")'." }
    $w.SetFocus()
    foreach ($e in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) { if ($e.Current.Name) { Write-Output "raise: $($e.Current.ControlType.ProgrammaticName -replace 'ControlType.', ''): $($e.Current.Name)" } }
    Write-Output "raise: '$($w.Current.Name)' has the focus"
  }
  'forget-download' {
    Remove-Item -LiteralPath "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Write-Output 'forget-download: the downloaded installer was deleted'
  }
  'escape' {
    # Before a screenshot: Escape while the Start menu or the search has the
    # keyboard focus, then the desktop (Program Manager) is given the focus,
    # which closes the Start menu whatever had the focus. Windows above the
    # desktop stay as they are.
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
    $A = [System.Windows.Automation.AutomationElement]
    $f = (Get-Process -Id ($A::FocusedElement.Current.ProcessId)).ProcessName
    if ($f -in 'StartMenuExperienceHost', 'SearchHost', 'ShellExperienceHost') { [System.Windows.Forms.SendKeys]::SendWait('{ESC}'); Start-Sleep -Milliseconds 500; Write-Output "escape: Escape was pressed while $f had the focus" }
    $desk = $A::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition($A::ClassNameProperty, 'Progman')))
    if (-not $desk) { Fail 'UI Automation finds no Program Manager window.' }
    # The desktop sometimes takes no focus (InvalidOperationException); the
    # screenshot is then taken as it is, and this says so.
    try { $desk.SetFocus(); Start-Sleep -Milliseconds 500; Write-Output "escape: the desktop has the focus; before, $f had it" }
    catch { Write-Output "escape: the desktop took no focus ($($_.Exception.Message)); before, $f had it" }
  }
  'downloaded' {
    # Whether Edge's download is in Downloads under its own name, and its mark
    # of the web.
    $file = "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Get-ChildItem -LiteralPath "$env:USERPROFILE\Downloads" -Force | ForEach-Object { Write-Output "downloaded: in Downloads: $($_.Name) ($($_.Length) bytes)" }
    if (-not (Test-Path -LiteralPath $file)) { Fail "$file is not there." }
    Write-Output ("downloaded: {0}, its mark of the web: {1}" -f $file, ((Get-Content -LiteralPath $file -Stream Zone.Identifier -ErrorAction SilentlyContinue) -join ' | '))
  }
  'open-explorer' {
    # As a double-click: Explorer opens the downloaded file, so the prompt
    # that follows is Explorer's (SmartScreen's, for a file with the mark of
    # the web) and shows on the desktop.
    $file = "$env:USERPROFILE\Downloads\mcbench-lite-setup-x64.exe"
    Start-Process -FilePath 'explorer.exe' -ArgumentList "`"$file`""
    Start-Sleep -Seconds 8
    No-Admin-Prompt
    Top-Windows | ForEach-Object { Write-Output "open-explorer: window: $_" }
  }
  'ps-install' {
    # `irm <base>/install.ps1 | iex` in a PowerShell window of its own on
    # plain's desktop, with MCBENCH_LITE_RELEASES set to BASE in that window,
    # as a person would paste it; the window's transcript goes to
    # results\ps-NAME.txt and the window stays open for the screenshot (its
    # pid in results\ps-NAME.pid). Waits up to 300 s for the command to end
    # and prints the transcript.
    $name, $base, $want = $Rest[0], $Rest[1], $Rest[2]
    $out = "$Results\ps-$name.txt"
    Remove-Item -LiteralPath $out -ErrorAction SilentlyContinue
    $cmd = "`$env:MCBENCH_LITE_RELEASES = '$base'; Start-Transcript -LiteralPath '$out' | Out-Null; `$ProgressPreference = 'SilentlyContinue'; `$d = Get-Date; Invoke-WebRequest -UseBasicParsing -Uri $base/mcbench-lite-setup-x64.exe -OutFile `$env:TEMP\ps-install-probe.bin; Write-Host ('ps-install: the download alone {0:N1} s' -f ((Get-Date) - `$d).TotalSeconds); Remove-Item `$env:TEMP\ps-install-probe.bin; `$t = Get-Date; irm $base/install.ps1 | iex; Write-Host ('ps-install: exit {0} after {1:N1} s' -f `$LASTEXITCODE, ((Get-Date) - `$t).TotalSeconds); Stop-Transcript | Out-Null; Start-Sleep -Seconds 3600"
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoLogo', '-NoExit', '-Command', $cmd) -PassThru
    "$($p.Id)" | Set-Content "$Results\ps-$name.pid"
    $t = Get-Date
    do { Start-Sleep -Milliseconds 500; $done = (Test-Path -LiteralPath $out) -and (Select-String -LiteralPath $out -Pattern '^ps-install: exit' -Quiet) } while (-not $done -and ((Get-Date) - $t).TotalSeconds -lt 300)
    No-Admin-Prompt
    if (Test-Path -LiteralPath $out) { Get-Content -LiteralPath $out | Where-Object { $_ -and $_ -notmatch '^\*{10}|^(Start time|End time|Username|RunAs User|Configuration Name|Machine|Host Application|Process ID|PSVersion|PSEdition|PSCompatibleVersions|BuildVersion|CLRVersion|WSManStackVersion|PSRemotingProtocolVersion|SerializationVersion|Windows PowerShell transcript|Transcript started)' } | ForEach-Object { Write-Output "ps-install ${name}: $_" } }
    if (-not $done) { Fail "the command in the PowerShell window (pid $($p.Id)) did not end within 300 s." }
    # The line the window wrote, not the command's own text, which the
    # transcript's header also holds (with {0} where the code goes).
    $code = [regex]::Match((Get-Content -Raw -LiteralPath $out), '(?m)^ps-install: exit (-?\d+) after').Groups[1].Value
    if ($code -ne $want) { Fail "install.ps1 ended with exit code '$code', not $want." }
  }
  'ps-close' {
    # Stops the PowerShell window ps-install NAME opened, by its recorded pid.
    $id = [int](Get-Content -LiteralPath "$Results\ps-$($Rest[0]).pid")
    Stop-Process -Id $id
    Write-Output "ps-close: the PowerShell window (pid $id) is closed"
  }
  'not-installed' {
    foreach ($gone in $InstDir, $Shortcut, $UninstallKey) { if (Test-Path -LiteralPath $gone) { Fail "$gone exists, although the install was refused." } }
    $left = @(Get-ChildItem -LiteralPath $env:TEMP -Force -Filter 'mcbench-lite-install-*')
    if ($left.Count) { Fail "the install's temporary folder is left: $(($left | ForEach-Object { $_.Name }) -join ', ')." }
    Write-Output 'not-installed: no install folder, shortcut or entry, and no temporary folder of the install'
  }
  'consoles' {
    # Every console window on the desktop (conhost's and Windows Terminal's
    # classes) with its title and process: the app, a window program, must
    # open none of its own.
    Load-Automation
    $A = [System.Windows.Automation.AutomationElement]
    $n = 0
    foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
      if ($w.Current.ClassName -in 'ConsoleWindowClass', 'CASCADIA_HOSTING_WINDOW_CLASS', 'PseudoConsoleWindow') {
        $n++
        Write-Output ("consoles: '{0}' ({1}, {2})" -f $w.Current.Name, (Get-Process -Id $w.Current.ProcessId -ErrorAction SilentlyContinue).ProcessName, $w.Current.ClassName)
      }
    }
    Write-Output "consoles: $n console windows; every top-level window:"
    Top-Windows | ForEach-Object { Write-Output "consoles: window: $_" }
  }
  'wsl-turn' {
    # Through the installed program's headless turn as plain, on the app's
    # data folder: headless setup for MCBENCH_TABLES and MCBENCH_TEXTURES, a
    # chat, one build call and one bash call (timed: the first fetches the
    # sandbox assets and imports the WSL distribution); each answer printed
    # as it came, then the files of the data folder's sandbox-assets.
    if ((App-Processes).Count) { Fail 'the app runs; headless setup takes the data folder.' }
    # A native program's stderr through 2>&1 is an error record; with Stop it
    # would end the step at its first line.
    $ErrorActionPreference = 'Continue'
    $exe = "$InstDir\mcbench-lite.exe"
    $set = @(& $exe headless setup $DataDir 2>&1 | ForEach-Object { "$_" })
    $set | ForEach-Object { Write-Output "wsl-turn: setup: $_" }
    if ($LASTEXITCODE -ne 0) { Fail "headless setup exited with $LASTEXITCODE." }
    $folders = @($set | Where-Object { $_ -match '^\{"tables"' })[-1] | ConvertFrom-Json
    $env:MCBENCH_TABLES = $folders.tables
    $env:MCBENCH_TEXTURES = $folders.textures
    # Nothing is copied into the data folder: since native-app 31ecac42 the
    # build reads the block registry itself, and a copy would hide a
    # regression. Whether the folder holds one is printed.
    $reg = "$DataDir\assets\1.21.10\blocks.json"
    Write-Output "wsl-turn: nothing was copied; $reg exists: $(Test-Path -LiteralPath $reg)"
    $script:answers = [System.Collections.Generic.List[string]]::new()
    function Ask([hashtable]$Request) {
      $line = $Request | ConvertTo-Json -Compress -Depth 5
      $out = @($line | & $exe headless turn 2>&1 | ForEach-Object { "$_" })
      # Kept for the step's end: a caller that discards this function's
      # output would discard lines written here.
      $out | ForEach-Object { $script:answers.Add("wsl-turn: $($Request.op) $($Request.name) answer: $_") }
      if ($LASTEXITCODE -ne 0) { Fail "headless turn ($($Request.op)) exited with $LASTEXITCODE." }
      $out
    }
    $created = Ask @{ id = 1; op = 'create'; data = $DataDir; provider = 'claude-code'; model = 'claude-haiku-5-5' }
    $chat = (@($created | Where-Object { $_ -match '"chat"' })[-1] | ConvertFrom-Json).chat
    $null = Ask @{ id = 2; op = 'call'; data = $DataDir; chat = $chat; name = 'build'; imageInput = $false; turnChat = $false; arguments = @{ summary = 'A stone floor'; program = "module.exports = mc => { mc.fill([0, 0, 0], [2, 0, 2], 'stone'); mc.set([1, 1, 1], 'glass'); }" } }
    $t = Get-Date
    $null = Ask @{ id = 3; op = 'call'; data = $DataDir; chat = $chat; name = 'bash'; imageInput = $false; turnChat = $false; arguments = @{ command = 'uname -a; python3 -c "print(6*7)"' } }
    Write-Output ("wsl-turn: the first bash call took {0:N1} s" -f ((Get-Date) - $t).TotalSeconds)
    $t = Get-Date
    $null = Ask @{ id = 4; op = 'call'; data = $DataDir; chat = $chat; name = 'bash'; imageInput = $false; turnChat = $false; arguments = @{ command = 'uname -a; python3 -c "print(6*7)"' } }
    Write-Output ("wsl-turn: the second bash call took {0:N1} s" -f ((Get-Date) - $t).TotalSeconds)
    $assets = "$DataDir\sandbox-assets"
    if (Test-Path -LiteralPath $assets) { Get-ChildItem -LiteralPath $assets -Recurse -Force -File | ForEach-Object { Write-Output ("wsl-turn: {0} ({1} bytes)" -f $_.FullName, $_.Length) } } else { Write-Output "wsl-turn: there is no $assets" }
    & wsl.exe --list --verbose 2>&1 | ForEach-Object { Write-Output "wsl-turn: wsl --list: $(("$_") -replace "`0", '')" }
    $script:answers | ForEach-Object { Write-Output $_ }
  }
  'finish-setup' {
    # The installer's last page (Installation Complete, Close the default
    # button): its controls are not seen through UI Automation, so Enter is
    # pressed while the installer has the keyboard focus, as a person's Enter
    # or click on Close; then the installer must exit.
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
    $A = [System.Windows.Automation.AutomationElement]
    $t = Get-Date
    do { Start-Sleep -Milliseconds 500; No-Admin-Prompt } while (-not (Test-Path -LiteralPath $UninstallKey) -and ((Get-Date) - $t).TotalSeconds -lt [int]$Rest[0])
    if (-not (Test-Path -LiteralPath $UninstallKey)) { Fail "the install made no uninstall entry within $($Rest[0]) s." }
    Start-Sleep -Seconds 3
    $setup = @(Get-Process -Name 'mcbench-lite-setup-x64' -ErrorAction SilentlyContinue)
    if (-not $setup.Count) { Write-Output 'finish-setup: the installer has already exited'; exit 0 }
    $f = (Get-Process -Id ($A::FocusedElement.Current.ProcessId)).ProcessName
    if ($f -ne 'mcbench-lite-setup-x64') { Fail "$f has the keyboard focus, not the installer." }
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
    $t = Get-Date
    while (@(Get-Process -Name 'mcbench-lite-setup-x64' -ErrorAction SilentlyContinue).Count -and ((Get-Date) - $t).TotalSeconds -lt 15) { Start-Sleep -Milliseconds 200 }
    if (@(Get-Process -Name 'mcbench-lite-setup-x64' -ErrorAction SilentlyContinue).Count) { Fail 'the installer still runs 15 s after Enter.' }
    No-Admin-Prompt
    Write-Output 'finish-setup: Enter on the installer''s last page (Close) ended it'
  }
  'console-text' {
    # The text of every Windows Terminal window (its terminal control's
    # TextPattern): the lines holding [renderer] are printed, else that there
    # are none.
    Load-Automation
    $A = [System.Windows.Automation.AutomationElement]
    $TP = [System.Windows.Automation.TextPattern]
    $n = 0
    foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
      if ($w.Current.ClassName -notin 'CASCADIA_HOSTING_WINDOW_CLASS', 'ConsoleWindowClass') { continue }
      foreach ($e in $w.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition($A::IsTextPatternAvailableProperty, $true)))) {
        $lines = @($e.GetCurrentPattern($TP::Pattern).DocumentRange.GetText(-1) -split "`r?`n" | Where-Object { $_.Trim() })
        $n += $lines.Count
        $hits = @($lines | Where-Object { $_ -match '\[renderer\]' })
        Write-Output ("console-text: '{0}' shows {1} lines, {2} of them with [renderer]; its lines:" -f $w.Current.Name, $lines.Count, $hits.Count)
        $lines | Select-Object -First 40 | ForEach-Object { Write-Output "console-text: | $_" }
      }
    }
    if (-not $n) { Fail 'no Windows Terminal window with readable text.' }
  }
  'close-console' {
    # Closes every Windows Terminal window as its title bar's close button
    # does (WM_CLOSE to the window), and waits up to 10 s for them to go.
    Load-Automation
    $A = [System.Windows.Automation.AutomationElement]
    $wins = @($A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.ClassName -in 'CASCADIA_HOSTING_WINDOW_CLASS', 'ConsoleWindowClass' })
    if (-not $wins.Count) { Fail 'there is no console window (Windows Terminal or conhost).' }
    if (-not ('McbClose' -as [type])) { Add-Type -Namespace '' -Name McbClose -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessage(System.IntPtr hWnd, uint msg, System.IntPtr w, System.IntPtr l);' }
    foreach ($w in $wins) { [void][McbClose]::PostMessage([IntPtr]$w.Current.NativeWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
    $t = Get-Date
    do { Start-Sleep -Milliseconds 300; $left = @($A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.Condition]::TrueCondition) | Where-Object { $_.Current.ClassName -in 'CASCADIA_HOSTING_WINDOW_CLASS', 'ConsoleWindowClass' }) } while ($left.Count -and ((Get-Date) - $t).TotalSeconds -lt 10)
    if ($left.Count) { Fail "$($left.Count) console windows are still open 10 s after Close." }
    Write-Output "close-console: $($wins.Count) console windows closed ($(($wins | ForEach-Object { $_.Current.ClassName }) -join ', '))"
  }
  'app-alive' {
    $w = @(App-Processes | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero })
    if (-not $w.Count) { Fail "no window of mcbench-lite.exe is shown (processes: $(@(App-Processes).Count))." }
    Write-Output ("app-alive: pid {0}, title '{1}', started {2:HH:mm:ss}" -f $w[0].Id, $w[0].MainWindowTitle, $w[0].StartTime)
  }
  'ps-direct' {
    # A PowerShell window of its own on plain's desktop runs the installed
    # program directly, as a person typing its path would, then prints a line
    # of its own: that line shows the prompt came back. Waits up to 60 s for
    # the app's window and that line; the window stays open.
    $exe = "$InstDir\mcbench-lite.exe"
    $out = "$Results\ps-direct.txt"
    Remove-Item -LiteralPath $out -ErrorAction SilentlyContinue
    $cmd = "Start-Transcript -LiteralPath '$out' | Out-Null; & '$exe'; Write-Host 'ps-direct: the prompt is back'; Stop-Transcript | Out-Null"
    $p = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoLogo', '-NoExit', '-Command', $cmd) -PassThru
    $t = Get-Date
    do {
      Start-Sleep -Milliseconds 300
      $w = @(App-Processes | Where-Object { $_.MainWindowHandle -ne [IntPtr]::Zero })
      $back = (Test-Path -LiteralPath $out) -and (Select-String -LiteralPath $out -Pattern 'the prompt is back' -Quiet)
    } while (-not ($w.Count -and $back) -and ((Get-Date) - $t).TotalSeconds -lt 60)
    if (-not $w.Count) { Fail "no window of mcbench-lite.exe within 60 s of the PowerShell window (pid $($p.Id)) running it." }
    if (-not $back) { Fail 'the PowerShell window did not give its prompt back within 60 s: it waits for the app.' }
    Write-Output ("ps-direct: the app's window (pid {0}, '{1}') is shown and the prompt came back {2:N1} s after the PowerShell window started" -f $w[0].Id, $w[0].MainWindowTitle, ((Get-Date) - $t).TotalSeconds)
  }
  'app-logs' {
    $logs = "$DataDir\logs"
    if (-not (Test-Path -LiteralPath $logs)) { Fail "the data folder has no logs folder ($logs)." }
    $dest = "$Results\app-logs"
    if (Test-Path -LiteralPath $dest) { Remove-Item -LiteralPath $dest -Recurse -Force }
    Copy-Item -LiteralPath $logs -Destination $dest -Recurse
    Get-ChildItem -LiteralPath $dest -Recurse -File | ForEach-Object { Write-Output ("app-logs: {0} ({1} bytes)" -f $_.FullName.Substring($dest.Length + 1), $_.Length) }
    $update = "$logs\update.log"
    if (Test-Path -LiteralPath $update) { Get-Content -LiteralPath $update | ForEach-Object { Write-Output "update.log: $_" } } else { Write-Output 'app-logs: there is no update.log' }
  }
  default { Fail "unknown step $Step" }
}
exit 0
