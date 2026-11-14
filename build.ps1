# Builds GameOverlay.exe (release) and packages it into an MSI installer.
# Usage: powershell -ExecutionPolicy Bypass -File build.ps1

$ErrorActionPreference = "Stop"
$root = "D:\Projects\GameOverlay"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location $root
Write-Host "==> cargo build --release"
cargo build --release
if ($LASTEXITCODE -ne 0) { Pop-Location; exit 1 }

$exe = "$root\target\release\GameOverlay.exe"
if (-not (Test-Path $exe)) { Write-Host "exe not found"; Pop-Location; exit 1 }

$stage = "$root\installer\stage"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $exe "$stage\GameOverlay.exe" -Force
Copy-Item "$root\installer\README.txt" "$stage\README.txt" -Force
Copy-Item "$root\installer\app.ico" "$stage\app.ico" -Force

$dist = "$root\dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null

Write-Host "==> wix build"
& "$env:USERPROFILE\.dotnet\tools\wix.exe" build "$root\installer\GameOverlay.wxs" `
    -arch x64 -bindpath "$stage" -o "$dist\GameOverlay-1.0.0-x64.msi"
if ($LASTEXITCODE -ne 0) { Pop-Location; exit 1 }

Copy-Item $exe "$dist\GameOverlay.exe" -Force
Compress-Archive -Path "$dist\GameOverlay.exe","$dist\GameOverlay-1.0.0-x64.msi","$stage\README.txt" `
    -DestinationPath "$dist\GameOverlay-1.0.0-portable.zip" -Force

Pop-Location
Write-Host "==> done"
Get-ChildItem $dist | Select-Object Name, Length
