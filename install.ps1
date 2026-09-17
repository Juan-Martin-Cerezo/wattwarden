# WattWarden Windows Installer
$ErrorActionPreference = "Stop"
$Repo = "Juan-Martin-Cerezo/wattwarden"
$InstallDir = "$env:LOCALAPPDATA\Programs\wattwarden"
$BinaryName = "wattwarden.exe"

Write-Host "⚡ Installing WattWarden for Windows..." -ForegroundColor Cyan

if (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
}

$DownloadUrl = "https://github.com/$Repo/releases/latest/download/wattwarden-windows-x86_64.exe"
$TargetPath = "$InstallDir\$BinaryName"

try {
    Write-Host "📥 Downloading $BinaryName from GitHub Releases..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $TargetPath
} catch {
    Write-Host "⚠️  Precompiled binary not found. Attempting build via Cargo..." -ForegroundColor Yellow
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        cargo install --git "https://github.com/$Repo.git" wattwarden-cli --root "$InstallDir"
    } else {
        Write-Error "Could not download binary or compile via Cargo."
    }
}

# Add to User PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to user PATH." -ForegroundColor Green
}

Write-Host "✅ WattWarden successfully installed to $TargetPath" -ForegroundColor Green
Write-Host "👉 Run from PowerShell: wattwarden" -ForegroundColor Cyan
