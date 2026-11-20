# start-app.ps1 -Exe PATH -Proc NAME [-Log RUST_LOG] [-Flags F] [-Dir D]: (re)start a GUI program in the
# desktop session; prints the gui.ps1 task line, whose log file holds the program's stderr.
param([Parameter(Mandatory)][string]$Exe, [Parameter(Mandatory)][string]$Proc, [string]$Log = 'info', [string]$Flags = '', [string]$Dir = 'C:\winvm')
Get-Process $Proc -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 500
& C:\winvm\gui.ps1 -Cmd "set RUST_LOG=$Log&& $Exe $Flags" -Wait 0 -Dir $Dir
Start-Sleep 4
