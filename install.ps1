# gh0st Installation Script for Windows
# This script downloads and installs the latest release of gh0st

param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\gh0st"
)

$ErrorActionPreference = "Stop"

$Repo = "yourusername/gh0st"
$BinaryName = "gh0st.exe"

# Colors for output
function Write-ColorOutput {
    param(
        [string]$Message,
        [string]$Color = "White"
    )
    Write-Host $Message -ForegroundColor $Color
}

function Write-Error-Message {
    param([string]$Message)
    Write-ColorOutput "Error: $Message" "Red"
}

function Write-Success {
    param([string]$Message)
    Write-ColorOutput $Message "Green"
}

function Write-Info {
    param([string]$Message)
    Write-ColorOutput $Message "Yellow"
}

# Detect architecture
function Get-Architecture {
    $arch = [System.Environment]::Is64BitOperatingSystem
    if ($arch) {
        return "x86_64"
    }
    else {
        Write-Error-Message "32-bit Windows is not supported"
        exit 1
    }
}

# Get the latest release version
function Get-LatestVersion {
    try {
        $apiUrl = "https://api.github.com/repos/$Repo/releases/latest"
        $response = Invoke-RestMethod -Uri $apiUrl -Method Get
        return $response.tag_name.TrimStart('v')
    }
    catch {
        Write-Error-Message "Could not determine latest version: $_"
        exit 1
    }
}

# Download and install
function Install-Gh0st {
    $arch = Get-Architecture

    if ($Version -eq "latest") {
        Write-Info "Fetching latest version..."
        $ver = Get-LatestVersion
        if ([string]::IsNullOrEmpty($ver)) {
            Write-Error-Message "Could not determine latest version"
            exit 1
        }
    }
    else {
        $ver = $Version
    }

    Write-Info "Installing gh0st v$ver for Windows-$arch..."

    $archiveName = "gh0st-windows-$arch.zip"
    $downloadUrl = "https://github.com/$Repo/releases/download/v$ver/$archiveName"

    # Create temporary directory
    $tmpDir = Join-Path $env:TEMP "gh0st-install-$(Get-Random)"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        # Download
        Write-Info "Downloading from $downloadUrl..."
        $archivePath = Join-Path $tmpDir $archiveName

        try {
            Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing
        }
        catch {
            Write-Error-Message "Failed to download gh0st: $_"
            exit 1
        }

        # Extract
        Write-Info "Extracting archive..."
        Expand-Archive -Path $archivePath -DestinationPath $tmpDir -Force

        # Create install directory if it doesn't exist
        if (-not (Test-Path $InstallDir)) {
            New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
        }

        # Install binary
        Write-Info "Installing to $InstallDir..."
        $sourceBinary = Join-Path $tmpDir $BinaryName
        $targetBinary = Join-Path $InstallDir $BinaryName

        if (Test-Path $targetBinary) {
            Write-Info "Removing existing installation..."
            Remove-Item $targetBinary -Force
        }

        Copy-Item $sourceBinary $targetBinary -Force

        Write-Success "✓ gh0st v$ver installed successfully!"

        # Check if install directory is in PATH
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath -notlike "*$InstallDir*") {
            Write-Info ""
            Write-Info "Note: $InstallDir is not in your PATH."
            Write-Info "Would you like to add it to your PATH? (Y/N)"

            $response = Read-Host
            if ($response -eq "Y" -or $response -eq "y") {
                $newPath = "$userPath;$InstallDir"
                [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
                Write-Success "✓ Added to PATH. Please restart your terminal for changes to take effect."
            }
            else {
                Write-Info ""
                Write-Info "To add it manually, run:"
                Write-Info '    $env:Path += ";' + $InstallDir + '"'
                Write-Info ""
                Write-Info "To make it permanent, add it to your system or user PATH environment variable."
            }
        }

        Write-Info ""
        Write-Info "Run 'gh0st --help' to get started!"
        Write-Info "(You may need to restart your terminal first)"
    }
    finally {
        # Cleanup
        if (Test-Path $tmpDir) {
            Remove-Item $tmpDir -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

# Main execution
try {
    Write-Host "gh0st Installation Script for Windows" -ForegroundColor Cyan
    Write-Host "======================================" -ForegroundColor Cyan
    Write-Host ""

    Install-Gh0st
}
catch {
    Write-Error-Message "Installation failed: $_"
    exit 1
}
