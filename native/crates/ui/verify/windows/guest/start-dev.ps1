# start-dev.ps1 [-Fixture gate] [-Scratch DIR] [-NoOrigin] [-Extra ARGS] [-Exe PATH] [-Env "NAME=VALUE"] [-Dump PATH] [-Fatal TEXT] [-Crash panic|abort|segv -CrashAfterMs N]: (re)start the dev binary in the desktop
# session at origin 40,40 with its action log at C:\s4\ui-actions.jsonl; stderr goes to the gui.ps1 task log (printed).
param([string]$Fixture = 'gate', [string]$Extra = '', [string]$Log = 'warn',
      [string]$Exe = 'C:\t\s4-native\release\mcbench-ui-dev.exe', [string]$Env = '', [string]$Dump = '', [string]$Fatal = '', [string]$Crash = '', [int]$CrashAfterMs = 0, [string]$Scratch = 'C:\s4\scratch', [switch]$NoOrigin)
# -Extra takes '+' for a space, since winvm.sh ps passes its arguments through two shells unquoted.
$Extra = $Extra -replace '\+', ' '
# -NoOrigin leaves the window where its saved preferences put it (the restore check).
$origin = if ($NoOrigin) { '' } else { '--origin 40,40' }
Get-Process mcbench-ui-dev -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
if (Test-Path C:\s4\ui-actions.jsonl) { Remove-Item C:\s4\ui-actions.jsonl }
$dumpArg = if ($Dump) { "--a11y-dump $Dump" } else { '' }
if ($Fatal) { $dumpArg += " --fatal `"$Fatal`"" }
if ($Crash) { $dumpArg += " --crash $Crash --crash-after-ms $CrashAfterMs" }
$set = if ($Env) { "set $Env&& " } else { '' }
& C:\winvm\gui.ps1 -Cmd "${set}set RUST_LOG=$Log&& $Exe --scratch $Scratch --fixture $Fixture $origin --log C:\s4\ui-actions.jsonl $dumpArg $Extra" -Wait 0
Start-Sleep 10
Get-Process mcbench-ui-dev -ErrorAction SilentlyContinue | Select-Object Id, MainWindowTitle
