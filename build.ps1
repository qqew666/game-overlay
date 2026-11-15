# Builds GameOverlay.exe (release) and packages it into an MSI installer.
# Usage: powershell -ExecutionPolicy Bypass -File build.ps1

$ErrorActionPreference = "Stop"

$root = $PSScriptRoot
if (-not $root) { $root = (Get-Location).Path }
$version = "1.4.0"
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Push-Location $root
try {
    Write-Host "==> cargo build --release"
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $exe = Join-Path $root "target\release\GameOverlay.exe"
    if (-not (Test-Path $exe)) { throw "exe not found at $exe" }

    $stage = Join-Path $root "installer\stage"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Copy-Item $exe (Join-Path $stage "GameOverlay.exe") -Force
    Copy-Item (Join-Path $root "installer\README.txt") (Join-Path $stage "README.txt") -Force
    Copy-Item (Join-Path $root "installer\app.ico") (Join-Path $stage "app.ico") -Force
    Copy-Item (Join-Path $root "installer\License.rtf") (Join-Path $stage "License.rtf") -Force

    $dist = Join-Path $root "dist"
    New-Item -ItemType Directory -Force -Path $dist | Out-Null

    $wix = Join-Path $env:USERPROFILE ".dotnet\tools\wix.exe"
    $msi = Join-Path $dist "GameOverlay-$version-x64.msi"

    Write-Host "==> wix build"
    & $wix build (Join-Path $root "installer\GameOverlay.wxs") `
        -arch x64 -bindpath $stage -ext WixToolset.UI.wixext -o $msi
    if ($LASTEXITCODE -ne 0) { throw "wix build failed" }

    Copy-Item $exe (Join-Path $dist "GameOverlay.exe") -Force
    $zip = Join-Path $dist "GameOverlay-$version-portable.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path (Join-Path $dist "GameOverlay.exe"), $msi, (Join-Path $stage "README.txt") `
        -DestinationPath $zip -Force

    Write-Host "==> done"
    Get-ChildItem $dist | Select-Object Name, Length
}
finally {
    Pop-Location
}
