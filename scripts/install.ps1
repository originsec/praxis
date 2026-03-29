#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Installation Script for Windows
.DESCRIPTION
    Installs Praxis service, web UI, and node agent
.EXAMPLE
    irm https://praxis.originhq.com/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"

# Configuration
$HomeDir = if ($env:USERPROFILE) { $env:USERPROFILE } elseif ($env:HOME) { $env:HOME } else { "~" }
$PraxisHome = if ($env:PRAXIS_HOME) { $env:PRAXIS_HOME } else { Join-Path $HomeDir ".praxis" }
$PraxisBin = "$PraxisHome\bin"
$PraxisNodes = "$PraxisBin\nodes\windows"
$PraxisRepo = "originsec/praxis"
$PraxisVersion = $env:PRAXIS_VERSION

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
    Write-Host "Praxis Installation Script for Windows"
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

function Check-Prerequisites {
    Write-Info "Checking prerequisites..."

    # Check for git
    if (Test-Command "git") {
        Write-Success "Found git"
    } else {
        Write-Err "git not found. Please install git from https://git-scm.com/download/win"
    }

    # Check for Rust/Cargo
    if (Test-Command "cargo") {
        $rustVersion = (rustc --version) -replace "rustc ", ""
        Write-Success "Found Rust $rustVersion"
    } else {
        Write-Warn "Rust not found. Installing via rustup..."

        # Download and run rustup-init
        $rustupInit = "$env:TEMP\rustup-init.exe"
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit
        Start-Process -FilePath $rustupInit -ArgumentList "-y" -Wait -NoNewWindow
        Remove-Item $rustupInit -Force

        # Refresh PATH
        $env:PATH = [System.Environment]::GetEnvironmentVariable("PATH", "Machine") + ";" + [System.Environment]::GetEnvironmentVariable("PATH", "User")

        if (Test-Command "cargo") {
            Write-Success "Rust installed"
        } else {
            Write-Err "Failed to install Rust. Please install manually from https://rustup.rs"
        }
    }

    # Check Rust version (need 1.85+ for edition 2024)
    $rustVersion = (rustc --version) -replace "rustc (\d+)\.(\d+).*", '$1.$2'
    $major, $minor = $rustVersion -split '\.'
    if ([int]$major -lt 1 -or ([int]$major -eq 1 -and [int]$minor -lt 85)) {
        Write-Warn "Rust 1.85+ required. Updating..."
        rustup update stable
    }

    # Check for Node.js (for frontend build)
    if ((Test-Command "node") -and (Test-Command "npm")) {
        $nodeVersion = node --version
        Write-Success "Found Node.js $nodeVersion"
    } else {
        Write-Warn "Node.js not found. Frontend build may fail."
        Write-Warn "Install Node.js 18+ from https://nodejs.org"
    }

    Write-Host ""
}

function Install-Praxis {
    Write-Info "Creating directories..."
    New-Item -ItemType Directory -Force -Path $PraxisBin | Out-Null
    New-Item -ItemType Directory -Force -Path $PraxisNodes | Out-Null

    $repoUrl = "https://github.com/$PraxisRepo"

    Write-Info "Installing praxis_service, praxis_web, and praxis_cli..."
    cargo install --git $repoUrl --tag $script:PraxisVersion --root $PraxisHome praxis_service praxis_web praxis_cli
    Write-Success "Installed praxis_service, praxis_web, and praxis_cli"

    $nodeVersionFile = "$PraxisNodes\.praxis_node_version"
    if ((Test-Path "$PraxisNodes\praxis_node.exe") -and (Test-Path $nodeVersionFile) -and ((Get-Content $nodeVersionFile) -eq $script:PraxisVersion)) {
        Write-Success "praxis_node (Windows) $($script:PraxisVersion) already installed, skipping"
    } else {
        Write-Info "Installing praxis_node..."
        cargo install --git $repoUrl --tag $script:PraxisVersion --root $PraxisHome praxis_node
        Move-Item -Force "$PraxisBin\praxis_node.exe" "$PraxisNodes\"
        $script:PraxisVersion | Out-File -FilePath $nodeVersionFile -Encoding UTF8 -NoNewline
        Write-Success "Installed praxis_node"
    }

    Write-Host ""
}

