#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Docker Installation Script for Powershell
.DESCRIPTION
    Clones the latest release and starts Praxis with Docker Compose
.EXAMPLE
    irm https://praxis.originhq.com/docker.ps1 | iex
#>

$ErrorActionPreference = "Stop"

# Configuration
$HomeDir = if ($env:USERPROFILE) { $env:USERPROFILE } elseif ($env:HOME) { $env:HOME } else { "~" }
$PraxisDir = if ($env:PRAXIS_DIR) { $env:PRAXIS_DIR } else { Join-Path $HomeDir ".praxis-docker" }
$PraxisRepo = "originsec/praxis"
$PraxisVersion = $env:PRAXIS_VERSION
$ComposeCmd = ""

# Colors
function Write-Info { param($msg) Write-Host "[INFO] " -ForegroundColor Cyan -NoNewline; Write-Host $msg }
function Write-Success { param($msg) Write-Host "[OK] " -ForegroundColor Green -NoNewline; Write-Host $msg }
function Write-Warn { param($msg) Write-Host "[WARN] " -ForegroundColor Yellow -NoNewline; Write-Host $msg }
function Write-Err { param($msg) Write-Host "[ERROR] " -ForegroundColor Red -NoNewline; Write-Host $msg; exit 1 }

function Print-Banner {
    Write-Host ""
    Write-Host "    ____                  _     " -ForegroundColor Cyan
    Write-Host "   / __ \_________ __  __(_)____" -ForegroundColor Cyan
    Write-Host "  / /_/ / ___/ __ ``/ |/_/ / ___/" -ForegroundColor Cyan
    Write-Host " / ____/ /  / /_/ />  </ (__  ) " -ForegroundColor Cyan
    Write-Host "/_/   /_/   \__,_/_/|_/_/____/  " -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Praxis Docker Setup"
    Write-Host "by [Ø] Origin"
    Write-Host ""
}

function Test-Command {
    param($cmd)
    $null = Get-Command $cmd -ErrorAction SilentlyContinue
    return $?
}

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

function Check-Docker {
    Write-Info "Checking prerequisites..."

    if (-not (Test-Command "docker")) {
        Write-Err "Docker not found. Please install Docker Desktop: https://docs.docker.com/desktop/install/windows-install/"
    }
    Write-Success "Found Docker"

    $dockerInfo = docker info 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Docker daemon not running. Please start Docker Desktop."
    }
    Write-Success "Docker daemon running"

    $composeVersion = docker compose version 2>&1
    if ($LASTEXITCODE -eq 0) {
        $script:ComposeCmd = "docker compose"
        Write-Success "Found Docker Compose (plugin)"
    } elseif (Test-Command "docker-compose") {
        $script:ComposeCmd = "docker-compose"
        Write-Success "Found docker-compose (standalone)"
    } else {
        Write-Err "Docker Compose not found. Please install Docker Compose."
    }

    if (-not (Test-Command "git")) {
        Write-Err "git not found. Please install git: https://git-scm.com/download/win"
    }
    Write-Success "Found git"

    Write-Host ""
}

function Clone-Repo {
    Write-Info "Setting up Praxis $script:PraxisVersion in $PraxisDir..."

    if (Test-Path $PraxisDir) {
        Remove-Item -Recurse -Force $PraxisDir
    }

    git clone --depth 1 --branch $script:PraxisVersion "https://github.com/$PraxisRepo.git" $PraxisDir
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Failed to clone repository."
    }

    Set-Location $PraxisDir

    Write-Success "Praxis $script:PraxisVersion ready"
    Write-Host ""
}

function Start-Praxis {
    Write-Info "Building and starting Praxis (this may take a few minutes on first run)..."
    Write-Host ""

    $cmdParts = $script:ComposeCmd -split ' '
    if ($cmdParts.Count -eq 2) {
        & $cmdParts[0] $cmdParts[1] up --build -d
    } else {
        & $script:ComposeCmd up --build -d
    }

    if ($LASTEXITCODE -ne 0) {
        Write-Err "Failed to start Praxis."
    }

    Write-Host ""
    Write-Success "Praxis is running!"
    Write-Host ""
}

function Print-Summary {
    Write-Host ""
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host "  Praxis $script:PraxisVersion is ready!" -ForegroundColor Green
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Web UI:              http://localhost:8080"
    Write-Host "RabbitMQ Management: http://localhost:15672"
    Write-Host "                     (praxis / praxis)"
    Write-Host ""
    Write-Host "Installation:        $PraxisDir"
    Write-Host ""
    Write-Host "Commands:" -ForegroundColor Cyan
    Write-Host "  cd $PraxisDir"
    Write-Host "  $script:ComposeCmd logs -f      # View logs"
    Write-Host "  $script:ComposeCmd down         # Stop Praxis"
    Write-Host "  $script:ComposeCmd up -d        # Start Praxis"
    Write-Host "  $script:ComposeCmd up --build   # Rebuild and start"
    Write-Host ""
}

# Main
Print-Banner
Check-Docker
Get-LatestVersion
Clone-Repo
Start-Praxis
Print-Summary
