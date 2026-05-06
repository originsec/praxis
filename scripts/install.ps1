#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Installation Script for Windows.
.DESCRIPTION
    The Praxis service is Linux-only, so on Windows it can only be
    installed via Docker. The CLI ('praxis') is always installed
    natively by compiling from source (requires Rust + git).

    The interactive menu first asks which components to install:
      [x] service   (Docker — rabbitmq + service container)
      [x] cli       (native — compiles 'praxis' for Windows)

.EXAMPLE
    irm https://praxis.originhq.com/install.ps1 | iex
.EXAMPLE
    .\install.ps1 -Service docker
.EXAMPLE
    .\install.ps1 -Cli
.EXAMPLE
    .\install.ps1 -Remove
#>

[CmdletBinding()]
param(
    [ValidateSet('docker')]
    [string]$Service,
    [switch]$Cli,
    [switch]$Remove,
    [switch]$Help
)

$ErrorActionPreference = "Stop"

$HomeDir       = if ($env:USERPROFILE) { $env:USERPROFILE } elseif ($env:HOME) { $env:HOME } else { "~" }
$PraxisRepo    = "originsec/praxis"
$PraxisVersion = $env:PRAXIS_VERSION
$PraxisDir     = if ($env:PRAXIS_DIR) { $env:PRAXIS_DIR } else { Join-Path $HomeDir ".praxis-docker" }
$CliInstallDir = if ($env:PRAXIS_CLI_DIR) { $env:PRAXIS_CLI_DIR } else { Join-Path $HomeDir ".praxis\bin" }
$ComposeCmd    = $null

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
    $cols = try { [Console]::WindowWidth } catch { 80 }
    if (-not $cols -or $cols -lt 1) { $cols = 80 }
    $logoW = 46; $tagW = 49; $byW = 13
    $lp = ' ' * [Math]::Max(0, [int](($cols - $logoW) / 2))
    $tp = ' ' * [Math]::Max(0, [int](($cols - $tagW)  / 2))
    $bp = ' ' * [Math]::Max(0, [int](($cols - $byW)   / 2))

    Write-Host ""
    Write-Host "$lp██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗" -ForegroundColor Cyan
    Write-Host "$lp██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝" -ForegroundColor Cyan
    Write-Host "$lp██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗" -ForegroundColor Cyan
    Write-Host "$lp██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║" -ForegroundColor Cyan
    Write-Host "$lp██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║" -ForegroundColor Cyan
    Write-Host "$lp╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝" -ForegroundColor Cyan
    Write-Host "${tp}Semantic Command & Control Framework for Agents" -ForegroundColor DarkGray
    Write-Host "${bp}by [Ø] Origin" -ForegroundColor Magenta
    Write-Host ""
}

function Print-Usage {
    @"
Usage: install.ps1 [flag]

Flags:
  -Service docker   Install service via Docker (only mode supported on Windows)
  -Cli              Install CLI natively (compiles 'praxis' from source)
  -Remove           Remove a previous install (CLI + Docker)
  -Help             Show this message

If no flag is given, an interactive menu is shown.
"@ | Write-Host
}

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
}

#
# Single-select arrow menu. Returns selected index.
#