function Install-Runner {
    Write-Info "Installing runner script..."

    $runnerScript = @'
#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Runner - starts service and web components
.EXAMPLE
    .\praxis.ps1
    .\praxis.ps1 -RabbitMqUrl "amqp://user:pass@host:5672"
#>

param(
    [string]$RabbitMqUrl = $env:PRAXIS_RABBITMQ_URL
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

if (-not $RabbitMqUrl) {
    $RabbitMqUrl = "amqp://guest:guest@localhost:5672"
}

$env:PRAXIS_RABBITMQ_URL = $RabbitMqUrl

$ServiceProc = $null
$WebProc = $null

function Cleanup {
    Write-Host ""
    Write-Host "Shutting down Praxis..."
    if ($WebProc -and !$WebProc.HasExited) {
        $WebProc.Kill()
        $WebProc.WaitForExit(5000)
    }
    if ($ServiceProc -and !$ServiceProc.HasExited) {
        $ServiceProc.Kill()
        $ServiceProc.WaitForExit(5000)
    }
    Write-Host "Praxis stopped."
}

try {
    Write-Host "Starting Praxis..."
    Write-Host "  RabbitMQ: $RabbitMqUrl"
    Write-Host ""

    $ServiceProc = Start-Process -FilePath "$ScriptDir\praxis_service.exe" -PassThru -NoNewWindow
    Start-Sleep -Seconds 1

    if ($ServiceProc.HasExited) {
        Write-Host "Error: praxis_service failed to start" -ForegroundColor Red
        exit 1
    }

    $WebProc = Start-Process -FilePath "$ScriptDir\praxis_web.exe" -PassThru -NoNewWindow

    Write-Host "Praxis running. Press Ctrl+C to stop."
    Write-Host "  Web UI: http://localhost:8080"
    Write-Host ""

    # Wait for either process to exit
    while (!$ServiceProc.HasExited -and !$WebProc.HasExited) {
        Start-Sleep -Milliseconds 500
    }
}
finally {
    Cleanup
}
'@

    $runnerScript | Out-File -FilePath "$PraxisBin\praxis.ps1" -Encoding UTF8
    Write-Success "Installed praxis.ps1 runner"
    Write-Host ""
}

$script:PathUpdated = $false

function Update-ShellPath {
    $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($currentPath -and $currentPath.Contains($PraxisBin)) {
        Write-Success "PATH already configured"
    } else {
        Write-Info "Adding $PraxisBin to user PATH..."
        $newPath = if ($currentPath) { "$currentPath;$PraxisBin" } else { $PraxisBin }
        [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
        $env:PATH = "$env:PATH;$PraxisBin"
        $script:PathUpdated = $true
        Write-Success "Updated user PATH"
    }
    Write-Host ""
}

function Print-Summary {
    Write-Host ""
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host "  Praxis $script:PraxisVersion installation complete!" -ForegroundColor Green
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Installed to: $PraxisHome"
    Write-Host ""
    Write-Host "Binaries:"
    Write-Host "  $PraxisBin\praxis_service.exe"
    Write-Host "  $PraxisBin\praxis_web.exe"
    Write-Host "  $PraxisBin\praxis_cli.exe"
    Write-Host "  $PraxisBin\praxis.ps1"
    Write-Host ""
    Write-Host "Node agent:"
    Write-Host "  $PraxisNodes\praxis_node.exe"
    Write-Host ""
    if ($script:PathUpdated) {
        Write-Host "PATH updated. Restart your terminal for changes to take effect." -ForegroundColor Yellow
        Write-Host ""
    }
    Write-Host "Usage:" -ForegroundColor Cyan
    Write-Host "  .\praxis.ps1                                  # Default RabbitMQ"
    Write-Host "  .\praxis.ps1 -RabbitMqUrl amqp://host:5672    # Custom RabbitMQ"
    Write-Host ""
    Write-Host "Web UI: http://localhost:8080"
    Write-Host ""

    #
    # Check if RabbitMQ is reachable by parsing the configured URL.
    #

    $rabbitmqUrl = $env:PRAXIS_RABBITMQ_URL
    if (-not $rabbitmqUrl) { $rabbitmqUrl = "amqp://guest:guest@localhost:5672" }

    $rabbitmqHost = "localhost"
    $rabbitmqPort = 5672
    if ($rabbitmqUrl -match 'amqps?://(?:[^@]+@)?([^:/?]+):(\d+)') {
        $rabbitmqHost = $Matches[1]
        $rabbitmqPort = [int]$Matches[2]
    } elseif ($rabbitmqUrl -match 'amqps?://(?:[^@]+@)?([^:/?]+)') {
        $rabbitmqHost = $Matches[1]
    }

    try {
        $tcp = New-Object System.Net.Sockets.TcpClient
        $tcp.Connect($rabbitmqHost, $rabbitmqPort)
        $tcp.Close()
        Write-Success "RabbitMQ is reachable at ${rabbitmqHost}:${rabbitmqPort}"
    } catch {
        Write-Warn "RabbitMQ does not appear to be running at ${rabbitmqHost}:${rabbitmqPort}"
        Write-Host "  Praxis requires RabbitMQ. Install and start it before launching Praxis."
        Write-Host ""
    }
}

function Remove-Praxis {
    Write-Info "Removing Praxis..."

    #
    # Stop running processes.
    #

    $praxisProcesses = Get-Process -Name "praxis_service", "praxis_web", "praxis_node", "praxis_cli" -ErrorAction SilentlyContinue
    if ($praxisProcesses) {
        Write-Info "Stopping running processes..."
        $praxisProcesses | Stop-Process -Force
        Write-Success "Stopped processes"
    }

    #
    # Remove binaries.
    #

    if (Test-Path $PraxisHome) {
        Remove-Item -Recurse -Force $PraxisHome
        Write-Success "Removed $PraxisHome"
    }

    #
    # Remove PATH entry.
    #

    $currentPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($currentPath -and $currentPath.Contains($PraxisBin)) {
        $newPath = ($currentPath -split ";" | Where-Object { $_ -ne $PraxisBin }) -join ";"
        [Environment]::SetEnvironmentVariable("PATH", $newPath, "User")
        Write-Success "Removed PATH entry"
    }

    Write-Host ""
    Write-Success "Praxis has been removed."
    Write-Host ""
}

# Main
Print-Banner

if ($args -contains "--remove") {
    Remove-Praxis
    exit 0
}

Get-LatestVersion
Check-Prerequisites
Install-Praxis
Install-Runner
Update-ShellPath
Print-Summary
