# windows.ps1 -Repository <path> -LinuxFiles <folder> -Out <folder> [-NoProfile]
#
# The release build of the Windows x64 payload folder, with MSVC, on an x64
# Windows machine (a GitHub windows-2025 runner,
# or the Windows 11 VM on adipc through native/packaging/test/windows/vm-build.sh).
# Windows PowerShell 5.1 or later.
#
# The payload is what native/packaging/windows/installer.sh packs and what
# native/packaging/release.sh puts in mcbench-lite-windows-x86_64.tar.gz for
# the updater. Its files are named in one place, $Payload below:
#
#   mcbench-lite.exe            cargo build --release --locked --features shipped-data
#                               -p mcbench-lite in <Repository>\native, with
#                               MCBENCH_DATA_BUNDLE, MCBENCH_UPDATE_KEY_FILE and
#                               MCBENCH_UPDATE_FEED from the environment (and
#                               MCBENCH_SANDBOX_ASSETS_URL when a staging build
#                               names the sandbox assets' folder)
#   mcbench-build-child         the Linux x86_64 build child, run inside the app's
#                               WSL distro, made by the Linux release build
#                               (native/packaging/build/linux.sh), copied from -LinuxFiles
#   version                     MAJOR.MINOR.PATCH and a newline (LF): mcbench-lite's
#                               version from cargo metadata, the value the build
#                               compiles in; the app checks it at every start
#
# install-source is the installer's and is not in the payload. Build programs
# run in the Linux build child inside the WSL distro, so the workspace's own
# Windows build child is not built or shipped.
#
# Windows on ARM is not built: the first release is x64 only.
#
# mcbench-lite.exe is built with profile-guided optimisation (pgo.md) when
# native\packaging\build\pgo\x86_64-pc-windows-msvc.profdata exists (the
# engine's pinned profile, recorded on the corpus): the build uses it alone,
# with no instrumented build, and fails when an engine function has a record
# for other code or more engine functions than at recording (the .record
# file beside it) find no record: the engine changed and the profile must be
# recorded again. The profile is kept as <Out>.profdata and the build's log
# as <Out>.pgo.log, beside the payload folder and outside it. When there is
# no pinned profile, mcbench-lite.exe is the release build with no profile,
# as with -NoProfile, and neither file is written.
# MCBENCH_PGO_PIN=<recording> records the pinned profile instead (a private
# recording of the corpus, pgo-workload.js, with its .counts beside it): an
# instrumented build in <target folder>-pgo, the replay of <recording> on its
# `headless engine` by pgo-replay.js with the pinned Node.js (node-pins.txt),
# the merge with llvm-profdata of the toolchain's llvm-tools, the profile
# reduced to the engine crate's functions and the build with it;
# <Out>.profdata and <Out>.record are then the files to copy to
# native\packaging\build\pgo\.
#
# -NoProfile builds mcbench-lite.exe with no profile, as a target without a
# pinned profile is built, even when the pinned profile exists: a switch for
# tests and for the with and without timing (pgo.md, Windows builds without
# a profile). A release never passes it.
#
# Every native command's exit code is checked; a failure prints one
# sentence and exits 1.
param(
  [Parameter(Mandatory)][string]$Repository,
  [Parameter(Mandatory)][string]$LinuxFiles,
  [Parameter(Mandatory)][string]$Out,
  [switch]$NoProfile
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Fail([string]$Sentence) {
  [Console]::Error.WriteLine("windows.ps1: $Sentence")
  exit 1
}
trap { Fail $_.Exception.Message }

# The payload's files: name, and where it comes from ('app' is the cargo
# build below; 'linux' is a file of -LinuxFiles; 'version' is written here).
$Payload = @(
  @{ Name = 'mcbench-lite.exe'; From = 'app' },
  @{ Name = 'mcbench-build-child'; From = 'linux' },
  @{ Name = 'version'; From = 'version' }
)
$triple = 'x86_64-pc-windows-msvc'

# Runs a native program, its stdout and stderr shown as text (Windows
# PowerShell 5.1 turns a native program's stderr into error records, which
# 'Stop' would throw on), and fails with $Sentence when it exits non-zero.
function Invoke-Native([string]$Sentence, [string]$File, [string[]]$Arguments) {
  $saved = $ErrorActionPreference
  $ErrorActionPreference = 'Continue'
  & $File @Arguments 2>&1 | ForEach-Object { Write-Host "$_" }
  $code = $LASTEXITCODE
  $ErrorActionPreference = $saved
  if ($code -ne 0) { Fail "$Sentence (exit code $code)" }
}

# The machine's own architecture, also under emulation (PROCESSOR_ARCHITEW6432
# is set for a 32-bit or emulated process).
$machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($machine -ne 'AMD64') { Fail "This machine is $machine, and the x64 payload is built on an AMD64 machine." }
foreach ($name in 'MCBENCH_DATA_BUNDLE', 'MCBENCH_UPDATE_KEY_FILE', 'MCBENCH_UPDATE_FEED') {
  if (-not [Environment]::GetEnvironmentVariable($name)) { Fail "$name is not set; a release build compiles it in." }
}
# The build runs from a copy of the tree at one fixed path. Cargo puts a path
# dependency's absolute path into its crate hash, which is in every mangled
# name, so the engine's pinned profile (pgo.md) matches only a build from the
# path it was recorded at (found with macos.sh, native-app 7645ada8). The
# copy holds what macos.sh copies (native, core, renderer, server, README.md,
# LICENSE, NOTICE.md) and nothing else: robocopy /MIR removes what the
# source no longer has. The folder list is taken as it is, since git is not
# on every machine this runs on (the Windows VM has none); a checkout or the
# tree tar of git ls-files holds only tracked files there. A folder named
# target is neither copied nor removed, so a target folder inside the copy
# survives the next copy.
$src = Join-Path $env:SystemDrive 'mcbench-lite-build\src'
$copied = 'native', 'core', 'renderer', 'server', 'README.md', 'LICENSE', 'NOTICE.md'
New-Item -ItemType Directory -Force $src | Out-Null
foreach ($name in $copied) {
  $from = Join-Path $Repository $name
  if (Test-Path -LiteralPath $from -PathType Container) {
    & robocopy $from (Join-Path $src $name) /MIR /XD target /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { Fail "robocopy of $from into $src failed (exit code $LASTEXITCODE)." }
  } else {
    Copy-Item -LiteralPath $from -Destination (Join-Path $src $name) -Force
  }
}
Get-ChildItem -LiteralPath $src -Force | Where-Object { $_.Name -notin $copied } | Remove-Item -Recurse -Force
Write-Host "windows.ps1: building from the copy in $src"
$Repository = $src
$native = Join-Path $Repository 'native'
if (-not (Test-Path -LiteralPath (Join-Path $native 'Cargo.toml'))) { Fail "$Repository has no native\Cargo.toml." }
foreach ($file in $Payload | Where-Object { $_.From -eq 'linux' }) {
  if (-not (Test-Path -LiteralPath (Join-Path $LinuxFiles $file.Name) -PathType Leaf)) { Fail "$LinuxFiles has no $($file.Name), which the Linux release build makes for the Windows payload." }
}
if ((Test-Path -LiteralPath $Out) -and (Get-ChildItem -LiteralPath $Out -Force)) { Fail "$Out exists and is not empty." }
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path

$profile = "$Out.profdata"
$buildLog = "$Out.pgo.log"
if (-not $NoProfile -and (Test-Path -LiteralPath $profile)) { Fail "$profile exists; the profile is recorded fresh." }

$t0 = Get-Date
Push-Location $native
try {
  # The target folder as cargo resolves it (CARGO_TARGET_DIR or native\target).
  $saved = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  $metadata = & cargo metadata --format-version 1 --no-deps --locked 2>$null
  $code = $LASTEXITCODE; $ErrorActionPreference = $saved
  if ($code -ne 0) { Fail "cargo metadata failed (exit code $code)" }
  $metadata = ($metadata -join "`n") | ConvertFrom-Json
  $target = $metadata.target_directory
  $version = @($metadata.packages | Where-Object { $_.name -eq 'mcbench-lite' })[0].version
  $pinned = Join-Path $Repository "native\packaging\build\pgo\$triple.profdata"
  $pinMode = [bool]$env:MCBENCH_PGO_PIN
  if ($NoProfile -or (-not $pinMode -and -not (Test-Path -LiteralPath $pinned))) {
    # No profile: the release build alone.
    Invoke-Native 'cargo build of mcbench-lite failed' 'cargo' @('build', '--release', '--locked', '--target', $triple, '-p', 'mcbench-lite', '--features', 'shipped-data')
    Write-Host ("build without a profile: {0:N0} s" -f ((Get-Date) - $t0).TotalSeconds)
  } else {
  Invoke-Native 'rustup component add llvm-tools failed' 'rustup' @('component', 'add', 'llvm-tools')

  $saved = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  $sysroot = (& rustc --print sysroot 2>$null | Out-String).Trim()
  $ErrorActionPreference = $saved
  $profdata = Join-Path $sysroot "lib\rustlib\$triple\bin\llvm-profdata.exe"
  $pgo = "$target-pgo"
  $recorded = Join-Path $pgo 'recorded.profdata'
  $step = Get-Date
  if ($pinMode) {
    # The pinned profile recorded here (pgo.md): the instrumented build, the
    # replay of the corpus recording on it, the merge.
    $recording = $env:MCBENCH_PGO_PIN
    if (-not (Test-Path -LiteralPath $recording) -or -not (Test-Path -LiteralPath ($recording -replace '\.tar\.zst$', '.counts'))) { Fail "MCBENCH_PGO_PIN names $recording, which is not a recording with its .counts beside it." }
    $raw = Join-Path $pgo 'raw'
    $node = Join-Path $pgo 'node'
    foreach ($folder in $raw, $node) { if (Test-Path -LiteralPath $folder) { Remove-Item -LiteralPath $folder -Recurse -Force } }
    New-Item -ItemType Directory -Force $raw | Out-Null
    $pin = (Get-Content -LiteralPath (Join-Path $Repository 'native\packaging\build\node-pins.txt') | Where-Object { $_ -match '^windows-x86_64 ' }) -split ' '
    $zip = Join-Path $pgo 'node.zip'
    if (-not (Test-Path -LiteralPath $zip)) { Invoke-WebRequest -UseBasicParsing -Uri $pin[1] -OutFile "$zip.part"; Move-Item -LiteralPath "$zip.part" -Destination $zip }
    if ((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLower() -ne $pin[2]) { Fail "$zip from $($pin[1]) is not the pinned file (sha256 $($pin[2]))." }
    Expand-Archive -LiteralPath $zip -DestinationPath $node
    $nodeExe = (Get-ChildItem -LiteralPath $node -Recurse -Filter 'node.exe' | Select-Object -First 1).FullName
    Invoke-Native 'the instrumented cargo build of mcbench-lite failed' 'cargo' @('build', '--release', '--locked', '--target', $triple, '-p', 'mcbench-lite', '--features', 'shipped-data',
      '--target-dir', $pgo, '--config', "build.rustflags=['-Cprofile-generate=$raw']")
    Write-Host ("pgo: instrumented build {0:N0} s" -f ((Get-Date) - $step).TotalSeconds); $step = Get-Date
    Invoke-Native 'the profile workload failed' $nodeExe @((Join-Path $Repository 'native\packaging\build\pgo-replay.js'), $recording, $env:MCBENCH_DATA_BUNDLE, (Join-Path $pgo 'work'), (Join-Path $pgo "$triple\release\mcbench-lite.exe"))
    Write-Host ("pgo: workload {0:N0} s" -f ((Get-Date) - $step).TotalSeconds); $step = Get-Date
    $runs = @(Get-ChildItem -LiteralPath $raw -Filter '*.profraw' | ForEach-Object { $_.FullName })
    if (-not $runs.Count) { Fail 'The instrumented mcbench-lite.exe wrote no profile.' }
    Invoke-Native 'llvm-profdata merge failed' $profdata (@('merge', '-o', $recorded) + $runs)
    Write-Host ("pgo: merged {0} raw profile files (the engine processes of one program share a file) into {1} bytes, sha256 {2}" -f $runs.Count, (Get-Item -LiteralPath $recorded).Length, (Get-FileHash -LiteralPath $recorded -Algorithm SHA256).Hash.ToLower())
    # The engine crate's records: those of its own code units, and the global
    # functions whose names hold the crate (the engine's generic code
    # instantiated in another crate's units carries that crate's hash, which a
    # version bump changes, so it is left out).
    Invoke-Native 'llvm-profdata merge failed' $profdata @('merge', '--function=^mcbench_core\.', '-o', (Join-Path $pgo 'local.profdata'), $recorded)
    Invoke-Native 'llvm-profdata merge failed' $profdata @('merge', '--function=mcbench_core', '--no-function=-cgu\.[0-9]+;', '-o', (Join-Path $pgo 'global.profdata'), $recorded)
    Invoke-Native 'llvm-profdata merge failed' $profdata @('merge', '-o', $profile, (Join-Path $pgo 'local.profdata'), (Join-Path $pgo 'global.profdata'))
  } else {
    Copy-Item -LiteralPath $pinned -Destination $profile
  }

  $saved = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
  & cargo build --release --locked --target $triple -p mcbench-lite --features shipped-data --config "build.rustflags=['-Cprofile-use=$profile', '-Cllvm-args=-pgo-warn-missing-function']" 2>&1 |
    ForEach-Object { "$_" } | Tee-Object -FilePath $buildLog | ForEach-Object { Write-Host $_ }
  $code = $LASTEXITCODE; $ErrorActionPreference = $saved
  if ($code -ne 0) { Fail "cargo build of mcbench-lite failed (exit code $code)" }
  Write-Host ("pgo: final build {0:N0} s" -f ((Get-Date) - $step).TotalSeconds)
  # The engine's profile: only the engine crate's code units are judged.
  $missing = @(Select-String -LiteralPath $buildLog -Pattern '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: no profile data available for function').Count
  $mismatched = @(Select-String -LiteralPath $buildLog -Pattern '^warning: mcbench_core\.[0-9a-f]+-cgu\.[0-9]+: .*hash mismatch').Count
  if ($pinMode) {
    [IO.File]::WriteAllText("$Out.record", "engine-missing`t$missing`n")
    Write-Host ("pgo: the engine's profile, {0} bytes, sha256 {1}" -f (Get-Item -LiteralPath $profile).Length, (Get-FileHash -LiteralPath $profile -Algorithm SHA256).Hash.ToLower())
  } else {
    $allowed = [int](((Get-Content -LiteralPath ($pinned -replace '\.profdata$', '.record')) | Where-Object { $_ -match '^engine-missing\t' }) -split "`t")[1]
    if ($mismatched -or $missing -gt $allowed) { Fail "The engine's pinned profile does not fit the engine: $mismatched engine functions with a hash mismatch, $missing without a record where it had $allowed ($buildLog); the engine changed, so record the profile again (pgo.md, Recording the engine's profile)." }
  }
  Write-Host "pgo: the engine's profile applied: $mismatched engine functions with a hash mismatch, $missing without a record"
  } # no profile
} finally {
  Pop-Location
}

foreach ($file in $Payload) {
  if ($file.From -eq 'version') { [IO.File]::WriteAllBytes((Join-Path $Out $file.Name), [Text.Encoding]::ASCII.GetBytes("$version`n")); continue }
  $from = if ($file.From -eq 'app') { Join-Path $target "$triple\release\$($file.Name)" } else { Join-Path $LinuxFiles $file.Name }
  if (-not (Test-Path -LiteralPath $from -PathType Leaf)) { Fail "$from was not made." }
  Copy-Item -LiteralPath $from -Destination (Join-Path $Out $file.Name)
}
$names = @($Payload | ForEach-Object { $_.Name })
$extra = @(Get-ChildItem -LiteralPath $Out -Force | Where-Object { $names -notcontains $_.Name })
if ($extra.Count) { Fail "$Out holds $(($extra | ForEach-Object { $_.Name }) -join ', '), which is not in the payload." }

Write-Host ("windows.ps1: the x64 payload in {0}, built in {1:N0} s:" -f $Out, ((Get-Date) - $t0).TotalSeconds)
foreach ($file in $Payload) {
  $path = Join-Path $Out $file.Name
  Write-Host ("{0}  {1,12}  {2}" -f (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLower(), (Get-Item -LiteralPath $path).Length, $file.Name)
}
