# Start Deskfolk.
#
# A debug build loads its UI from Vite's dev server rather than from bundled
# assets, so two processes have to be up. This starts whichever is missing and
# leaves the companion in the foreground so Ctrl+C stops everything.

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# Vite binds ::1, the brain binds 127.0.0.1. Checking only one stack made the
# launcher think the port was free, start a second Vite, and fail with
# "port already in use" — so try both.
function Test-Port($port) {
    foreach ($host_ in @("127.0.0.1", "::1")) {
        try {
            $c = New-Object Net.Sockets.TcpClient
            $c.Connect($host_, $port)
            $c.Close()
            return $true
        } catch { }
    }
    return $false
}

Write-Host ""
Write-Host "  Deskfolk" -ForegroundColor Yellow
Write-Host "  --------"

# --- UI dev server --------------------------------------------------------
if (Test-Port 1420) {
    Write-Host "  UI server already running" -ForegroundColor DarkGray
} else {
    if (-not (Test-Path "ui\node_modules")) {
        Write-Host "  installing UI dependencies (one time)..." -ForegroundColor DarkGray
        Push-Location ui; npm install | Out-Null; Pop-Location
    }
    Write-Host "  starting the UI server..." -ForegroundColor DarkGray
    Start-Process -FilePath "cmd.exe" `
        -ArgumentList "/c", "npm run dev" `
        -WorkingDirectory (Join-Path $PSScriptRoot "ui") `
        -WindowStyle Minimized
    $ok = $false
    foreach ($i in 1..40) {
        Start-Sleep -Milliseconds 500
        if (Test-Port 1420) { $ok = $true; break }
    }
    if (-not $ok) {
        Write-Host "  UI server did not come up on :1420." -ForegroundColor Red
        Write-Host "  Try it by hand:  cd ui;  npm run dev" -ForegroundColor Red
        exit 1
    }
    Write-Host "  UI server up" -ForegroundColor DarkGray
}

# --- Local brain (optional) ----------------------------------------------
if (Test-Port 8087) {
    Write-Host "  local brain found on :8087 - he'll use mistral-heretic" -ForegroundColor DarkGray
} else {
    Write-Host "  no local brain on :8087 - he'll fall back to his offline lines" -ForegroundColor DarkGray
    Write-Host "    start it with:  cd ..\brain;  .\run_brain.ps1" -ForegroundColor DarkGray
}

# --- The companion --------------------------------------------------------
if (-not (Test-Path "target\debug\deskfolk-app.exe")) {
    Write-Host "  first build - this takes a few minutes..." -ForegroundColor DarkGray
}
Write-Host "  starting the companion (Ctrl+C to stop)" -ForegroundColor Green
Write-Host ""
$env:DESKFOLK_LOG = if ($env:DESKFOLK_LOG) { $env:DESKFOLK_LOG } else { "info" }
cargo run -q -p deskfolk-app
