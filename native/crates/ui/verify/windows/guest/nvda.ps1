# nvda.ps1 start|stop|log: portable NVDA in the desktop session, logging speech (level 12 = input/output) to C:\s4\nvda.log.
param([string]$Op)
switch ($Op) {
  'start' {
    if (Test-Path C:\s4\nvda.log) { Remove-Item C:\s4\nvda.log }
    & C:\winvm\gui.ps1 -Cmd 'C:\s4\nvda\nvda.exe -m --disable-addons --log-level=12 --log-file=C:\s4\nvda.log' -Wait 0 | Out-Null
    Start-Sleep 8
    Get-Process nvda -ErrorAction SilentlyContinue | Select-Object Id
  }
  'stop' {
    & C:\winvm\gui.ps1 -Cmd 'C:\s4\nvda\nvda.exe -q' -Wait 30 | Out-Null
    Start-Sleep 2
    Get-Process nvda -ErrorAction SilentlyContinue | Stop-Process -Force
  }
  'log' { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; Get-Content -Encoding UTF8 C:\s4\nvda.log | Select-String -Pattern 'Speaking|Input:' | ForEach-Object { $_.Line } }
  'raw' { Get-Content C:\s4\nvda.log | Select-Object -Last 60 }
}
