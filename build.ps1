# Build the standalone portable EXE. No ZIP, installer or WiX required.
# Usage: powershell -ExecutionPolicy Bypass -File .\build.ps1
$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    New-Item -ItemType Directory -Force -Path dist | Out-Null
    Copy-Item target\release\GameOverlay.exe dist\GameOverlay.exe -Force
    Write-Host "Ready: dist\GameOverlay.exe (Mimi portrait is embedded)"
} finally { Pop-Location }
