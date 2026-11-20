# clip.ps1: the desktop session's clipboard text (Narrator's "copy last phrase" lands there).
$o = & C:\winvm\gui.ps1 -Cmd 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\winvm\clip.ps1 get' -Wait 30
$o | Select-Object -Skip 1 | Where-Object { $_ -match 'clipboard=' } | ForEach-Object { ($_ -split ' units=')[0] }
