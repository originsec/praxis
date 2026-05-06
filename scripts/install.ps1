#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Installation Script for Windows.
.DESCRIPTION
    Windows is supported via Docker only. This installer detects Windows,
    verifies Docker / Docker Compose, clones the chosen Praxis release,
    and starts it via `docker compose up --build -d`.
.EXAMPLE
    irm https://praxis.originhq.com/install.ps1 | iex
.EXAMPLE
    .\install.ps1 -Docker
.EXAMPLE
    .\install.ps1 -Remove
#>

[CmdletBinding()]
param(
    [switch]$Docker,
    [switch]$Remove,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

#
# Configuration.
#

$HomeDir       = if ($env:USERPROFILE) { $env:USERPROFILE } elseif ($env:HOME) { $env:HOME } else { "~" }
$PraxisRepo    = "originsec/praxis"
$PraxisVersion = $env:PRAXIS_VERSION
$PraxisDir     = if ($env:PRAXIS_DIR) { $env:PRAXIS_DIR } else { Join-Path $HomeDir ".praxis-docker" }
$ComposeCmd    = $null

#
# Console helpers.
#

function Write-Info    { param($msg) Write-Host "[INFO] "  -ForegroundColor Cyan   -NoNewline; Write-Host $msg }
function Write-Success { param($msg) Write-Host "[OK] "    -ForegroundColor Green  -NoNewline; Write-Host $msg }
function Write-Warn    { param($msg) Write-Host "[WARN] "  -ForegroundColor Yellow -NoNewline; Write-Host $msg }
function Write-Err     { param($msg) Write-Host "[ERROR] " -ForegroundColor Red    -NoNewline; Write-Host $msg; exit 1 }

function Test-Command {
    param($cmd)
    $null = Get-Command $cmd -ErrorAction SilentlyContinue
    return $?
}

function Print-Banner {
    Write-Host ""
    Write-Host "   ██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗" -ForegroundColor Cyan
    Write-Host "   ██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝" -ForegroundColor Cyan
    Write-Host "   ██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗" -ForegroundColor Cyan
    Write-Host "   ██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║" -ForegroundColor Cyan
    Write-Host "   ██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║" -ForegroundColor Cyan
    Write-Host "   ╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝" -ForegroundColor Cyan
    Write-Host "            discover " -ForegroundColor DarkGray -NoNewline
    Write-Host "·" -ForegroundColor Gray -NoNewline
    Write-Host " control " -ForegroundColor DarkGray -NoNewline
    Write-Host "·" -ForegroundColor Gray -NoNewline
    Write-Host " orchestrate" -ForegroundColor DarkGray
    Write-Host "                  by [Ø] Origin" -ForegroundColor Magenta
    Write-Host ""
}

function Print-Usage {
    @"
Usage: install.ps1 [flag]

Flags:
  -Docker     Docker install (default and only supported flow on Windows)
  -Remove     Remove a previous Docker install
  -Help       Show this message

If no flag is given, an interactive menu is shown.
"@ | Write-Host
}

#
# Platform detection. Hard-fail if not on Windows.
#

function Test-Windows {
    if ($IsWindows -or ($env:OS -eq "Windows_NT") -or ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows))) {
        return $true
    }
    return $false
}

function Assert-Windows {
    if (-not (Test-Windows)) {
        Write-Err "install.ps1 only runs on Windows. Use install.sh on Linux/macOS."
    }
    Write-Success "Detected Windows"
    Write-Host ""
}

#
# Arrow-key menu. Returns the selected index.
#

function Select-Menu {
    param(
        [string]$Prompt,
        [string[]]$Options
    )

    $n = $Options.Length
    $sel = 0

    Write-Host $Prompt
    Write-Host "  (Up/Down arrows · Enter to select · Q to quit)" -ForegroundColor DarkGray

    # Reserve lines for each option.
    foreach ($_ in $Options) { Write-Host "" }

    [Console]::CursorVisible = $false
    try {
        while ($true) {
            # Move cursor up to the start of the menu.
            [Console]::SetCursorPosition(0, [Console]::CursorTop - $n)

            for ($i = 0; $i -lt $n; $i++) {
                $line = "".PadRight([Console]::WindowWidth - 1)
                [Console]::Write("`r$line`r")
                if ($i -eq $sel) {
                    Write-Host "  ▶ $($Options[$i])" -ForegroundColor Cyan
                } else {
                    Write-Host "    $($Options[$i])" -ForegroundColor DarkGray
                }
            }

            $key = [Console]::ReadKey($true)
            switch ($key.Key) {
                'UpArrow'   { $sel = ($sel - 1 + $n) % $n }
                'DownArrow' { $sel = ($sel + 1) % $n }
                'K'         { $sel = ($sel - 1 + $n) % $n }
                'J'         { $sel = ($sel + 1) % $n }
                'Enter'     { return $sel }
                'Spacebar'  { return $sel }
                'Q'         { [Console]::CursorVisible = $true; Write-Host ""; exit 130 }
                'Escape'    { [Console]::CursorVisible = $true; Write-Host ""; exit 130 }
                default {
                    if ($key.KeyChar -match '^[1-9]$') {
                        $idx = [int]$key.KeyChar.ToString() - 1
                        if ($idx -lt $n) { return $idx }
                    }
                }
            }
        }
    } finally {
        [Console]::CursorVisible = $true
    }
}

#
# Version resolution.
#

function Get-LatestVersion {
    if ($script:PraxisVersion) {
        Write-Success "Using specified version: $script:PraxisVersion"
        Write-Host ""
        return
    }

    Write-Info "Fetching latest release version..."
    try {
        $response = Invoke-RestMethod -Uri "https://api.github.com/repos/$PraxisRepo/releases/latest" -UseBasicParsing
        $script:PraxisVersion = $response.tag_name
    } catch {
        Write-Err "Could not determine latest version. Check your internet connection."
    }

    if (-not $script:PraxisVersion) {
        Write-Err "Could not determine latest version."
    }

    Write-Success "Latest version: $script:PraxisVersion"
    Write-Host ""
}

