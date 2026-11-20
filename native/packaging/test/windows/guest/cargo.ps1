# C:\mcb\cargo.ps1 STEP: one step of vm-cargo.sh in the Windows guest, as bench
# (the account the base's MSVC Build Tools and rustup are installed for).
#
# | Step | Does |
# |---|---|
# | cache | brings stage five's cache disk online and formats it (NTFS, label mcbs5cache) when it is empty |
# | unpack | brings the cache disk's \ctree to C:\mcb\tree.tar by content, as native/check.sh's guest does: a file whose bytes differ, or a new one, is written and its time becomes now; an identical file keeps its time; a file not in the tar goes. Cargo's time-based fingerprints then see exactly the changed files as newer than the kept \ctarget's last build |
# | cargo | runs cargo with the arguments in C:\mcb\args.txt (one per line) from \ctree\native, with the toolchain of native\rust-toolchain.toml, cargo's registry in \cargo and the target folder \ctarget on the cache disk, CARGO_INCREMENTAL=0 and no debug information; exits with cargo's exit code |
#
# vm-build.sh's \tree and \target on the same disk are its own; this script
# keeps \ctree and \ctarget, so a second run is incremental.
param([Parameter(Mandatory)][string]$Step)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
function Fail([string]$Sentence) { Write-Output "cargo.ps1 $Step`: $Sentence"; exit 2 }
trap { Fail $_.Exception.Message }

if ($Step -eq 'cache') {
  Get-Disk | Where-Object IsOffline | Set-Disk -IsOffline $false
  Get-Disk | Where-Object IsReadOnly | Set-Disk -IsReadOnly $false
  if (-not (Get-Volume -FileSystemLabel mcbs5cache -ErrorAction SilentlyContinue)) {
    $raw = @(Get-Disk | Where-Object PartitionStyle -eq 'RAW')
    if ($raw.Count -ne 1) { Fail "there is no volume mcbs5cache and $($raw.Count) empty disks." }
    $raw[0] | Initialize-Disk -PartitionStyle GPT -PassThru | New-Partition -UseMaximumSize -AssignDriveLetter |
      Format-Volume -FileSystem NTFS -NewFileSystemLabel mcbs5cache -Confirm:$false | Out-Null
    Write-Output 'cargo.ps1: formatted the cache disk'
  }
  exit 0
}
$c = (Get-Volume -FileSystemLabel mcbs5cache).DriveLetter
if (-not $c) { Fail 'the cache volume mcbs5cache has no drive letter.' }

switch ($Step) {
  'unpack' {
    $t0 = Get-Date
    $stage = "${c}:\cstage"; $tree = "${c}:\ctree"
    if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
    New-Item -ItemType Directory $stage | Out-Null
    New-Item -ItemType Directory -Force $tree | Out-Null
    & tar -xf C:\mcb\tree.tar -C $stage
    if ($LASTEXITCODE -ne 0) { Fail "tar -xf failed ($LASTEXITCODE)." }
    # The run's files are the tar's own list (its directories end in /).
    $files = @(& tar -tf C:\mcb\tree.tar | Where-Object { -not $_.EndsWith('/') } | ForEach-Object { ($_ -replace '^\./', '') -replace '/', '\' })
    if ($LASTEXITCODE -ne 0) { Fail "tar -tf failed ($LASTEXITCODE)." }
    $now = Get-Date
    $keep = @{}
    $written = 0; $same = 0; $removed = 0
    foreach ($rel in $files) {
      $keep[$rel] = $true
      $src = Get-Item -LiteralPath (Join-Path $stage $rel) -Force
      $dst = Join-Path $tree $rel
      $old = Get-Item -LiteralPath $dst -Force -ErrorAction SilentlyContinue
      if ($old -and $old.Length -eq $src.Length -and (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $src.FullName -Algorithm SHA256).Hash) { $same++; continue }
      New-Item -ItemType Directory -Force (Split-Path -Parent $dst) | Out-Null
      Copy-Item -LiteralPath $src.FullName -Destination $dst -Force
      (Get-Item -LiteralPath $dst -Force).LastWriteTime = $now
      $written++
    }
    foreach ($f in Get-ChildItem -LiteralPath $tree -Recurse -File -Force) {
      if (-not $keep.ContainsKey($f.FullName.Substring($tree.Length + 1))) { Remove-Item -LiteralPath $f.FullName -Force; $removed++ }
    }
    Get-ChildItem -LiteralPath $tree -Recurse -Directory -Force | Sort-Object { $_.FullName.Length } -Descending |
      Where-Object { -not (Get-ChildItem -LiteralPath $_.FullName -Force) } | Remove-Item -Force
    Remove-Item -Recurse -Force $stage
    Write-Output ("cargo.ps1: tree synced by content: {0} written, {1} unchanged, {2} removed, in {3:N1} s" -f $written, $same, $removed, ((Get-Date) - $t0).TotalSeconds)
  }
  'cargo' {
    $arguments = @(Get-Content C:\mcb\args.txt | Where-Object { $_ -ne '' })
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    $env:CARGO_HOME = "${c}:\cargo"
    $env:CARGO_TARGET_DIR = "${c}:\ctarget"
    $env:CARGO_BUILD_JOBS = '4'
    $env:RUST_TEST_THREADS = '4'
    $env:CARGO_INCREMENTAL = '0'
    $env:CARGO_PROFILE_DEV_DEBUG = '0'
    $env:CARGO_PROFILE_TEST_DEBUG = '0'
    Set-Location "${c}:\ctree\native"
    # cargo writes its progress on stderr; cmd joins it to stdout so Windows
    # PowerShell 5.1 does not turn it into error records.
    $ErrorActionPreference = 'Continue'
    $line = (@('cargo') + ($arguments | ForEach-Object { if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ } })) -join ' '
    Write-Output "cargo.ps1: $line"
    cmd /c "$line 2>&1"
    exit $LASTEXITCODE
  }
  default { Fail "unknown step $Step" }
}
exit 0
