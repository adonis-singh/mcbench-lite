# scale.ps1 -Percent 150|100: set bench's display scale (LogPixels) and sign the desktop session out; autologon signs it back in.
param([Parameter(Mandatory)][int]$Percent)
$dpi = [int](96 * $Percent / 100)
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name LogPixels -Value $dpi -Type DWord
Set-ItemProperty 'HKCU:\Control Panel\Desktop' -Name Win8DpiScaling -Value 1 -Type DWord
$s = (quser 2>$null | Select-String 'console') -replace '\s+', ' '
$id = ($s -split ' ')[3]
"console session $id; LogPixels $dpi"
logoff $id