#
# === Docker install flow =====================================================
#

function Check-Docker {
    Write-Info "Checking prerequisites..."

    if (-not (Test-Command "docker")) {
        Write-Err "Docker not found. Install Docker Desktop: https://www.docker.com/products/docker-desktop/"
    }
    Write-Success "Found Docker"

    try {
        docker info | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "docker info failed" }
    } catch {
        Write-Err "Docker daemon not running. Start Docker Desktop and try again."
    }
    Write-Success "Docker daemon running"

    docker compose version *> $null
    if ($LASTEXITCODE -eq 0) {
        $script:ComposeCmd = "docker compose"
        Write-Success "Found Docker Compose (plugin)"
    } elseif (Test-Command "docker-compose") {
        $script:ComposeCmd = "docker-compose"
        Write-Success "Found docker-compose (standalone)"
    } else {
        Write-Err "Docker Compose not found. Install Docker Desktop, which includes Compose."
    }

    if (-not (Test-Command "git")) {
        Write-Err "git not found. Install git from https://git-scm.com/download/win"
    }
    Write-Success "Found git"

    Write-Host ""
}

function Clone-Repo-Docker {
    Write-Info "Setting up Praxis $script:PraxisVersion in $PraxisDir..."
    if (Test-Path $PraxisDir) { Remove-Item -Recurse -Force $PraxisDir }
    git clone --depth 1 --branch $script:PraxisVersion "https://github.com/$PraxisRepo.git" $PraxisDir
    if ($LASTEXITCODE -ne 0) { Write-Err "git clone failed." }
    Set-Location $PraxisDir
    Write-Success "Praxis $script:PraxisVersion ready"
    Write-Host ""
}

function Start-Praxis-Docker {
    Write-Info "Building and starting Praxis (this may take a few minutes on first run)..."
    Write-Host ""

    if ($script:ComposeCmd -eq "docker compose") {
        docker compose up --build -d
    } else {
        & $script:ComposeCmd up --build -d
    }
    if ($LASTEXITCODE -ne 0) { Write-Err "Failed to start Praxis containers." }

    Write-Host ""
    Write-Success "Praxis is running!"
    Write-Host ""
}

function Print-Docker-Summary {
    Write-Host ""
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host "  Praxis $script:PraxisVersion is ready!"   -ForegroundColor Green
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Web UI:              http://localhost:8080"
    Write-Host "RabbitMQ Management: http://localhost:15672"
    Write-Host "                     (praxis / praxis)"
    Write-Host ""
    Write-Host "Installation:        $PraxisDir"
    Write-Host ""

    $container = $null
    try {
        if ($script:ComposeCmd -eq "docker compose") {
            $container = (docker compose ps -q praxis 2>$null | Select-Object -First 1)
        } else {
            $container = (& $script:ComposeCmd ps -q praxis 2>$null | Select-Object -First 1)
        }
    } catch { $container = $null }
    $cpSource = if ($container) { "$container`:/app/praxis_cli.exe" } else { "<container_id>:/app/praxis_cli.exe" }

    Write-Host "CLI:" -ForegroundColor Cyan
    Write-Host "  docker cp $cpSource .\praxis_cli.exe"
    Write-Host ""
    Write-Host "Commands:" -ForegroundColor Cyan
    Write-Host "  cd $PraxisDir"
    Write-Host "  $script:ComposeCmd logs -f      # View logs"
    Write-Host "  $script:ComposeCmd down         # Stop Praxis"
    Write-Host "  $script:ComposeCmd up -d        # Start Praxis"
    Write-Host "  $script:ComposeCmd up --build   # Rebuild and start"
    Write-Host ""
}

function Run-Docker {
    Check-Docker
    Get-LatestVersion
    Clone-Repo-Docker
    Start-Praxis-Docker
    Print-Docker-Summary
}

#
# === Remove ==================================================================
#

function Remove-Praxis {
    Write-Info "Removing Praxis (Docker install)..."

    if (Test-Path $PraxisDir) {
        try {
            Push-Location $PraxisDir
            if (Test-Command "docker") {
                docker compose down -v *> $null
                if ($LASTEXITCODE -ne 0) {
                    if (Test-Command "docker-compose") { docker-compose down -v *> $null }
                }
            }
            Pop-Location
        } catch {
            try { Pop-Location } catch {}
        }

        Remove-Item -Recurse -Force $PraxisDir
        Write-Success "Removed $PraxisDir"
    } else {
        Write-Warn "No install found at $PraxisDir"
    }

    Write-Host ""
    Write-Success "Praxis has been removed."
    Write-Host ""
}

#
# === Menu / dispatch =========================================================
#

function Interactive-Menu {
    $options = @(
        "Docker install   - run via docker compose (recommended on Windows)",
        "Quit"
    )

    $idx = Select-Menu -Prompt "Choose how to install Praxis" -Options $options
    Write-Host ""

    switch ($idx) {
        0 { Run-Docker }
        1 { Write-Host "Aborted."; exit 0 }
    }
}

#
# Main.
#

Print-Banner

if ($Help) {
    Print-Usage
    exit 0
}

Assert-Windows

if ($Remove) { Remove-Praxis;       exit 0 }
if ($Docker) { Run-Docker;          exit 0 }

# Legacy positional flags (e.g. piped --remove via iex) are unusual on Windows;
# rely on the parameters above and fall through to the menu otherwise.
Interactive-Menu
