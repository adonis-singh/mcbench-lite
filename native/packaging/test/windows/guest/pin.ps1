# C:\mcb\pin.ps1 STEP: one step of vm-pin.sh in the Windows guest, as bench.
#
# | Step | Does |
# |---|---|
# | pin | unpacks C:\mcb\tree.tar into the cache disk's \tree (fresh), empties \target, and runs windows.ps1 with MCBENCH_PGO_PIN naming the corpus recording in C:\mcbpriv into C:\mcb\pin; C:\mcb\pin.profdata and C:\mcb\pin.record are the pinned profile; the payload's program is kept as C:\mcb\progs\pinned.exe |
# | noprof | windows.ps1 -NoProfile on the same tree into C:\mcb\np; its program is kept as C:\mcb\progs\noprof.exe |
# | prove | the tree again in \\tree2 with C:\\mcb\\pin.profdata and .record as its pinned profile, and an ordinary windows.ps1 build from there into C:\\mcb\\prove: the profile from a second checkout path |
# | times ROUNDS | pgo-replay.js --times of C:\mcbpriv\corpus6.tar.zst on noprof.exe and pinned.exe in turn, ROUNDS rounds, with the pinned Node.js the profile build unpacked; prints each run's times lines |
#
# C:\mcbpriv holds the private benchmark's recordings. It is on the run's
# overlay (C:), which vm-pin.sh deletes, never on the cache disk, and nothing
# of it is copied back.
param([Parameter(Mandatory, Position = 0)][string]$Step, [Parameter(ValueFromRemainingArguments)][string[]]$Rest = @())
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
function Fail([string]$Sentence) { Write-Output "pin.ps1 $Step`: $Sentence"; exit 1 }
trap { Fail $_.Exception.Message }
$c = (Get-Volume -FileSystemLabel mcbs5cache).DriveLetter
if (-not $c) { Fail 'the cache volume mcbs5cache has no drive letter.' }
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_HOME = "${c}:\cargo"
$env:CARGO_TARGET_DIR = "${c}:\target"
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_JOBS = '4'
$env:MCBENCH_DATA_BUNDLE = (Get-ChildItem C:\mcb\in\bundle -File | Select-Object -First 1).FullName
$env:MCBENCH_UPDATE_KEY_FILE = 'C:\mcb\in\minisign.pub'
$env:MCBENCH_UPDATE_FEED = (Get-Content -Raw C:\mcb\in\feed).Trim()
$progs = 'C:\mcb\progs'
New-Item -ItemType Directory -Force $progs | Out-Null

function Build([string]$Out, [string[]]$Extra, [string]$Tree = "${c}:\tree") {
  $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "$Tree\native\packaging\build\windows.ps1", '-Repository', $Tree, '-LinuxFiles', 'C:\mcb\in\linux', '-Out', $Out) + $Extra
  $t = Get-Date
  & powershell @arguments
  if ($LASTEXITCODE -ne 0) { Fail "windows.ps1 failed (exit code $LASTEXITCODE)." }
  Write-Output ("pin.ps1: windows.ps1 {0} took {1:N0} s" -f ($Extra -join ' '), ((Get-Date) - $t).TotalSeconds)
}

