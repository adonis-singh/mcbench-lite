# rate.ps1 -Log FILE [-Seconds 20] [-Proc mcbench-ui-dev]: over the interval, the instrument's frames and presents
# per second (lines "gatewin-rate" in the program's stderr log) and the process's CPU share of one core.
param([Parameter(Mandatory)][string]$Log, [int]$Seconds = 20, [string]$Proc = 'mcbench-ui-dev')
$p = Get-Process $Proc
$n0 = (Get-Content $Log).Count
$c0 = $p.TotalProcessorTime.TotalMilliseconds; $t0 = Get-Date
Start-Sleep $Seconds
$p.Refresh()
$c1 = $p.TotalProcessorTime.TotalMilliseconds; $dt = ((Get-Date) - $t0).TotalMilliseconds
$lines = Get-Content $Log | Select-Object -Skip $n0 | Where-Object { $_ -match 'gatewin-rate' }
$presents = ($lines | ForEach-Object { [int](($_ -split ' ')[-1]) } | Measure-Object -Sum).Sum
$frames = ($lines | ForEach-Object { [int](($_ -split ' ')[2]) } | Measure-Object -Sum).Sum
"{0} s: {1} presents ({2} frame callbacks), CPU {3:n0} ms = {4:n2}% of one core" -f $lines.Count, $presents, $frames, ($c1 - $c0), (100 * ($c1 - $c0) / $dt)
