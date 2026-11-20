# actions.ps1: the dev binary's action log (one JSON line per user-visible action).
if (Test-Path C:\s4\ui-actions.jsonl) { Get-Content C:\s4\ui-actions.jsonl } else { 'no action log' }