function Select-Menu {
    param(
        [string]$Prompt,
        [string[]]$Options
    )

    $n = $Options.Length
    $sel = 0
    Write-Host $Prompt -NoNewline
    Write-Host " (↑↓ move, enter select, q quit)" -ForegroundColor DarkGray
    foreach ($_ in $Options) { Write-Host "" }

    [Console]::CursorVisible = $false
    try {
        while ($true) {
            [Console]::SetCursorPosition(0, [Console]::CursorTop - $n)
            for ($i = 0; $i -lt $n; $i++) {
                $blank = "".PadRight([Console]::WindowWidth - 1)
                [Console]::Write("`r$blank`r")
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
# Multi-select checkbox menu. $Initial: array of [bool] same length as $Options.
# Returns array of [bool].
#

function MultiSelect-Menu {
    param(
        [string]$Prompt,
        [string[]]$Options,
        [bool[]]$Initial
    )

    $n = $Options.Length
    $checked = @($Initial)
    if ($checked.Length -lt $n) {
        $checked = @(1..$n | ForEach-Object { $true })
    }
    $sel = 0

    Write-Host $Prompt -NoNewline
    Write-Host " (↑↓ move, space toggle, enter next, q quit)" -ForegroundColor DarkGray
    foreach ($_ in $Options) { Write-Host "" }

    [Console]::CursorVisible = $false
    try {
        while ($true) {
            [Console]::SetCursorPosition(0, [Console]::CursorTop - $n)
            for ($i = 0; $i -lt $n; $i++) {
                $blank = "".PadRight([Console]::WindowWidth - 1)
                [Console]::Write("`r$blank`r")
                $mark = if ($checked[$i]) { "[x]" } else { "[ ]" }
                $color = if ($checked[$i]) { "Cyan" } else { "DarkGray" }
                if ($i -eq $sel) {
                    Write-Host "  > " -NoNewline -ForegroundColor Cyan
                    Write-Host "$mark " -NoNewline -ForegroundColor $color
                    Write-Host "$($Options[$i])"
                } else {
                    Write-Host "    " -NoNewline
                    Write-Host "$mark " -NoNewline -ForegroundColor $color
                    Write-Host "$($Options[$i])" -ForegroundColor DarkGray
                }
            }
            $key = [Console]::ReadKey($true)
            switch ($key.Key) {
                'UpArrow'   { $sel = ($sel - 1 + $n) % $n }
                'DownArrow' { $sel = ($sel + 1) % $n }
                'K'         { $sel = ($sel - 1 + $n) % $n }
                'J'         { $sel = ($sel + 1) % $n }
                'Spacebar'  { $checked[$sel] = -not $checked[$sel] }
                'Enter'     { return $checked }
                'Q'         { [Console]::CursorVisible = $true; Write-Host ""; exit 130 }
                'Escape'    { [Console]::CursorVisible = $true; Write-Host ""; exit 130 }
            }
        }
    } finally {
        [Console]::CursorVisible = $true
    }
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
    if (-not $script:PraxisVersion) { Write-Err "Could not determine latest version." }
    Write-Success "Latest version: $script:PraxisVersion"
    Write-Host ""
}

#
# === CLI native install (Windows) ==========================================
#

function Ensure-Rust {
    if (Test-Command "cargo") {
        Write-Success "Found Rust $((rustc --version) -split ' ' | Select-Object -Index 1)"
        return
    }
    Write-Warn "Rust not found. Install from https://rustup.rs and re-run."
    Write-Err  "Cannot build CLI without Rust toolchain."
}

function Install-Cli {
    if (-not (Test-Command "git"))   { Write-Err "git not found. Install git from https://git-scm.com/download/win" }
    Ensure-Rust

    Write-Info "Building Praxis CLI for Windows..."
    if (-not (Test-Path $CliInstallDir)) { New-Item -ItemType Directory -Force -Path $CliInstallDir | Out-Null }

    $repoUrl = "https://github.com/$PraxisRepo"
    cargo install --git $repoUrl --tag $script:PraxisVersion --root $CliInstallDir.TrimEnd('\') praxis_cli
    if ($LASTEXITCODE -ne 0) { Write-Err "cargo install failed." }

    $exe = Join-Path $CliInstallDir "bin\praxis_cli.exe"
    if (-not (Test-Path $exe)) { Write-Err "Build succeeded but praxis_cli.exe not found at $exe" }

    #
    # Create a `praxis.exe` copy alongside (Windows doesn't follow
    # symlinks well by default). The CLI derives its display name
    # from argv[0], so this gives users a clean `praxis` command.
    #

    $praxisCopy = Join-Path $CliInstallDir "bin\praxis.exe"
    Copy-Item -Force $exe $praxisCopy

    #
    # Add to user PATH if not already present.
    #

    $binDir = (Resolve-Path (Join-Path $CliInstallDir "bin")).Path
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not ($userPath -split ';' | Where-Object { $_ -ieq $binDir })) {
        $newPath = if ($userPath) { "$userPath;$binDir" } else { $binDir }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Success "Added $binDir to user PATH (open a new shell to use 'praxis')"
    } else {
        Write-Success "$binDir already in user PATH"
    }

    Write-Success "Installed: $praxisCopy"
    Write-Host ""
}

#
# === Docker service install ================================================
#

function Check-Docker {
    Write-Info "Checking Docker..."
    if (-not (Test-Command "docker")) {
        Write-Err "Docker not found. Install Docker Desktop: https://www.docker.com/products/docker-desktop/"
    }
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
    } elseif (Test-Command "docker-compose") {
        $script:ComposeCmd = "docker-compose"
    } else {
        Write-Err "Docker Compose not found. Install Docker Desktop, which includes Compose."
    }
    Write-Success "Found $script:ComposeCmd"
    if (-not (Test-Command "git")) {
        Write-Err "git not found. Install git from https://git-scm.com/download/win"
    }
    Write-Host ""
}

function Install-Service-Docker {
    Check-Docker
    Write-Info "Setting up Praxis $script:PraxisVersion in $PraxisDir..."
    if (Test-Path $PraxisDir) { Remove-Item -Recurse -Force $PraxisDir }
    git clone --depth 1 --branch $script:PraxisVersion "https://github.com/$PraxisRepo.git" $PraxisDir
    if ($LASTEXITCODE -ne 0) { Write-Err "git clone failed." }

    Push-Location $PraxisDir
    try {
        Write-Info "Building and starting (this may take a few minutes on first run)..."
        if ($script:ComposeCmd -eq "docker compose") {
            docker compose up --build -d
        } else {
            & $script:ComposeCmd up --build -d
        }
        if ($LASTEXITCODE -ne 0) { Write-Err "Failed to start Praxis containers." }
    } finally {
        Pop-Location
    }
    Write-Success "Praxis is running"
    Write-Host ""
}

function Print-Docker-Summary {
    Write-Host ""
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host "  Praxis $script:PraxisVersion (docker) is ready"   -ForegroundColor Green
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Web UI:              http://localhost:8080"
    Write-Host "RabbitMQ Management: http://localhost:15672 (praxis / praxis)"
    Write-Host "Installation:        $PraxisDir"
    Write-Host ""
    Write-Host "Inside the container (systemd-managed):" -ForegroundColor Cyan
    Write-Host "  $script:ComposeCmd exec praxis praxisctl status"
    Write-Host "  $script:ComposeCmd exec praxis praxisctl webserver disable"
    Write-Host "  $script:ComposeCmd exec praxis praxisctl set rabbitmq-url <url>"
    Write-Host ""
    Write-Host "Compose lifecycle:" -ForegroundColor Cyan
    Write-Host "  cd $PraxisDir"
    Write-Host "  $script:ComposeCmd logs -f"
    Write-Host "  $script:ComposeCmd down"
    Write-Host "  $script:ComposeCmd up -d"
    Write-Host ""
}

function Print-Cli-Summary {
    Write-Host ""
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host "  Praxis CLI installed"                            -ForegroundColor Green
    Write-Host "==============================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Run:                 praxis"
    Write-Host "Set rabbitmq URL:    praxis set-rabbitmqurl amqp://praxis:praxis@host:5672"
    Write-Host "Config file:         $env:USERPROFILE\.config\praxis\config (or %APPDATA%\praxis\config)"
    Write-Host ""
}

#
# === Remove ================================================================
#

function Remove-All {
    if (Test-Path $PraxisDir) {
        Write-Info "Removing docker install at $PraxisDir..."
        try {
            Push-Location $PraxisDir
            if (Test-Command "docker") {
                docker compose down -v *> $null
                if ($LASTEXITCODE -ne 0 -and (Test-Command "docker-compose")) {
                    docker-compose down -v *> $null
                }
            }
            Pop-Location
        } catch {
            try { Pop-Location } catch {}
        }
        Remove-Item -Recurse -Force $PraxisDir
        Write-Success "Removed $PraxisDir"
    }

    if (Test-Path $CliInstallDir) {
        Write-Info "Removing CLI install at $CliInstallDir..."
        Remove-Item -Recurse -Force $CliInstallDir
        Write-Success "Removed $CliInstallDir"

        $binDir = $CliInstallDir.TrimEnd('\') + "\bin"
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $newPath = ($userPath -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ine $binDir) }) -join ';'
        if ($newPath -ne $userPath) {
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            Write-Success "Removed $binDir from user PATH"
        }
    }

    Write-Host ""
    Write-Success "Praxis has been removed."
    Write-Host ""
}

#
# === Interactive flow ======================================================
#

function Interactive-Install {
    $checked = MultiSelect-Menu `
        -Prompt "Choose components to install" `
        -Options @(
            "service   - Praxis service + web",
            "cli       - Praxis CLI"
        ) `
        -Initial @($true, $true)

    Write-Host ""
    if (-not $checked[0] -and -not $checked[1]) { Write-Err "Nothing selected." }

    Get-LatestVersion

    if ($checked[1]) { Install-Cli }
    if ($checked[0]) { Install-Service-Docker; Print-Docker-Summary }
    if ($checked[1] -and -not $checked[0]) { Print-Cli-Summary }
}

#
# Main.
#

Print-Banner
if ($Help)    { Print-Usage; exit 0 }
Assert-Windows
if ($Remove)  { Remove-All; exit 0 }

if ($Service -or $Cli) {
    Get-LatestVersion
    if ($Cli)               { Install-Cli; Print-Cli-Summary }
    if ($Service -eq 'docker') { Install-Service-Docker; Print-Docker-Summary }
    exit 0
}

Interactive-Install
