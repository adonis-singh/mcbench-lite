# crash-check.ps1 -Kind panic|abort|segv|fastfail [-Exe PATH]: start the dev binary in the desktop session with --crash KIND
# --crash-after-ms 4000, then report its exit code, the new crash logs, Windows Error Reporting entries and
# Application-log events from the run, and whether a WerFault process or dialog appeared.
param([Parameter(Mandatory)][string]$Kind, [string]$Exe = 'C:\t\s4-native\release\mcbench-ui-dev.exe')
# start-dev.ps1 gives the binary --scratch C:\s4\scratch; its crash logs go to the scratch folder's logs.
$logs = 'C:\s4\scratch\logs'
$t0 = Get-Date
$before = @(Get-ChildItem $logs -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name)
$task = & C:\s4\ps\start-dev.ps1 -Exe $Exe -Crash $Kind -CrashAfterMs 4000
$log = (($task | Select-String 'log (C:\S+)').Matches[0].Groups[1].Value)
$wer = 0
for ($i = 0; $i -lt 40; $i++) {
  if (Get-Process WerFault -ErrorAction SilentlyContinue) { $wer++ }
  if (-not (Get-Process mcbench-ui-dev -ErrorAction SilentlyContinue)) { break }
  Start-Sleep -Milliseconds 250
}
Start-Sleep 3
"kind $Kind; process gone: " + (-not (Get-Process mcbench-ui-dev -ErrorAction SilentlyContinue)) + "; WerFault seen in $wer of the polls; WerFault now: " + @(Get-Process WerFault -ErrorAction SilentlyContinue).Count
"stderr and exit: " + ((Get-Content $log) -join ' | ')
Get-ChildItem $logs | Where-Object { $before -notcontains $_.Name -or $_.Name -eq 'crash-signals.log' } | ForEach-Object { "--- $($_.Name)"; Get-Content $_.FullName | Select-Object -Last 4 }
"WER reports since start: " + @(Get-ChildItem C:\ProgramData\Microsoft\Windows\WER\ReportArchive, C:\ProgramData\Microsoft\Windows\WER\ReportQueue, "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportArchive", "$env:LOCALAPPDATA\Microsoft\Windows\WER\ReportQueue" -ErrorAction SilentlyContinue | Where-Object { $_.LastWriteTime -ge $t0 }).Count
"Application log since start: " + ((Get-WinEvent -FilterHashtable @{LogName='Application'; StartTime=$t0} -ErrorAction SilentlyContinue | ForEach-Object { "$($_.ProviderName) $($_.Id): " + ($_.Message -split "`n")[0] }) -join ' || ')
