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
$binaryName = "officecli-hancom-hwp.exe"
$builtBinary = Join-Path $repoRoot "target\release\$binaryName"
if (-not [IO.Path]::IsPathFullyQualified($HOME)) {
    throw "HOME must be an absolute path"
}
$homeDirectory = [IO.Path]::GetFullPath($HOME)
$officeCliDirectory = Join-Path $homeDirectory ".officecli"
$pluginsDirectory = Join-Path $officeCliDirectory "plugins"
$pluginRoot = Join-Path $pluginsDirectory "dump-reader"
$hwpInstallDirectory = Join-Path $homeDirectory ".officecli\plugins\dump-reader\hwp"
$hwpxInstallDirectory = Join-Path $homeDirectory ".officecli\plugins\dump-reader\hwpx"
$owpmlInstallDirectory = Join-Path $homeDirectory ".officecli\plugins\dump-reader\owpml"
$hmlInstallDirectory = Join-Path $homeDirectory ".officecli\plugins\dump-reader\hml"
$installTargets = @(
    [PSCustomObject]@{ Extension = "hwp"; EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_HWP"; Directory = $hwpInstallDirectory; Path = (Join-Path $hwpInstallDirectory "plugin.exe") }
    [PSCustomObject]@{ Extension = "hwpx"; EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_HWPX"; Directory = $hwpxInstallDirectory; Path = (Join-Path $hwpxInstallDirectory "plugin.exe") }
    [PSCustomObject]@{ Extension = "owpml"; EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_OWPML"; Directory = $owpmlInstallDirectory; Path = (Join-Path $owpmlInstallDirectory "plugin.exe") }
    [PSCustomObject]@{ Extension = "hml"; EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_HML"; Directory = $hmlInstallDirectory; Path = (Join-Path $hmlInstallDirectory "plugin.exe") }
)

function Assert-InstallDirectoryNotReparse([string]$Path) {
    foreach ($component in @($officeCliDirectory, $pluginsDirectory, $pluginRoot, $Path)) {
        try {
            $directoryItem = Get-Item -LiteralPath $component -Force -ErrorAction Stop
        } catch [System.Management.Automation.ItemNotFoundException] {
            continue
        }
        if (($directoryItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "refusing reparseable install directory: $component"
        }
        if (-not $directoryItem.PSIsContainer) {
            throw "refusing non-directory install path component: $component"
        }
    }
}

function Assert-InstallTargetSafe([string]$Path) {
    try {
        $targetItem = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    } catch [System.Management.Automation.ItemNotFoundException] {
        return
    }
    if (($targetItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "refusing reparseable install target: $Path"
    }
    if ($targetItem.PSIsContainer) {
        throw "refusing non-file install target: $Path"
    }
}

if ($Uninstall) {
    foreach ($target in $installTargets) {
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path
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
    foreach ($target in $installTargets) {
        Write-Output "`$env:$($target.EnvironmentVariable) = '$builtBinary'"
    }
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
}
foreach ($target in $installTargets) {
    [void][IO.Directory]::CreateDirectory($target.Directory)
}
foreach ($target in $installTargets) {
    Assert-InstallDirectoryNotReparse $target.Directory
}

$sourceHash = (Get-FileHash -LiteralPath $builtBinary -Algorithm SHA256).Hash
$staged = @()
$records = @()
try {
    foreach ($target in $installTargets) {
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path

        $stage = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".tmp.exe")
        $staged += [PSCustomObject]@{ Target = $target; Stage = $stage }
        Copy-Item -LiteralPath $builtBinary -Destination $stage
        $stageHash = (Get-FileHash -LiteralPath $stage -Algorithm SHA256).Hash
        if ($stageHash -ne $sourceHash) {
            throw "staged plugin checksum mismatch for $($target.Extension)"
        }
        & $stage --info | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "staged $($target.Extension) plugin failed its manifest check"
        }
    }

    foreach ($item in $staged) {
        $target = $item.Target
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path
        $backup = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".bak.exe")
        $hadExisting = Test-Path -LiteralPath $target.Path -PathType Leaf
        $record = [PSCustomObject]@{
            Target = $target
            Backup = $backup
            HadExisting = $hadExisting
            Committed = $false
        }
        $records += $record

        try {
            if ($hadExisting) {
                [IO.File]::Replace($item.Stage, $target.Path, $backup, $true)
            } else {
                [IO.File]::Move($item.Stage, $target.Path)
            }
        } catch {
            throw "failed to commit $($target.Extension) plugin: $($_.Exception.Message)"
        }
        $record.Committed = $true
    }

    foreach ($record in $records) {
        & $record.Target.Path --info | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "installed $($record.Target.Extension) plugin failed its manifest check"
        }
    }
    foreach ($record in $records) {
        if (Test-Path -LiteralPath $record.Backup) {
            try {
                Remove-Item -LiteralPath $record.Backup -Force -ErrorAction Stop
            } catch {
                Write-Warning "installed plugin is valid, but backup cleanup failed and was preserved at $($record.Backup): $($_.Exception.Message)"
            }
        }
    }
} catch {
    $installError = $_
    $rollbackErrors = @()
    [array]::Reverse($records)
    foreach ($record in $records) {
        try {
            Assert-InstallDirectoryNotReparse $record.Target.Directory
            if ($record.Committed) {
                if ($record.HadExisting) {
                    if (-not (Test-Path -LiteralPath $record.Backup -PathType Leaf)) {
                        throw "recovery backup is missing: $($record.Backup)"
                    }
                    if (Test-Path -LiteralPath $record.Target.Path -PathType Leaf) {
                        $restoreDisplaced = Join-Path $record.Target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".rollback.exe")
                        $staged += [PSCustomObject]@{ Target = $record.Target; Stage = $restoreDisplaced }
                        [IO.File]::Replace($record.Backup, $record.Target.Path, $restoreDisplaced, $true)
                    } else {
                        [IO.File]::Move($record.Backup, $record.Target.Path)
                    }
                } elseif (Test-Path -LiteralPath $record.Target.Path) {
                    Remove-Item -LiteralPath $record.Target.Path -Force
                }
            } elseif (Test-Path -LiteralPath $record.Backup -PathType Leaf) {
                throw "commit state is uncertain; recovery backup preserved at $($record.Backup)"
            }
        } catch {
            $rollbackErrors += "[$($record.Target.Extension)] $($_.Exception.Message)"
        }
    }
    if ($rollbackErrors.Count -gt 0) {
        throw "installation failed: $($installError.Exception.Message); rollback incomplete and recovery backups were preserved: $($rollbackErrors -join '; ')"
    }
    throw $installError
} finally {
    foreach ($item in $staged) {
        if (Test-Path -LiteralPath $item.Stage) {
            try {
                Remove-Item -LiteralPath $item.Stage -Force -ErrorAction Stop
            } catch {
                Write-Warning "temporary cleanup failed at $($item.Stage): $($_.Exception.Message)"
            }
        }
    }
}

foreach ($target in $installTargets) {
    Write-Host "installed: $($target.Path)"
}
Write-Host "verify discovery with: officecli plugins list"
