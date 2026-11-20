# clip-image.ps1 -Path PNG: put the picture PNG on the desktop session's clipboard (the clipboard is per session,
# so the script runs itself again there through C:\winvm\gui.ps1 with -Inner).
param([Parameter(Mandatory)][string]$Path, [switch]$Inner)
if ($Inner) {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  [System.Windows.Forms.Clipboard]::SetImage([System.Drawing.Image]::FromFile($Path))
  "clipboard holds an image: $([System.Windows.Forms.Clipboard]::ContainsImage())"
  exit 0
}
& C:\winvm\gui.ps1 -Cmd "powershell -STA -NoProfile -ExecutionPolicy Bypass -File $PSCommandPath -Path $Path -Inner" -Wait 30
