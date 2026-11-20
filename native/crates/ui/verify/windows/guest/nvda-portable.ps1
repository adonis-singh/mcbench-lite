# nvda-portable.ps1: make a portable NVDA in C:\s4\nvda (no install), with a config that skips first-run dialogs.
$t = Get-Date
& C:\winvm\gui.ps1 -Cmd 'C:\s4\dl\nvda_2026.3.1.exe --create-portable-silent --portable-path=C:\s4\nvda' -Wait 300
"portable {0:n0} s" -f ((Get-Date) - $t).TotalSeconds
Get-Process nvda -ErrorAction SilentlyContinue | Select-Object Id, Path
Get-ChildItem C:\s4\nvda | Select-Object -First 12 -ExpandProperty Name
$cfg = 'C:\s4\nvda\userConfig'
New-Item -ItemType Directory -Force $cfg | Out-Null
@"
schemaVersion = 18
[general]
	showWelcomeDialogAtStartup = False
	askToExit = False
	playStartAndExitSounds = False
[update]
	autoCheckForUpdates = False
	allowUsageStats = False
	askedAllowUsageStats = True
[speech]
	synth = silence
[speechViewer]
	showSpeechViewerAtStartup = False
"@ | Set-Content -Encoding utf8 "$cfg\nvda.ini"
Get-Content "$cfg\nvda.ini"
