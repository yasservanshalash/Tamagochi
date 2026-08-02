# Start Deskfolk.
#
# The companion is a native window now, so there is no UI dev server to wait
# for — that requirement died with the webview. What he does still need is the
# brain, for both his mind and his voice, so this starts it if it is not
# already up and leaves the companion in the foreground so Ctrl+C stops it.
#
#   .\run.ps1              debug build
#   .\run.ps1 -Release     optimised build (slower to compile, smaller, faster)
#   .\run.ps1 -NoBrain     do not touch the brain; he falls back to offline lines

param(
    [switch]$Release,
    [switch]$NoBrain
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

function Test-Port($port) {
    foreach ($h in @("127.0.0.1", "::1")) {
        try {
            $c = New-Object Net.Sockets.TcpClient
            $c.Connect($h, $port)
            $c.Close()
            return $true
        } catch { }
    }
    return $false
}

Write-Host ""
Write-Host "  Deskfolk" -ForegroundColor Yellow
Write-Host "  --------"

# --- The brain ------------------------------------------------------------
# He needs it for two separate things: /pet/think for what he says, and
# /pet/speak for saying it out loud. Without it he still runs, on offline lines.
if ($NoBrain) {
    Write-Host "  skipping the brain (-NoBrain)" -ForegroundColor DarkGray
} elseif (Test-Port 8087) {
    Write-Host "  brain already up on :8087" -ForegroundColor DarkGray
} else {
    $brain = Join-Path (Split-Path $PSScriptRoot -Parent) "brain\run_brain.ps1"
    if (Test-Path $brain) {
        Write-Host "  starting the brain on :8087..." -ForegroundColor DarkGray
        Start-Process -FilePath "powershell" `
            -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $brain `
            -WindowStyle Minimized
        $ok = $false
        foreach ($i in 1..60) {
            Start-Sleep -Milliseconds 500
            if (Test-Port 8087) { $ok = $true; break }
        }
        if ($ok) {
            Write-Host "  brain up" -ForegroundColor DarkGray
        } else {
            Write-Host "  brain did not come up - he'll use his offline lines" -ForegroundColor DarkYellow
            Write-Host "    try it by hand:  cd ..\brain;  .\run_brain.ps1" -ForegroundColor DarkGray
        }
    } else {
        Write-Host "  no brain script found - he'll use his offline lines" -ForegroundColor DarkGray
    }
}

# --- The Control Center (optional) ----------------------------------------
# The only thing left that is a web page. It opens on demand from his menu, so
# the dev server is only needed if you actually go looking for it.
if (-not (Test-Port 1420)) {
    Write-Host "  no UI server on :1420 - the Control Center won't open" -ForegroundColor DarkGray
    Write-Host "    if you need it:  cd ui;  npm run dev" -ForegroundColor DarkGray
}

# --- The companion --------------------------------------------------------
$cargoArgs = @("run", "-q", "-p", "deskfolk-app")
$built = "target\debug\deskfolk-app.exe"
if ($Release) {
    $cargoArgs += "--release"
    $built = "target\release\deskfolk-app.exe"
}
if (-not (Test-Path $built)) {
    Write-Host "  first build - this takes a few minutes..." -ForegroundColor DarkGray
}

Write-Host "  starting the companion (Ctrl+C to stop)" -ForegroundColor Green
Write-Host ""
$env:DESKFOLK_LOG = if ($env:DESKFOLK_LOG) { $env:DESKFOLK_LOG } else { "info" }
cargo @cargoArgs
