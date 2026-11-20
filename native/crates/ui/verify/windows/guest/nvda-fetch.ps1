# nvda-fetch.ps1: download NVDA 2026.3.1 from nvaccess.org, check the SHA-256 published in its release notes.
$ErrorActionPreference = 'Stop'
$want = '6f80a5b0ca720a7d0d314b5be212f37388bbe81a1107ac3298c1821a1cd3f052'
New-Item -ItemType Directory -Force C:\s4\dl | Out-Null
$f = 'C:\s4\dl\nvda_2026.3.1.exe'
$t = Get-Date
Invoke-WebRequest -UseBasicParsing -Uri 'https://download.nvaccess.org/releases/2026.3.1/nvda_2026.3.1.exe' -OutFile $f
"download {0:n1} s, {1} bytes" -f ((Get-Date) - $t).TotalSeconds, (Get-Item $f).Length
$got = (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower()
"sha256 $got"
if ($got -ne $want) { throw "hash mismatch: want $want" }
'hash matches the published value'
(Get-AuthenticodeSignature $f) | Select-Object Status, @{n='Signer';e={$_.SignerCertificate.Subject}} | Format-List
