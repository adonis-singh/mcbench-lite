# narrator-setup.ps1: stop Narrator Home from opening at Narrator's start (bench's HKCU, in the VM overlay only).
Set-ItemProperty HKCU:\Software\Microsoft\Narrator\NarratorHome AutoStart 0
Get-ItemProperty HKCU:\Software\Microsoft\Narrator\NarratorHome | Select-Object AutoStart
