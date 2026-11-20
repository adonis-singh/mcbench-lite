# C:\mcb\build.ps1 STEP: one step of vm-build.sh in the Windows guest, as bench
# (the account the base's MSVC Build Tools and rustup are installed for:
# rustup is in bench's %USERPROFILE%\.cargo, as native/check.sh's guest steps use it).
#
# | Step | Does |
# |---|---|
# | cache | brings stage five's cache disk online and formats it (NTFS, label mcbs5cache) when it is empty |
# | build | unpacks C:\mcb\tree.tar into the cache disk's \tree (fresh), empties \target, and runs native\packaging\build\windows.ps1 into C:\mcb\out (with the profile, kept as C:\mcb\out.profdata and out.pgo.log, or -NoProfile when C:\mcb\in\no-profile exists) with cargo's registry on the cache disk |
#
# The cache disk keeps only cargo's registry (CARGO_HOME), so a release build
# is cold every run, as on a release runner, and the disk stays small.
param([Parameter(Mandatory)][string]$Step)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
function Fail([string]$Sentence) { Write-Output "build.ps1 $Step`: $Sentence"; exit 1 }
trap { Fail $_.Exception.Message }

switch ($Step) {
  'cache' {
    # A fresh Windows may see the disk offline; an empty one is formatted.
    Get-Disk | Where-Object IsOffline | Set-Disk -IsOffline $false
    Get-Disk | Where-Object IsReadOnly | Set-Disk -IsReadOnly $false
    if (-not (Get-Volume -FileSystemLabel mcbs5cache -ErrorAction SilentlyContinue)) {
      $raw = @(Get-Disk | Where-Object PartitionStyle -eq 'RAW')
      if ($raw.Count -ne 1) { Fail "there is no volume mcbs5cache and $($raw.Count) empty disks." }
      $raw[0] | Initialize-Disk -PartitionStyle GPT -PassThru | New-Partition -UseMaximumSize -AssignDriveLetter |
        Format-Volume -FileSystem NTFS -NewFileSystemLabel mcbs5cache -Confirm:$false | Out-Null
      Write-Output 'build.ps1: formatted the cache disk'
    }
  }
  'build' {
    $c = (Get-Volume -FileSystemLabel mcbs5cache).DriveLetter
    if (-not $c) { Fail 'the cache volume mcbs5cache has no drive letter.' }
    foreach ($d in "${c}:\tree", "${c}:\target", 'C:\mcb\out') { if (Test-Path -LiteralPath $d) { Remove-Item -Recurse -Force -LiteralPath $d } }
    New-Item -ItemType Directory "${c}:\tree" | Out-Null
    & tar -xf C:\mcb\tree.tar -C "${c}:\tree"
    if ($LASTEXITCODE -ne 0) { Fail "tar -xf failed (exit code $LASTEXITCODE)." }
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    $env:CARGO_HOME = "${c}:\cargo"
    $env:CARGO_TARGET_DIR = "${c}:\target"
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_BUILD_JOBS = '4'
    $env:MCBENCH_DATA_BUNDLE = (Get-ChildItem C:\mcb\in\bundle -File | Select-Object -First 1).FullName
    $env:MCBENCH_UPDATE_KEY_FILE = 'C:\mcb\in\minisign.pub'
    $env:MCBENCH_UPDATE_FEED = (Get-Content -Raw C:\mcb\in\feed).Trim()
    if (Test-Path -LiteralPath C:\mcb\in\assets-url) { $env:MCBENCH_SANDBOX_ASSETS_URL = (Get-Content -Raw C:\mcb\in\assets-url).Trim() }
    $script = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "${c}:\tree\native\packaging\build\windows.ps1", '-Repository', "${c}:\tree", '-LinuxFiles', 'C:\mcb\in\linux', '-Out', 'C:\mcb\out')
    if (Test-Path -LiteralPath C:\mcb\in\no-profile) { $script += '-NoProfile' }
    foreach ($f in 'C:\mcb\out.profdata', 'C:\mcb\out.pgo.log') { if (Test-Path -LiteralPath $f) { Remove-Item -LiteralPath $f } }
    & powershell @script
    if ($LASTEXITCODE -ne 0) { Fail "windows.ps1 failed (exit code $LASTEXITCODE)." }
    $parts = foreach ($d in 'cargo', 'target') {
      '{0} {1:N2} GiB' -f $d, ((Get-ChildItem -LiteralPath "${c}:\$d" -Recurse -File -Force -ErrorAction SilentlyContinue | Measure-Object -Sum Length).Sum / 1GB)
    }
    Write-Output ("build.ps1: the cache disk holds " + ($parts -join ', '))
  }
  default { Fail "unknown step $Step" }
}
exit 0
