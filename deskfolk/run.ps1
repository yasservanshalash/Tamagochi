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
    [switch]$NoBrain,
    [switch]$Dev
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

# Developer mode: the modular companion + the dev-only Sprite Studio (right-click
# him -> "Sprite Studio (dev)"). Off by default so a normal run ships untouched.
if ($Dev) {
    $env:DESKFOLK_DEV = "1"
    $env:DESKFOLK_MODULAR = "1"
    Write-Host "  dev mode: modular render + Sprite Studio enabled" -ForegroundColor Cyan
}

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

# --- The web UI (wizard + Control Center) ---------------------------------
# A debug build loads its web pages from the Vite dev server on :1420; a release
# build uses the built bundle in ui/dist. The onboarding wizard now opens on
# first run, so a debug session needs that dev server up *before* the app —
# otherwise the wizard window can't load.
$ui = Join-Path $PSScriptRoot "ui"
if ($Release) {
    Write-Host "  building the UI bundle..." -ForegroundColor DarkGray
    Push-Location $ui
    npm run build | Out-Null
    Pop-Location
} elseif (Test-Port 1420) {
    Write-Host "  UI dev server already up on :1420" -ForegroundColor DarkGray
} else {
    Write-Host "  starting the UI dev server on :1420 (its own window)..." -ForegroundColor DarkGray
    # A visible, persistent window (`cmd /k`) so any npm/vite error is on screen
    # rather than swallowed by a hidden process — the silent version could hang
    # here forever with nothing to show for it.
    Start-Process -FilePath "cmd.exe" `
        -ArgumentList "/k", "npm run dev" `
        -WorkingDirectory $ui
    $ok = $false
    foreach ($i in 1..40) {
        Start-Sleep -Milliseconds 500
        if (Test-Port 1420) { $ok = $true; break }
    }
    if ($ok) {
        Write-Host "  UI dev server up" -ForegroundColor DarkGray
    } else {
        Write-Host "  UI dev server did not come up in 20s - check its window for an error." -ForegroundColor DarkYellow
        Write-Host "  continuing anyway; if the wizard is blank, wait for that window then refresh it." -ForegroundColor DarkGray
    }
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
