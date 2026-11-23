# AuraWM - start script (PowerShell, pure ASCII)
#   powershell -ExecutionPolicy Bypass -File scripts\start-aurawm.ps1 [-Rebuild]
# 1. Verifies dependencies (prompts to install anything missing)
# 2. Gracefully stops GlazeWM if it is running
# 3. Builds (if needed) and installs binaries
# 4. Starts whkd + wmd with verbose logs for troubleshooting
param([switch]$Rebuild)
$ErrorActionPreference = "Stop"
function Say($m) { Write-Host "[aurawm] $m" -ForegroundColor Magenta }

$RepoDir = Split-Path -Parent $PSScriptRoot
$BinDir  = Join-Path $env:USERPROFILE ".local\bin"
$LogDir  = Join-Path $env:LOCALAPPDATA "aurawm\logs"
New-Item -ItemType Directory -Force -Path $BinDir, $LogDir | Out-Null

# ---- 1. dependency checks (ask before installing) -----------------------
function Ensure-Dep($cmd, $wingetId, $why) {
    if (Get-Command $cmd -ErrorAction SilentlyContinue) { return }
    Say "Missing dependency: $cmd ($why)"
    $answer = Read-Host "Install $cmd via winget ($wingetId)? [Y/n]"
    if ($answer -eq "" -or $answer -match "^[Yy]") {
        winget install --id $wingetId -e --accept-source-agreements --accept-package-agreements
        # refresh PATH so the new tool is visible in this session
        $env:Path = [Environment]::GetEnvironmentVariable("Path","User") + ";" +
                    [Environment]::GetEnvironmentVariable("Path","Machine")
        if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
            Say "$cmd installed but not on PATH yet - open a new terminal and re-run."
            exit 1
        }
    } else {
        Say "Cannot continue without $cmd. Aborting."
        exit 1
    }
}
Ensure-Dep "cargo" "Rustlang.Rustup" "builds the WM"
Ensure-Dep "whkd"  "LGUG2Z.whkd"     "hotkey daemon that drives wmctl"

# ---- 2. hand off from GlazeWM -------------------------------------------
$glaze = Get-Process glazewm -ErrorAction SilentlyContinue
if ($glaze) {
    Say "GlazeWM is running - stopping it for the AuraWM session..."
    # Try the graceful CLI exit first (restores GlazeWM-hidden windows),
    # fall back to killing the process.
    try { glazewm command wm-exit 2>$null; Start-Sleep -Seconds 2 } catch {}
    Get-Process glazewm -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Get-Process zebar -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Say "GlazeWM stopped."
}

# ---- 3. build + install ---------------------------------------------------
$WmdExe = Join-Path $BinDir "wmd.exe"
if ($Rebuild -or -not (Test-Path $WmdExe)) {
    Say "Building workspace (release)..."
    Set-Location $RepoDir
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) { Say "Build failed - paste the errors back to Claude."; exit 1 }
    Copy-Item "target\release\wmd.exe","target\release\wmctl.exe","target\release\tilekit-cheatsheet.exe" $BinDir -Force
    Say "Binaries installed to $BinDir"
}

# ---- 4. keybindings + daemons ---------------------------------------------
$WhkdRc = Join-Path $env:USERPROFILE ".config\whkdrc"
New-Item -ItemType Directory -Force -Path (Split-Path $WhkdRc) | Out-Null
$existing = if (Test-Path $WhkdRc) { Get-Content $WhkdRc -Raw } else { "" }
if ($existing -notmatch "tilekit|aurawm") {
    Get-Content (Join-Path $RepoDir "configs\whkdrc") -Raw | Add-Content $WhkdRc
    Say "Keybindings appended to $WhkdRc"
}
Get-Process whkd -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Process whkd -WindowStyle Hidden

# wmd with verbose telemetry, stderr captured to a log file
if (Get-Process wmd -ErrorAction SilentlyContinue) {
    Say "wmd already running - restarting it."
    & (Join-Path $BinDir "wmctl.exe") wm-exit 2>$null
    Start-Sleep -Milliseconds 500
    Get-Process wmd -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$LogFile = Join-Path $LogDir "wmd-$stamp.log"
if (-not $env:RUST_LOG) { $env:RUST_LOG = "wmd=debug,wm_core=debug" }
Start-Process $WmdExe -WindowStyle Hidden -RedirectStandardOutput $LogFile -WorkingDirectory $BinDir
Start-Sleep -Milliseconds 800

$status = & (Join-Path $BinDir "wmctl.exe") state
Say "wmd status: $status"
Say "Log (attach this when reporting issues): $LogFile"
Say "Alt+H/J/K/L focus | Alt+Shift+E exit+restore | Alt+Shift+/ cheatsheet"
