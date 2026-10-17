$ErrorActionPreference = 'Stop'
$projectDir = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $projectDir 'PresentMon.exe'
$api = 'https://api.github.com/repos/GameTechDev/PresentMon/releases/latest'

Write-Host 'Finding the latest official PresentMon release...' -ForegroundColor Cyan
$release = Invoke-RestMethod -Uri $api -Headers @{ 'User-Agent' = 'ZoroFPSOverlay-setup' }
$asset = $release.assets | Where-Object { $_.name -match '^PresentMon.*x64\.exe$' } | Select-Object -First 1
if (-not $asset) {
    $asset = $release.assets | Where-Object { $_.name -match 'PresentMon.*\.exe$' -and $_.name -match 'x64|AMD64' } | Select-Object -First 1
}
if (-not $asset) {
    throw "Could not find a Windows x64 PresentMon executable in release $($release.tag_name). Open https://github.com/GameTechDev/PresentMon/releases and download the x64 console executable manually."
}

Write-Host "Downloading $($asset.name)..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $destination
if (-not (Test-Path $destination) -or (Get-Item $destination).Length -lt 100000) {
    Remove-Item $destination -Force -ErrorAction SilentlyContinue
    throw 'Download looks incomplete. Please retry or download the official executable manually.'
}
# Normalize the name expected by the Rust app.
Write-Host "Saved official PresentMon binary to: $destination" -ForegroundColor Green
Write-Host 'Next: build the Rust project with build-release.bat.' -ForegroundColor Green
