# Zenith CLI installer for Windows (PowerShell)
#
# Install latest stable:
#   irm https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.ps1 | iex
#
# Install a specific version:
#   $v="0.1.0"; irm https://raw.githubusercontent.com/farhan-syah/zenith/main/scripts/install.ps1 | iex
#
# Build and install from a local source checkout:
#   .\scripts\install.ps1 -Local

param(
    [String]$Version = "latest",
    [Switch]$Local,
    [Switch]$NoModifyPath
)

$ErrorActionPreference = "Stop"

$Repo = "farhan-syah/zenith"
$BinName = "zenith.exe"

function Install-Zenith {
    if ($Local -and $Version -ne "latest") {
        Write-Error "-Local cannot be combined with -Version."
        return
    }

    # Detect architecture
    $Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    switch ($Arch) {
        "X64"   { $Target = "windows-x64" }
        "Arm64" { $Target = "windows-arm64" }
        default {
            Write-Error "Unsupported architecture: $Arch"
            return
        }
    }

    # Determine install directory
    $InstallDir = if ($env:ZENITH_INSTALL_DIR) {
        $env:ZENITH_INSTALL_DIR
    } else {
        Join-Path $env:USERPROFILE ".zenith\bin"
    }

    if ($Local) {
        Install-ZenithLocal $InstallDir
        return
    }

    # Resolve version
    if ($Version -eq "latest") {
        $Version = Get-LatestVersion
        if (-not $Version) {
            Write-Error "Failed to fetch latest version. Check https://github.com/$Repo/releases"
            return
        }
    }

    $VersionNum = $Version -replace "^v", ""

    # Check if already installed
    $ExistingBin = Join-Path $InstallDir $BinName
    if (Test-Path $ExistingBin) {
        try {
            $Current = (& $ExistingBin --version 2>$null) -replace "zenith ", ""
            if ($Current -eq $VersionNum) {
                Write-Host "zenith $VersionNum is already installed."
                return
            }
            Write-Host "Switching zenith $Current -> $VersionNum ($Target)..."
        } catch {
            Write-Host "Installing zenith $VersionNum ($Target)..."
        }
    } else {
        Write-Host "Installing zenith $VersionNum ($Target)..."
    }

    # Download
    $Url = "https://github.com/$Repo/releases/download/v$VersionNum/zenith-$VersionNum-$Target.zip"
    $TempDir = Join-Path ([System.IO.Path]::GetTempPath()) "zenith-install-$(Get-Random)"
    $ZipPath = Join-Path $TempDir "zenith.zip"

    New-Item -ItemType Directory -Path $TempDir -Force | Out-Null

    Write-Host "Downloading from GitHub Releases..."
    try {
        Invoke-WebRequest -Uri $Url -OutFile $ZipPath -UseBasicParsing
    } catch {
        Write-Error "Download failed. Check the version and try again.`n  $Url"
        return
    }

    # Extract
    Expand-Archive -Path $ZipPath -DestinationPath $TempDir -Force

    $BinPath = Join-Path $TempDir $BinName
    if (-not (Test-Path $BinPath)) {
        $BinPath = Get-ChildItem -Path $TempDir -Filter $BinName -Recurse | Select-Object -First 1 -ExpandProperty FullName
        if (-not $BinPath) {
            Write-Error "Binary not found in archive"
            return
        }
    }

    # Install
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $BinPath -Destination (Join-Path $InstallDir $BinName) -Force

    # Clean up
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue

    # Verify
    $InstalledBin = Join-Path $InstallDir $BinName
    Write-Host ""
    Write-Host "Installed zenith to $InstalledBin"
    try { & $InstalledBin --version } catch {}

    # Add to PATH if needed
    Add-ToPath $InstallDir

    Write-Host ""
    Write-Host "Get started:"
    Write-Host "  zenith --help"
    Write-Host "  zenith validate document.zen"
    Write-Host "  zenith render document.zen --png out.png"
}

function Install-ZenithLocal {
    param([String]$InstallDir)

    # Repo root is the parent of this script's directory (<root>\scripts).
    $Root = Resolve-Path (Join-Path $PSScriptRoot "..")

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Error "cargo not found. Install Rust from https://rustup.rs and try again."
        return
    }

    $TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "target" }
    $BuiltBin = Join-Path $TargetDir "release\$BinName"

    Write-Host "Building zenith from $Root (cargo build --release)..."
    Push-Location $Root
    try {
        & cargo build --release --bin zenith
        if ($LASTEXITCODE -ne 0) {
            Write-Error "cargo build failed."
            return
        }
    } finally {
        Pop-Location
    }

    if (-not (Test-Path $BuiltBin)) {
        Write-Error "Built binary not found at $BuiltBin"
        return
    }

    # Install
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $BuiltBin -Destination (Join-Path $InstallDir $BinName) -Force

    # Verify
    $InstalledBin = Join-Path $InstallDir $BinName
    Write-Host ""
    Write-Host "Installed zenith to $InstalledBin"
    try { & $InstalledBin --version } catch {}

    # Add to PATH if needed
    Add-ToPath $InstallDir

    Write-Host ""
    Write-Host "Get started:"
    Write-Host "  zenith --help"
    Write-Host "  zenith validate document.zen"
    Write-Host "  zenith render document.zen --png out.png"
}

function Get-LatestVersion {
    try {
        $Releases = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=10" -UseBasicParsing
        foreach ($Release in $Releases) {
            if (-not $Release.prerelease) {
                return $Release.tag_name -replace "^v", ""
            }
        }
    } catch {
        return $null
    }
    return $null
}

function Add-ToPath {
    param([String]$Dir)

    $UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -split ";" | Where-Object { $_ -eq $Dir }) {
        return
    }

    if ($NoModifyPath) {
        Write-Host ""
        Write-Host "Note: -NoModifyPath set; $Dir was not added to PATH."
        Write-Host "Add it yourself, or run: `$env:Path = `"$Dir;`$env:Path`""
        return
    }

    [System.Environment]::SetEnvironmentVariable("Path", "$Dir;$UserPath", "User")
    $env:Path = "$Dir;$env:Path"
    Write-Host ""
    Write-Host "Added $Dir to your PATH."
}

Install-Zenith
