# AuraWM - stop script (PowerShell, pure ASCII)
#   powershell -ExecutionPolicy Bypass -File scripts\stop-aurawm.ps1 [-StartGlazeWM]
# Gracefully exits wmd (restoring every hidden window), stops whkd,
# and optionally hands control back to GlazeWM.
param([switch]$StartGlazeWM)
function Say($m) { Write-Host "[aurawm] $m" -ForegroundColor Magenta }

$BinDir = Join-Path $env:USERPROFILE ".local\bin"
$Wmctl  = Join-Path $BinDir "wmctl.exe"

if (Get-Process wmd -ErrorAction SilentlyContinue) {
    Say "Asking wmd to exit gracefully (restores all hidden windows)..."
    if (Test-Path $Wmctl) { & $Wmctl wm-exit 2>$null }
    Start-Sleep -Milliseconds 800
    # Fallback if the graceful path did not finish
    Get-Process wmd -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Say "wmd stopped."
} else {
    Say "wmd was not running."
}

Get-Process whkd -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Say "whkd stopped."

if ($StartGlazeWM) {
    $glaze = Get-Command glazewm -ErrorAction SilentlyContinue
    if ($glaze) {
        Start-Process glazewm -WindowStyle Hidden
        Say "GlazeWM started."
    } else {
        Say "glazewm not found on PATH - start it manually."
    }
} else {
    Say "Run with -StartGlazeWM to hand control back to GlazeWM automatically."
}
