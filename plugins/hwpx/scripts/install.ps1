param(
    [switch]$NoBuild,
    [switch]$Uninstall,
    [switch]$PrintEnv
)

$ErrorActionPreference = "Stop"

if ((@($Uninstall, $PrintEnv) | Where-Object { $_ }).Count -gt 1) {
    throw "-Uninstall and -PrintEnv cannot be used together"
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$binaryName = "officecli-dump-reader-hwpx.exe"
$builtBinary = Join-Path $repoRoot "target\release\$binaryName"
$installDirectory = Join-Path $HOME ".officecli\plugins\dump-reader\hwpx"
$installPath = Join-Path $installDirectory "plugin.exe"

if ($Uninstall) {
    if (Test-Path -LiteralPath $installPath) {
        Remove-Item -LiteralPath $installPath -Force
        Write-Host "removed $installPath"
    } else {
        Write-Host "not installed: $installPath"
    }
    exit 0
}

if ($PrintEnv) {
    Write-Output "`$env:OFFICECLI_PLUGIN_DUMP_READER_HWPX = '$builtBinary'"
    exit 0
}

if (-not $NoBuild) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo not found. Install Rust from https://rustup.rs first."
    }

    Write-Host "building release binary..."
    Push-Location $repoRoot
    try {
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $builtBinary -PathType Leaf)) {
    throw "binary not found at $builtBinary"
}

& $builtBinary --info | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "'$binaryName --info' failed; refusing to install"
}

New-Item -ItemType Directory -Force -Path $installDirectory | Out-Null
Copy-Item -LiteralPath $builtBinary -Destination $installPath -Force

& $installPath --info | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "installed plugin failed its manifest check"
}

Write-Host "installed: $installPath"
Write-Host "verify discovery with: officecli plugins list"
