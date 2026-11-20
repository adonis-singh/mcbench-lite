# C:\mcb\standin.ps1 STEP [arguments]: the steps of vm-standin.sh that
# vm-test.ps1 does not have. Windows PowerShell 5.1.
#
# | Step | Runs as | Does |
# |---|---|---|
# | inventory | bench | what the base image holds that stage five's holds depend on |
# | parse FILE... | bench | parses each PowerShell file with Windows PowerShell 5.1's parser and prints its errors |
# | refused INSTALLER | plain | starts the stand-in mcbench-lite.exe from the install folder (it sleeps ten minutes), runs vm-test.ps1 refused, and stops it by its pid |
# | write-fails INSTALLER | plain | puts a read-only file named .install.tmp in the install folder, where the installer makes its folder of new files, runs the installer with /S (stopped after 60 s if it hangs) and reports its exit code and stderr; fails only if the installed files changed |
# | data-folder | plain | makes %LOCALAPPDATA%\mcbench-lite with one file, as the app's first start would (the stand-in makes none), so the uninstaller's keeping it is tested |
# | winget INSTALLER | plain | installs with /S /SOURCE=winget and checks install-source; then /S /SOURCE=other stops with exit code 2 and changes nothing |
param([Parameter(Mandatory, Position = 0)][string]$Step, [Parameter(ValueFromRemainingArguments)][string[]]$Rest = @())
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$InstDir = "$env:LOCALAPPDATA\Programs\mcbench-lite"
function Fail([string]$Sentence) { Write-Output "standin.ps1 $Step`: $Sentence"; exit 1 }
trap { Fail $_.Exception.Message }
function Hashes { Get-ChildItem -LiteralPath $InstDir -Force -File | Sort-Object Name | ForEach-Object { '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName).Hash, $_.Name } }

switch ($Step) {
  'inventory' {
    $nt = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
    Write-Output ("windows: {0} {1}, build {2}.{3}" -f $nt.ProductName, $nt.DisplayVersion, $nt.CurrentBuild, $nt.UBR)
    $lic = Get-CimInstance SoftwareLicensingProduct -Filter "PartialProductKey IS NOT NULL AND ApplicationID='55c92734-d682-4d71-983e-d6ec3f16059f'" | Select-Object -First 1
    Write-Output ("licence: {0}, {1:N1} days of the evaluation left" -f $lic.Name, ($lic.GracePeriodRemaining / 1440))
    $nsis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
    if (Test-Path -LiteralPath $nsis) { Write-Output "nsis: $nsis $(& $nsis /VERSION)" } else { Write-Output "nsis: none at $nsis" }
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
      foreach ($i in (& $vswhere -products * -format json | Out-String | ConvertFrom-Json)) { Write-Output "msvc: $($i.displayName) $($i.installationVersion) at $($i.installationPath)" }
    } else { Write-Output 'msvc: no vswhere.exe' }
    foreach ($u in Get-ChildItem C:\Users -Directory) {
      $rustc = "$($u.FullName)\.cargo\bin\rustc.exe"
      if (Test-Path -LiteralPath $rustc) { Write-Output "rust: $($u.Name): $(& $rustc --version)" }
    }
    foreach ($u in Get-LocalUser) { Write-Output ("account: {0}, enabled {1}" -f $u.Name, $u.Enabled) }
    Write-Output ("administrators: " + ((Get-LocalGroupMember Administrators | ForEach-Object { $_.Name }) -join ', '))
    Write-Output ("users: " + ((Get-LocalGroupMember Users | ForEach-Object { $_.Name }) -join ', '))
    foreach ($f in 'Microsoft-Windows-Subsystem-Linux', 'VirtualMachinePlatform') { Write-Output "feature: $f $((Get-WindowsOptionalFeature -Online -FeatureName $f).State)" }
    $wsl = Get-AppxPackage -AllUsers -Name MicrosoftCorporationII.WindowsSubsystemForLinux
    Write-Output "wsl package: $(if ($wsl) { $wsl.Version } else { 'none' })"
    foreach ($p in Get-AppxPackage -AllUsers -Name Microsoft.DesktopAppInstaller) { Write-Output "winget (App Installer): $($p.Version) for $(($p.PackageUserInformation | ForEach-Object { $_.UserSecurityId.Username }) -join ', ')" }
    foreach ($c in 'git', 'bash', 'tar', 'node', 'python') { $g = Get-Command $c -ErrorAction SilentlyContinue; Write-Output "$c`: $(if ($g) { $g.Source } else { 'none' })" }
    $d = Get-PSDrive C; Write-Output ("disk C: {0:N1} GB used, {1:N1} GB free" -f ($d.Used / 1GB), ($d.Free / 1GB))
    Write-Output ("autologon: {0}" -f (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon').DefaultUserName)
    Write-Output ("smartscreen policy: {0}" -f (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer' -ErrorAction SilentlyContinue).SmartScreenEnabled)
  }
  'parse' {
    $bad = 0
    foreach ($f in $Rest) {
      $errors = $null
      [void][System.Management.Automation.Language.Parser]::ParseFile($f, [ref]$null, [ref]$errors)
      if ($errors.Count) { $bad++; foreach ($e in $errors) { Write-Output "parse: $f line $($e.Extent.StartLineNumber): $($e.Message)" } }
      else { Write-Output "parse: $f parses in PowerShell $($PSVersionTable.PSVersion)" }
    }
    if ($bad) { Fail "$bad files do not parse." }
  }
  'refused' {
    $p = Start-Process -FilePath "$InstDir\mcbench-lite.exe" -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 500
    try {
      & powershell -NoProfile -ExecutionPolicy Bypass -File C:\mcb\vm-test.ps1 refused $Rest[0]
      $code = $LASTEXITCODE
    } finally {
      Stop-Process -Id $p.Id
      $p.WaitForExit()
    }
    if ($code -ne 0) { exit $code }
  }
  'write-fails' {
    $installer = $Rest[0]
    $was = @(Hashes)
    $block = "$InstDir\.install.tmp"
    Set-Content -LiteralPath $block 'in the way'
    (Get-Item -LiteralPath $block).IsReadOnly = $true
    $p = Start-Process -FilePath $installer -ArgumentList '/S' -PassThru -RedirectStandardError C:\mcb\results\write-fails.err
    # Reading Handle keeps the process handle open, so ExitCode is known after the exit.
    [void]$p.Handle
    if ($p.WaitForExit(60000)) { $outcome = "exit $($p.ExitCode)" } else { Stop-Process -Id $p.Id; $p.WaitForExit(); $outcome = 'still running after 60 s (stopped)' }
    (Get-Item -LiteralPath $block -Force).IsReadOnly = $false
    $left = if (Test-Path -LiteralPath $block -PathType Container) { 'a folder' } elseif (Test-Path -LiteralPath $block) { 'the blocking file' } else { 'nothing' }
    Remove-Item -LiteralPath $block -Recurse -Force -ErrorAction SilentlyContinue
    $now = @(Hashes | Where-Object { $_ -notmatch '  \.install\.tmp$' })
    $was = @($was | Where-Object { $_ -notmatch '  \.install\.tmp$' })
    Write-Output "write-fails: the installer with .install.tmp blocked: $outcome; stderr: $((Get-Content -Raw C:\mcb\results\write-fails.err)); .install.tmp afterwards was $left"
    if (Compare-Object $was $now) { Fail 'the installed files changed although the new files could not be written.' }
    Write-Output 'write-fails: the installed files are unchanged'
  }
  'data-folder' {
    $data = "$env:LOCALAPPDATA\mcbench-lite"
    New-Item -ItemType Directory -Force $data | Out-Null
    Set-Content -LiteralPath "$data\standin.txt" 'made by vm-standin.sh in place of the app'
    Write-Output "data-folder: made $data"
  }
  'winget' {
    $installer = $Rest[0]
    $p = Start-Process -FilePath $installer -ArgumentList '/S', '/SOURCE=winget' -Wait -PassThru
    if ($p.ExitCode -ne 0) { Fail "the installer with /SOURCE=winget exited with $($p.ExitCode)." }
    $bytes = [IO.File]::ReadAllBytes("$InstDir\install-source")
    if ([Text.Encoding]::ASCII.GetString($bytes) -ne "winget`n") { Fail "install-source holds '$([Text.Encoding]::ASCII.GetString($bytes))', not 'winget' and a newline." }
    Write-Output 'winget: /SOURCE=winget wrote install-source "winget\n"'
    $was = @(Hashes)
    $p = Start-Process -FilePath $installer -ArgumentList '/S', '/SOURCE=other' -Wait -PassThru -RedirectStandardError C:\mcb\results\source-other.err
    if ($p.ExitCode -ne 2) { Fail "the installer with /SOURCE=other exited with $($p.ExitCode), not 2." }
    if (Compare-Object $was @(Hashes)) { Fail 'the installer with /SOURCE=other changed the install folder.' }
    Write-Output "winget: /SOURCE=other stopped with exit 2 and changed nothing: $((Get-Content -Raw C:\mcb\results\source-other.err).Trim())"
  }
  default { Fail "unknown step $Step" }
}
exit 0
