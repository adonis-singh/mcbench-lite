# probe.ps1 -ArgsFile FILE: run C:\t\uia-probe\release\uia-probe.exe with the arguments in FILE (already
# quoted for cmd.exe) in the desktop session through C:\winvm\gui.ps1, and print its output (UTF-8).
param([Parameter(Mandatory)][string]$ArgsFile, [int]$Wait = 120)
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$a = (Get-Content -Raw $ArgsFile).Trim()
$out = & C:\winvm\gui.ps1 -Cmd "C:\t\uia-probe\release\uia-probe.exe $a" -Wait $Wait
$log = ($out[0] -split ' log ')[1]
Get-Content -Encoding UTF8 $log
