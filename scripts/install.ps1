#Requires -Version 5.1
<#
.SYNOPSIS
    Praxis Installation Script for Windows.
.DESCRIPTION
    The Praxis service is Linux-only, so on Windows it can only be
    installed via Docker. The CLI ('praxis') is installed natively.

    By default the CLI is downloaded from the latest GitHub release.
    Pass -Src to build it from source instead (requires Rust + git).
    The Docker path always builds from source regardless of -Src.

    The interactive menu asks how to install the service:
      - Docker install (rabbitmq + service container)
      - Client only    (no service; just the praxis CLI)
    The CLI is always installed natively regardless of the choice.

.EXAMPLE
    irm https://praxis.originhq.com/install.ps1 | iex
.EXAMPLE
    .\install.ps1 -Service docker
.EXAMPLE
    .\install.ps1 -Cli
.EXAMPLE
    .\install.ps1 -Cli -Src
.EXAMPLE
    .\install.ps1 -Remove
#>

[CmdletBinding()]
param(
    [ValidateSet('docker')]
    [string]$Service,
    [switch]$Cli,
    [switch]$Src,
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
$BuildFromSource = [bool]$Src

function Write-Info    { param($msg) Write-Host "  ▸ " -ForegroundColor Cyan   -NoNewline; Write-Host $msg }
function Write-Success { param($msg) Write-Host "  ✓ " -ForegroundColor Green  -NoNewline; Write-Host $msg }
function Write-Warn    { param($msg) Write-Host "  ⚠ " -ForegroundColor Yellow -NoNewline; Write-Host $msg }
function Write-Err     { param($msg) Write-Host "  ✗ " -ForegroundColor Red    -NoNewline; Write-Host $msg; exit 1 }

function Write-Section {
    param([string]$Title)
    Write-Host ""
    Write-Host "  ▌ " -ForegroundColor Cyan -NoNewline
    Write-Host $Title
    Write-Host ""
}

#
# Run an external command in the background, streaming its output to a
# log file, and draw a fake-progress bar while we wait. Mirrors the
# `run_with_progress_bar` helper in install.sh.
#
# Returns the command's exit code; on failure, dumps the last 50 log
# lines so the user can see what went wrong.
#

function Run-WithProgressBar {
    param(
        [string]$LogFile,
        [string]$Exe,
        [string[]]$ExeArgs
    )

    $proc = Start-Process -FilePath $Exe -ArgumentList $ExeArgs `
        -RedirectStandardOutput $LogFile -RedirectStandardError "$LogFile.err" `
        -NoNewWindow -PassThru

    $width = 40
    $percent = 0
    $step = 2
    $delayMs = 300
    $spin = @('⣾','⣽','⣻','⢿','⡿','⣟','⣯','⣷')
    $spinIdx = 0

    while (-not $proc.HasExited) {
        $filled = [int]($percent * $width / 100)
        $empty = $width - $filled
        $bar = ('█' * $filled) + ('░' * $empty)
        Write-Host -NoNewline ("`r" + (' ' * ([Console]::WindowWidth - 1)) + "`r")
        Write-Host -NoNewline "[" -ForegroundColor Cyan
        Write-Host -NoNewline ('█' * $filled) -ForegroundColor Cyan
        Write-Host -NoNewline ('░' * $empty)  -ForegroundColor DarkGray
        Write-Host -NoNewline "] " -ForegroundColor Cyan
        Write-Host -NoNewline ("{0,3}% {1}" -f $percent, $spin[$spinIdx]) -ForegroundColor Cyan

        Start-Sleep -Milliseconds $delayMs
        $spinIdx = ($spinIdx + 1) % $spin.Length
        $percent += $step
        if ($percent -gt 95) { $percent = 95 }
    }

    Write-Host -NoNewline ("`r" + (' ' * ([Console]::WindowWidth - 1)) + "`r")
    Write-Host -NoNewline "[" -ForegroundColor Cyan
    Write-Host -NoNewline ('█' * $width) -ForegroundColor Cyan
    Write-Host "] 100%" -ForegroundColor Cyan

    if (Test-Path "$LogFile.err") {
        Get-Content "$LogFile.err" | Add-Content $LogFile
        Remove-Item "$LogFile.err" -Force
    }

    return $proc.ExitCode
}

#
# Detect whether install.ps1 is being run from inside a praxis git
# checkout; returns the absolute path of the repo root or $null.
#

function Get-LocalRepoRoot {
    if (-not $PSScriptRoot) { return $null }
    $candidate = Resolve-Path (Join-Path $PSScriptRoot "..") -ErrorAction SilentlyContinue
    if (-not $candidate) { return $null }
    $candidate = $candidate.Path
    if ((Test-Path (Join-Path $candidate "docker-compose.yml")) -and `
        (Test-Path (Join-Path $candidate "Dockerfile"))) {
        return $candidate
    }
    return $null
}

function Test-Command {
    param($cmd)
    $null = Get-Command $cmd -ErrorAction SilentlyContinue
    return $?
}

function Print-Banner {
    Write-Host ""
    Write-Host "██████╗ ██████╗  █████╗ ██╗  ██╗██╗███████╗" -ForegroundColor Cyan
    Write-Host "██╔══██╗██╔══██╗██╔══██╗╚██╗██╔╝██║██╔════╝" -ForegroundColor Cyan
    Write-Host "██████╔╝██████╔╝███████║ ╚███╔╝ ██║███████╗" -ForegroundColor Cyan
    Write-Host "██╔═══╝ ██╔══██╗██╔══██║ ██╔██╗ ██║╚════██║" -ForegroundColor Cyan
    Write-Host "██║     ██║  ██║██║  ██║██╔╝ ██╗██║███████║" -ForegroundColor Cyan
    Write-Host "╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚══════╝" -ForegroundColor Cyan
    Write-Host "Semantic Command & Control Framework for Agents " -ForegroundColor DarkGray -NoNewline
    Write-Host "by [Ø] Origin" -ForegroundColor Magenta
    Write-Host ""
}

function Print-Usage {
    @"
Usage: install.ps1 [flag]

Flags:
  -Service docker   Install service via Docker (only mode supported on Windows)
  -Cli              Install CLI natively
  -Src              Build the CLI from source instead of downloading the
                    prebuilt release binary (default). No effect on
                    -Service docker, which always builds from source.
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
        [string[]]$Options,
        [string]$Footer = ""
    )

    $n = $Options.Length
    $sel = 0
    Write-Host $Prompt -NoNewline
    Write-Host " (↑↓ move, enter select, q quit)" -ForegroundColor DarkGray
    Write-Host ""
    foreach ($_ in $Options) { Write-Host "" }

    #
    # Optional footer below the option list. Drawn once; the option redraw
    # only walks back up across the option rows so the footer stays put.
    #

    $footerLines = 0
    if ($Footer) {
        Write-Host ""
        Write-Host $Footer -ForegroundColor DarkGray
        $footerLines = 2
        [Console]::SetCursorPosition(0, [Console]::CursorTop - $footerLines)
    }

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
                'Enter'     {
                    if ($footerLines -gt 0) { [Console]::SetCursorPosition(0, [Console]::CursorTop + $footerLines) }
                    return $sel
                }
                'Spacebar'  {
                    if ($footerLines -gt 0) { [Console]::SetCursorPosition(0, [Console]::CursorTop + $footerLines) }
                    return $sel
                }
                'Q'         {
                    [Console]::CursorVisible = $true
                    if ($footerLines -gt 0) { [Console]::SetCursorPosition(0, [Console]::CursorTop + $footerLines) }
                    Write-Host ""; exit 130
                }
                'Escape'    {
                    [Console]::CursorVisible = $true
                    if ($footerLines -gt 0) { [Console]::SetCursorPosition(0, [Console]::CursorTop + $footerLines) }
                    Write-Host ""; exit 130
                }
                default {
                    if ($key.KeyChar -match '^[1-9]$') {
                        $idx = [int]$key.KeyChar.ToString() - 1
                        if ($idx -lt $n) {
                            if ($footerLines -gt 0) { [Console]::SetCursorPosition(0, [Console]::CursorTop + $footerLines) }
                            return $idx
                        }
                    }
                }
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
    Write-Section "Installing CLI"
    if (-not (Test-Path $CliInstallDir)) { New-Item -ItemType Directory -Force -Path $CliInstallDir | Out-Null }
    $binDir = Join-Path $CliInstallDir "bin"
    if (-not (Test-Path $binDir)) { New-Item -ItemType Directory -Force -Path $binDir | Out-Null }
    $exe = Join-Path $binDir "praxis_cli.exe"

    if ($script:BuildFromSource) {
        if (-not (Test-Command "git"))   { Write-Err "git not found. Install git from https://git-scm.com/download/win" }
        Ensure-Rust

        Write-Info "Building Praxis CLI for Windows..."
        $repoUrl = "https://github.com/$PraxisRepo"
        $cliRoot = $CliInstallDir.TrimEnd('\')
        $cargoLog = Join-Path $env:TEMP "praxis-cargo-install.log"

        $exitCode = Run-WithProgressBar -LogFile $cargoLog -Exe "cargo" -ExeArgs @(
            "install", "--git", $repoUrl, "--tag", $script:PraxisVersion,
            "--root", $cliRoot, "praxis_cli"
        )
        if ($exitCode -ne 0) {
            Write-Host ""
            Write-Warn "Build output (last 50 lines):"
            Get-Content $cargoLog -Tail 50
            Write-Err "cargo install failed."
        }
        if (-not (Test-Path $exe)) { Write-Err "Build succeeded but praxis_cli.exe not found at $exe" }
    } else {
        $asset = "praxis_cli-windows-x86_64.exe"
        $url = "https://github.com/$PraxisRepo/releases/download/$($script:PraxisVersion)/$asset"
        Write-Info "Downloading $asset..."
        try {
            Invoke-WebRequest -Uri $url -OutFile $exe -UseBasicParsing
        } catch {
            Write-Err "Failed to download $url - $($_.Exception.Message)"
        }
        if (-not (Test-Path $exe)) { Write-Err "Download did not produce $exe" }
        Write-Success "Downloaded $asset"
    }

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
    Write-Section "Installing Service (Docker)"
    Check-Docker

    #
    # If we're running from a local praxis checkout, build directly
    # against it instead of cloning the tagged release into
    # ~/.praxis-docker. Mirrors install.sh.
    #

    $localRoot = Get-LocalRepoRoot
    if ($localRoot) {
        Write-Info "Using local repository at $localRoot"
        $composeDir = $localRoot
    } else {
        Write-Info "Setting up Praxis $script:PraxisVersion in $PraxisDir..."
        if (Test-Path $PraxisDir) { Remove-Item -Recurse -Force $PraxisDir }
        git clone --depth 1 --branch $script:PraxisVersion "https://github.com/$PraxisRepo.git" $PraxisDir
        if ($LASTEXITCODE -ne 0) { Write-Err "git clone failed." }
        $composeDir = $PraxisDir
    }

    Push-Location $composeDir
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

    $script:PraxisDir = $composeDir
}

function Print-Summary-Box {
    param([string]$Title)
    $inner = 46
    $pad = [Math]::Max(0, [int](($inner - $Title.Length) / 2))
    $lpad = ' ' * $pad
    $rpad = ' ' * [Math]::Max(0, $inner - $Title.Length - $pad)
    $hbar = '─' * $inner
    Write-Host ""
    Write-Host "  ╭$hbar╮" -ForegroundColor Green
    Write-Host "  │" -ForegroundColor Green -NoNewline
    Write-Host "$lpad" -NoNewline
    Write-Host "$Title" -ForegroundColor Green -NoNewline
    Write-Host "$rpad" -NoNewline
    Write-Host "│" -ForegroundColor Green
    Write-Host "  ╰$hbar╯" -ForegroundColor Green
    Write-Host ""
}

function Print-Docker-Summary {
    Print-Summary-Box "Praxis $script:PraxisVersion (docker) ready"
    Write-Host "  RabbitMQ Management " -NoNewline; Write-Host "http://localhost:15672 " -NoNewline; Write-Host "(praxis / praxis)" -ForegroundColor DarkGray
    Write-Host "  Installation        $PraxisDir"
    Write-Host ""
    Write-Host "  Inside the container (systemd-managed)" -ForegroundColor Cyan
    Write-Host "    $script:ComposeCmd exec praxis praxisctl status"
    Write-Host "    $script:ComposeCmd exec praxis praxisctl set-rabbitmqurl <url>"
    Write-Host ""
    Write-Host "  Compose lifecycle" -ForegroundColor Cyan
    Write-Host "    cd $PraxisDir"
    Write-Host "    $script:ComposeCmd logs -f"
    Write-Host "    $script:ComposeCmd down"
    Write-Host "    $script:ComposeCmd up -d"
    Write-Host ""
}

function Print-Cli-Summary {
    Print-Summary-Box "Praxis CLI installed"
    Write-Host "  Binary       $CliInstallDir\bin\praxis.exe"
    Write-Host "  Config file  $env:USERPROFILE\.config\praxis\config " -NoNewline
    Write-Host "(or %APPDATA%\praxis\config)" -ForegroundColor DarkGray
    Write-Host ""
    Write-Host "  CLI" -ForegroundColor Cyan
    Write-Host "    praxis                      " -NoNewline; Write-Host "# interactive TUI" -ForegroundColor DarkGray
    Write-Host "    praxis set-rabbitmqurl amqp://praxis:praxis@localhost:5672"
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

function Prompt-BinaryOrSource {
    $idx = Select-Menu `
        -Prompt "CLI install method" `
        -Options @(
            "Download prebuilt binary from GitHub (recommended)",
            "Build from source (requires Rust + git)"
        ) `
        -Footer "Docker installs always build from source regardless of this choice."
    Write-Host ""
    if ($idx -eq 1) { $script:BuildFromSource = $true }
}

function Interactive-Install {
    $idx = Select-Menu `
        -Prompt "Install service as" `
        -Options @(
            "Docker install   - rabbitmq + service in containers",
            "Client only      - install only the praxis CLI",
            "Cancel"
        ) `
        -Footer "Note: client will always be installed natively."
    Write-Host ""

    switch ($idx) {
        0 {
            Prompt-BinaryOrSource
            Get-LatestVersion
            Install-Cli
            Install-Service-Docker
            Print-Docker-Summary
        }
        1 {
            Prompt-BinaryOrSource
            Get-LatestVersion
            Install-Cli
            Print-Cli-Summary
        }
        2 { Write-Err "Aborted." }
    }
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
