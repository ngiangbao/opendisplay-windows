# Installs the Virtual Display Driver that OpenDisplay's "extended display" needs.
#   - official release 25.5.2 of github.com/VirtualDrivers/Virtual-Display-Driver (MIT)
#   - pinned SHA-256; the setup exe itself is not code-signed, the driver inside is (SignPath Foundation)
#   - Windows will ask "Would you like to install this device software?": choose Install
# Run elevated (the OpenDisplay installer already is; the GUI starts it with -Verb RunAs).
$ErrorActionPreference = 'Stop'
$url = 'https://github.com/VirtualDrivers/Virtual-Display-Driver/releases/download/25.5.2/Virtual.Display.Driver-v25.05.03-setup-x64.exe'
$sha = 'CA10B85BABECFB636C85B3F04D2306968D4F940DD3DD35767F866207BFBA846E'
$f = Join-Path $env:TEMP 'vdd-setup.exe'
Write-Host 'Downloading the Virtual Display Driver installer...'
Invoke-WebRequest -Uri $url -OutFile $f -UseBasicParsing
if ((Get-FileHash $f -Algorithm SHA256).Hash -ne $sha) {
    Remove-Item $f -Force
    throw 'SHA-256 mismatch: refusing to run the installer'
}
Write-Host 'Running the installer. Click Install when Windows asks about the device software.'
Start-Process -FilePath $f -ArgumentList '/SILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-' -Wait
Remove-Item $f -Force -ErrorAction SilentlyContinue
Write-Host 'Done.'
