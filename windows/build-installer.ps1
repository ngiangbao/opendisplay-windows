# Builds opendisplay.exe (receiver + sender, release, static CRT) and the Inno Setup installer.
# Prereqs: Rust + MSVC Build Tools, Inno Setup 6 (winget install JRSoftware.InnoSetup).
param([string]$Version = "0.2.0", [string]$TargetDir = "C:\odt")
$ErrorActionPreference = "Stop"
$env:Path += ";$env:USERPROFILE\.cargo\bin"
$env:CARGO_TARGET_DIR = $TargetDir          # short path avoids MAX_PATH link errors
$env:RUSTFLAGS = "-C target-feature=+crt-static"   # no VC++ redistributable needed
Push-Location "$PSScriptRoot\..\linux"
$ErrorActionPreference = "Continue"
cargo build --release -p opendisplay
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
$ErrorActionPreference = "Stop"
Pop-Location
$iscc = @("$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe", "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found" }
& $iscc "/DAppVersion=$Version" "/DExePath=$TargetDir\release\opendisplay.exe" "$PSScriptRoot\opendisplay.iss"
Get-ChildItem "$PSScriptRoot\output\*.exe"