switch ($Step) {
  'pin' {
    foreach ($d in "${c}:\tree", "${c}:\target", 'C:\mcb\pin', 'C:\mcb\pin.profdata', 'C:\mcb\pin.record', 'C:\mcb\pin.pgo.log') { if (Test-Path -LiteralPath $d) { Remove-Item -Recurse -Force -LiteralPath $d } }
    New-Item -ItemType Directory "${c}:\tree" | Out-Null
    & tar -xf C:\mcb\tree.tar -C "${c}:\tree"
    if ($LASTEXITCODE -ne 0) { Fail "tar -xf failed (exit code $LASTEXITCODE)." }
    $env:MCBENCH_PGO_PIN = 'C:\mcbpriv\corpus-recording.tar.zst'
    Build 'C:\mcb\pin' @()
    Copy-Item -LiteralPath C:\mcb\pin\mcbench-lite.exe -Destination "$progs\pinned.exe"
  }
  'noprof' {
    if (Test-Path -LiteralPath C:\mcb\np) { Remove-Item -Recurse -Force -LiteralPath C:\mcb\np }
    Build 'C:\mcb\np' @('-NoProfile')
    Copy-Item -LiteralPath C:\mcb\np\mcbench-lite.exe -Destination "$progs\noprof.exe"
  }
  'times' {
    $rounds = [int]$Rest[0]
    $node = (Get-ChildItem -LiteralPath "${c}:\target-pgo\node" -Recurse -Filter node.exe | Select-Object -First 1).FullName
    if (-not $node) { Fail "there is no node.exe under ${c}:\target-pgo\node; the profile build unpacks it." }
    $replay = "${c}:\tree\native\packaging\build\pgo-replay.js"
    $ErrorActionPreference = 'Continue'
    foreach ($round in 1..$rounds) {
      foreach ($p in 'noprof', 'pinned') {
        $work = "C:\mcbpriv\tw-$p-$round"
        Write-Output "pin.ps1: round $round, $p"
        # Only sums are printed: the recording's folder names are the private
        # benchmark's.
        $lines = @(& $node $replay C:\mcbpriv\corpus6.tar.zst $env:MCBENCH_DATA_BUNDLE $work "$progs\$p.exe" --times 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { Fail "pgo-replay.js on $p failed (exit code $LASTEXITCODE)." }
        $sum = @{ PLACE = 0.0; TICK = 0.0; VERIFY = 0.0; INPUT = 0.0; other = 0.0 }
        $folders = 0
        foreach ($line in $lines | Where-Object { $_ -match ' times: ' }) {
          $folders++
          foreach ($m in [regex]::Matches($line, '(PLACE|TICK|VERIFY|INPUT|other) ([0-9.]+) s')) { $sum[$m.Groups[1].Value] += [double]$m.Groups[2].Value }
        }
        $refused = @($lines | Where-Object { $_ -match 'refused' }).Count
        Write-Output ("pin.ps1: round {0}, {1}: {2} folders, PLACE {3:N3} s, TICK {4:N3} s, VERIFY {5:N3} s, INPUT {6:N3} s, other {7:N3} s" -f $round, $p, $folders, $sum.PLACE, $sum.TICK, $sum.VERIFY, $sum.INPUT, $sum.other)
        Remove-Item -Recurse -Force -LiteralPath $work -ErrorAction SilentlyContinue
      }
    }
  }
  'prove' {
    # The recorded profile from a second checkout path: the tree unpacked
    # again into \tree2 with C:\mcb\pin.profdata and .record as its pinned
    # profile, and an ordinary build from there (windows.ps1 copies it to
    # its fixed path), which must apply the profile to every engine function.
    $tree2 = "${c}:\tree2"
    foreach ($d in $tree2, 'C:\mcb\prove', 'C:\mcb\prove.profdata', 'C:\mcb\prove.pgo.log') { if (Test-Path -LiteralPath $d) { Remove-Item -Recurse -Force -LiteralPath $d } }
    New-Item -ItemType Directory $tree2 | Out-Null
    & tar -xf C:\mcb\tree.tar -C $tree2
    if ($LASTEXITCODE -ne 0) { Fail "tar -xf failed (exit code $LASTEXITCODE)." }
    $pgo = "$tree2\native\packaging\build\pgo"
    Copy-Item -LiteralPath C:\mcb\pin.profdata -Destination "$pgo\x86_64-pc-windows-msvc.profdata" -Force
    Copy-Item -LiteralPath C:\mcb\pin.record -Destination "$pgo\x86_64-pc-windows-msvc.record" -Force
    Remove-Item Env:\MCBENCH_PGO_PIN -ErrorAction SilentlyContinue
    Build 'C:\mcb\prove' @() $tree2
  }
  default { Fail "unknown step $Step" }
}
exit 0
