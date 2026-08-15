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
$hwpInstallDirectory = Join-Path $HOME ".officecli\plugins\dump-reader\hwp"
$hwpxInstallDirectory = Join-Path $HOME ".officecli\plugins\dump-reader\hwpx"
$installTargets = @(
    [PSCustomObject]@{ Extension = "hwp"; Directory = $hwpInstallDirectory; Path = (Join-Path $hwpInstallDirectory "plugin.exe") }
    [PSCustomObject]@{ Extension = "hwpx"; Directory = $hwpxInstallDirectory; Path = (Join-Path $hwpxInstallDirectory "plugin.exe") }
)

function Assert-InstallDirectoryNotReparse([string]$Path) {
    if (Test-Path -LiteralPath $Path) {
        $directoryItem = Get-Item -LiteralPath $Path -Force
        if (($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "refusing reparseable install directory: $Path"
        }
    }
}

if ($Uninstall) {
    foreach ($target in $installTargets) {
        Assert-InstallDirectoryNotReparse $target.Directory
    }
    foreach ($target in $installTargets) {
        if (Test-Path -LiteralPath $target.Path) {
            Remove-Item -LiteralPath $target.Path -Force
            Write-Host "removed $($target.Path)"
        } else {
            Write-Host "not installed: $($target.Path)"
        }
        if (Test-Path -LiteralPath $target.Directory -PathType Container) {
            $remaining = @(Get-ChildItem -LiteralPath $target.Directory -Force)
            if ($remaining.Count -eq 0) {
                Remove-Item -LiteralPath $target.Directory -Force
            }
        }
    }
    exit 0
}

if ($PrintEnv) {
    Write-Output "`$env:OFFICECLI_PLUGIN_DUMP_READER_HWPX = '$builtBinary'"
    Write-Output "`$env:OFFICECLI_PLUGIN_DUMP_READER_HWP = '$builtBinary'"
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

foreach ($target in $installTargets) {
    Assert-InstallDirectoryNotReparse $target.Directory
    New-Item -ItemType Directory -Force -Path $target.Directory | Out-Null
}

$sourceHash = (Get-FileHash -LiteralPath $builtBinary -Algorithm SHA256).Hash
$staged = @()
$records = @()
try {
    foreach ($target in $installTargets) {
        if (Test-Path -LiteralPath $target.Path) {
            $existing = Get-Item -LiteralPath $target.Path -Force
            if (($existing.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "refusing reparseable install target: $($target.Path)"
            }
        }

        $stage = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".tmp.exe")
        Copy-Item -LiteralPath $builtBinary -Destination $stage
        $stageHash = (Get-FileHash -LiteralPath $stage -Algorithm SHA256).Hash
        if ($stageHash -ne $sourceHash) {
            throw "staged plugin checksum mismatch for $($target.Extension)"
        }
        & $stage --info | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "staged $($target.Extension) plugin failed its manifest check"
        }
        $staged += [PSCustomObject]@{ Target = $target; Stage = $stage }
    }

    foreach ($item in $staged) {
        $target = $item.Target
        $backup = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".bak.exe")
        $hadExisting = Test-Path -LiteralPath $target.Path -PathType Leaf
        $record = [PSCustomObject]@{
            Target = $target
            Backup = $backup
            HadExisting = $hadExisting
            Committed = $false
        }
        $records += $record

        if ($hadExisting) {
            [IO.File]::Replace($item.Stage, $target.Path, $backup, $true)
        } else {
            [IO.File]::Move($item.Stage, $target.Path)
        }
        $record.Committed = $true
    }

    foreach ($record in $records) {
        & $record.Target.Path --info | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "installed $($record.Target.Extension) plugin failed its manifest check"
        }
    }
} catch {
    [array]::Reverse($records)
    foreach ($record in $records) {
        if ($record.Committed -and (Test-Path -LiteralPath $record.Target.Path)) {
            Remove-Item -LiteralPath $record.Target.Path -Force
        }
        if ($record.HadExisting -and (Test-Path -LiteralPath $record.Backup -PathType Leaf)) {
            [IO.File]::Move($record.Backup, $record.Target.Path)
        }
    }
    throw
} finally {
    foreach ($item in $staged) {
        if (Test-Path -LiteralPath $item.Stage) {
            Remove-Item -LiteralPath $item.Stage -Force
        }
    }
    foreach ($record in $records) {
        if (Test-Path -LiteralPath $record.Backup) {
            Remove-Item -LiteralPath $record.Backup -Force
        }
    }
}

foreach ($target in $installTargets) {
    Write-Host "installed: $($target.Path)"
}
Write-Host "verify discovery with: officecli plugins list"
