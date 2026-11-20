# idle-cpu.ps1 [-Proc mcbench-ui-dev] [-Seconds 20]: the process's CPU time over the interval, as a share of one core.
param([string]$Proc = 'mcbench-ui-dev', [int]$Seconds = 20)
$p = Get-Process $Proc
$c0 = $p.TotalProcessorTime.TotalMilliseconds; $t0 = Get-Date
Start-Sleep $Seconds
$p.Refresh()
$c1 = $p.TotalProcessorTime.TotalMilliseconds; $dt = ((Get-Date) - $t0).TotalMilliseconds
"{0}: {1:n0} ms CPU in {2:n0} ms = {3:n2}% of one core; working set {4:n0} MB" -f $Proc, ($c1 - $c0), $dt, (100 * ($c1 - $c0) / $dt), ($p.WorkingSet64 / 1MB)
