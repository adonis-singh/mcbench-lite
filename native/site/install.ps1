# mcbench-lite installer for Windows, run in PowerShell as
#   irm https://adonis-singh.github.io/mcbench-lite/install.ps1 | iex
#
# No administrator. It downloads the release's mcbench-lite-setup-x64.exe and
# SHA256SUMS into a new folder under %TEMP% with Invoke-WebRequest, which
# marks no file as downloaded from the internet, checks the installer's
# sha256 against SHA256SUMS, runs it silently (the per-user install in
# %LOCALAPPDATA%\Programs\mcbench-lite, the same as a double-click of the
# installer gives), removes the folder and starts the app.
#
# MCBENCH_LITE_RELEASES names the folder the release assets are fetched from.
# MCBENCH_LITE_NO_OPEN=1 installs without starting the app (a script piped to
# iex takes no arguments). Windows PowerShell 5.1 as Windows ships it, or
# PowerShell 7. The whole body is one function called on the last line, so a
# download cut off part way runs nothing.

function Install-McbenchLite {
  $releases = if ($env:MCBENCH_LITE_RELEASES) { $env:MCBENCH_LITE_RELEASES } else { 'https://github.com/adonis-singh/mcbench-lite/releases/latest/download' }
  $asset = 'mcbench-lite-setup-x64.exe'
  $instDir = "$env:LOCALAPPDATA\Programs\mcbench-lite"
  $ProgressPreference = 'SilentlyContinue'
  $ErrorActionPreference = 'Stop'
  $stage = $null
  try {
    if ($env:OS -ne 'Windows_NT') { throw 'this script installs on Windows; on macOS and Linux use install.sh' }
    # The machine's own architecture, also when this PowerShell runs as x64
    # code emulated on ARM.
    $arch = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Environment').PROCESSOR_ARCHITECTURE
    if ($arch -ne 'AMD64') { throw "there is no Windows build for $arch; the Windows build is for x64" }
    # Windows PowerShell 5.1 may not offer TLS 1.2 by itself, which GitHub requires.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $stage = Join-Path $env:TEMP ('mcbench-lite-install-' + [IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory $stage | Out-Null
    Write-Output "Downloading $releases/$asset"
    foreach ($name in $asset, 'SHA256SUMS') {
      try { Invoke-WebRequest -UseBasicParsing -Uri "$releases/$name" -OutFile (Join-Path $stage $name) }
      catch { throw "could not download $releases/$name ($($_.Exception.Message))" }
    }
    $want = @(Get-Content (Join-Path $stage 'SHA256SUMS') | ForEach-Object { $f = $_ -split '\s+', 2; if ($f.Count -eq 2 -and $f[1].TrimStart('*') -eq $asset) { $f[0].ToLower() } })
    if ($want.Count -ne 1) { throw "$asset is not listed in the release's SHA256SUMS" }
    $have = (Get-FileHash -Algorithm SHA256 (Join-Path $stage $asset)).Hash.ToLower()
    if ($have -ne $want[0]) { throw "the sha256 of the downloaded $asset does not match the release's SHA256SUMS, so nothing was installed" }
    Write-Output "Checked the sha256 of $asset against SHA256SUMS"

    # The installer refuses with exit code 2 and one sentence on stderr while
    # the app runs; both are passed on. A refusal is written with Write-Host,
    # so it shows in the window that ran iex and in a transcript.
    $err = Join-Path $stage 'setup.err'
    $p = Start-Process -FilePath (Join-Path $stage $asset) -ArgumentList '/S' -Wait -PassThru -RedirectStandardError $err
    $sentence = (Get-Content -Raw $err)
    if ($sentence) { $sentence = $sentence.Trim() }
    if ($p.ExitCode -eq 2) { Write-Host "mcbench-lite install: $sentence"; $global:LASTEXITCODE = 2; return }
    if ($p.ExitCode -ne 0) { throw "the installer exited with $($p.ExitCode): $sentence" }
    Write-Output "Installed $instDir"
    if ($env:MCBENCH_LITE_NO_OPEN -ne '1') {
      # Started by Explorer, as from the Start menu: the app attaches to the
      # console of the process that starts it, so started from here it would
      # write into this window and end when the person closes it.
      Start-Process -FilePath "$env:WINDIR\explorer.exe" -ArgumentList "`"$instDir\mcbench-lite.exe`""
      Write-Output 'Started mcbench-lite'
    }
    Write-Output 'Start mcbench-lite from the Start menu.'
    Write-Output "To uninstall: Settings > Apps > Installed apps > mcbench-lite > Uninstall, or run `"$instDir\uninstall.exe`""
    $global:LASTEXITCODE = 0
  } catch {
    Write-Host "mcbench-lite install: $($_.Exception.Message)"
    $global:LASTEXITCODE = 1
  } finally {
    if ($stage -and (Test-Path -LiteralPath $stage)) { Remove-Item -LiteralPath $stage -Recurse -Force }
  }
}

Install-McbenchLite
