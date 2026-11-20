# tail.ps1 -Log FILE [-N 40] [-Match REGEX]: the last N lines of FILE that match.
param([Parameter(Mandatory)][string]$Log, [int]$N = 40, [string]$Match = '.')
Get-Content $Log | Where-Object { $_ -match $Match } | Select-Object -Last $N
