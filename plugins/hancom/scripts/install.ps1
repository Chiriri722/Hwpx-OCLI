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
$hwpBinaryName = "officecli-hancom-hwp.exe"
$hwpxBinaryName = "officecli-hancom-hwpx.exe"
$hwpBuiltBinary = Join-Path $repoRoot "target\release\$hwpBinaryName"
$hwpxBuiltBinary = Join-Path $repoRoot "target\release\$hwpxBinaryName"

if (-not [IO.Path]::IsPathFullyQualified($HOME)) {
    throw "HOME must be an absolute path"
}
$homeDirectory = [IO.Path]::GetFullPath($HOME)
$officeCliDirectory = Join-Path $homeDirectory ".officecli"
$pluginsDirectory = Join-Path $officeCliDirectory "plugins"
$dumpReaderRoot = Join-Path $pluginsDirectory "dump-reader"
$formatHandlerRoot = Join-Path $pluginsDirectory "format-handler"
$pluginRoots = @($dumpReaderRoot, $formatHandlerRoot)

$hwpInstallDirectory = Join-Path $dumpReaderRoot "hwp"
$hmlInstallDirectory = Join-Path $dumpReaderRoot "hml"
$hwpxInstallDirectory = Join-Path $formatHandlerRoot "hwpx"
$owpmlInstallDirectory = Join-Path $formatHandlerRoot "owpml"
$legacyHwpxInstallDirectory = Join-Path $dumpReaderRoot "hwpx"
$legacyOwpmlInstallDirectory = Join-Path $dumpReaderRoot "owpml"

$installTargets = @(
    [PSCustomObject]@{
        Kind = "dump-reader"
        Extension = "hwp"
        Label = "dump-reader/hwp"
        EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_HWP"
        PluginRoot = $dumpReaderRoot
        Directory = $hwpInstallDirectory
        Path = (Join-Path $hwpInstallDirectory "plugin.exe")
        PluginName = "officecli-hancom-hwp"
        BinaryName = $hwpBinaryName
        BuiltBinary = $hwpBuiltBinary
        Install = $true
    }
    [PSCustomObject]@{
        Kind = "dump-reader"
        Extension = "hml"
        Label = "dump-reader/hml"
        EnvironmentVariable = "OFFICECLI_PLUGIN_DUMP_READER_HML"
        PluginRoot = $dumpReaderRoot
        Directory = $hmlInstallDirectory
        Path = (Join-Path $hmlInstallDirectory "plugin.exe")
        PluginName = "officecli-hancom-hwp"
        BinaryName = $hwpBinaryName
        BuiltBinary = $hwpBuiltBinary
        Install = $true
    }
    [PSCustomObject]@{
        Kind = "format-handler"
        Extension = "hwpx"
        Label = "format-handler/hwpx"
        EnvironmentVariable = "OFFICECLI_PLUGIN_FORMAT_HANDLER_HWPX"
        PluginRoot = $formatHandlerRoot
        Directory = $hwpxInstallDirectory
        Path = (Join-Path $hwpxInstallDirectory "plugin.exe")
        PluginName = "officecli-hancom-hwpx"
        BinaryName = $hwpxBinaryName
        BuiltBinary = $hwpxBuiltBinary
        Install = $true
    }
    [PSCustomObject]@{
        Kind = "format-handler"
        Extension = "owpml"
        Label = "format-handler/owpml"
        EnvironmentVariable = "OFFICECLI_PLUGIN_FORMAT_HANDLER_OWPML"
        PluginRoot = $formatHandlerRoot
        Directory = $owpmlInstallDirectory
        Path = (Join-Path $owpmlInstallDirectory "plugin.exe")
        PluginName = "officecli-hancom-hwpx"
        BinaryName = $hwpxBinaryName
        BuiltBinary = $hwpxBuiltBinary
        Install = $true
    }
)

# These paths were populated by releases before HWPX/OWPML became editable
# format handlers. They stay in the transaction so promotion cannot leave a
# dump-reader that shadows the new handler.
$obsoleteTargets = @(
    [PSCustomObject]@{
        Kind = "dump-reader"
        Extension = "hwpx"
        Label = "dump-reader/hwpx"
        EnvironmentVariable = $null
        PluginRoot = $dumpReaderRoot
        Directory = $legacyHwpxInstallDirectory
        Path = (Join-Path $legacyHwpxInstallDirectory "plugin.exe")
        PluginName = $null
        BinaryName = $null
        BuiltBinary = $null
        Install = $false
    }
    [PSCustomObject]@{
        Kind = "dump-reader"
        Extension = "owpml"
        Label = "dump-reader/owpml"
        EnvironmentVariable = $null
        PluginRoot = $dumpReaderRoot
        Directory = $legacyOwpmlInstallDirectory
        Path = (Join-Path $legacyOwpmlInstallDirectory "plugin.exe")
        PluginName = $null
        BinaryName = $null
        BuiltBinary = $null
        Install = $false
    }
)
$managedTargets = @($installTargets) + @($obsoleteTargets)

function Get-PluginRootForPath([string]$Path) {
    $fullPath = [IO.Path]::GetFullPath($Path)
    foreach ($root in $pluginRoots) {
        $rootWithSeparator = "$root$([IO.Path]::DirectorySeparatorChar)"
        if (
            [string]::Equals($fullPath, $root, [StringComparison]::OrdinalIgnoreCase) -or
            $fullPath.StartsWith($rootWithSeparator, [StringComparison]::OrdinalIgnoreCase)
        ) {
            return $root
        }
    }
    throw "install path is outside the managed plugin roots: $Path"
}

function Assert-InstallDirectoryNotReparse([string]$Path) {
    $pluginRoot = Get-PluginRootForPath $Path
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

function Assert-PluginManifest([string]$Path, [object]$Target, [string]$Context) {
    $manifestOutput = & $Path --info
    $manifestExitCode = $LASTEXITCODE
    if ($manifestExitCode -ne 0) {
        throw "$Context $($Target.Label) plugin failed its manifest check"
    }
    try {
        $manifest = (($manifestOutput -join [Environment]::NewLine).Trim() | ConvertFrom-Json -ErrorAction Stop)
    } catch {
        throw "$Context $($Target.Label) plugin emitted an invalid JSON manifest: $($_.Exception.Message)"
    }
    if ($manifest.name -ne $Target.PluginName) {
        throw "$Context $($Target.Label) plugin name is '$($manifest.name)', expected '$($Target.PluginName)'"
    }
    if ($manifest.protocol -ne 1) {
        throw "$Context $($Target.Label) plugin protocol is '$($manifest.protocol)', expected '1'"
    }
    if (@($manifest.kinds) -notcontains $Target.Kind) {
        throw "$Context $($Target.Label) manifest does not declare kind '$($Target.Kind)'"
    }
    $expectedExtension = ".$($Target.Extension)"
    if (@($manifest.extensions) -notcontains $expectedExtension) {
        throw "$Context $($Target.Label) manifest does not declare extension '$expectedExtension'"
    }
    if (
        $Target.Kind -eq "dump-reader" -and
        $manifest.target -notin @("docx", "xlsx", "pptx")
    ) {
        throw "$Context $($Target.Label) dump-reader has an invalid target '$($manifest.target)'"
    }
}

function Remove-EmptyInstallDirectory([object]$Target) {
    if (Test-Path -LiteralPath $Target.Directory -PathType Container) {
        Assert-InstallDirectoryNotReparse $Target.Directory
        $remaining = @(Get-ChildItem -LiteralPath $Target.Directory -Force)
        if ($remaining.Count -eq 0) {
            Remove-Item -LiteralPath $Target.Directory -Force
        }
    }
}

if ($Uninstall) {
    foreach ($target in $managedTargets) {
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path
    }
    foreach ($target in $managedTargets) {
        if (Test-Path -LiteralPath $target.Path) {
            Remove-Item -LiteralPath $target.Path -Force
            Write-Host "removed $($target.Path)"
        } else {
            Write-Host "not installed: $($target.Path)"
        }
        Remove-EmptyInstallDirectory $target
    }
    exit 0
}

if ($PrintEnv) {
    foreach ($target in $installTargets) {
        Write-Output ('$env:{0} = ''{1}''' -f $target.EnvironmentVariable, $target.BuiltBinary)
    }
    exit 0
}

if (-not $NoBuild) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "cargo not found. Install Rust from https://rustup.rs first."
    }

    Write-Host "building release binaries..."
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

$binarySources = @(
    [PSCustomObject]@{ BinaryName = $hwpBinaryName; Path = $hwpBuiltBinary }
    [PSCustomObject]@{ BinaryName = $hwpxBinaryName; Path = $hwpxBuiltBinary }
)
foreach ($binary in $binarySources) {
    if (-not (Test-Path -LiteralPath $binary.Path -PathType Leaf)) {
        throw "binary not found at $($binary.Path)"
    }
}

foreach ($target in $managedTargets) {
    Assert-InstallDirectoryNotReparse $target.Directory
    Assert-InstallTargetSafe $target.Path
}
foreach ($target in $installTargets) {
    [void][IO.Directory]::CreateDirectory($target.Directory)
}
foreach ($target in $managedTargets) {
    Assert-InstallDirectoryNotReparse $target.Directory
    Assert-InstallTargetSafe $target.Path
}

$staged = @()
$records = @()
try {
    foreach ($target in $installTargets) {
        $sourceHash = (Get-FileHash -LiteralPath $target.BuiltBinary -Algorithm SHA256).Hash
        $stage = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".tmp.exe")
        $staged += [PSCustomObject]@{
            Target = $target
            Stage = $stage
            SourceHash = $sourceHash
        }
        Copy-Item -LiteralPath $target.BuiltBinary -Destination $stage
        $stageHash = (Get-FileHash -LiteralPath $stage -Algorithm SHA256).Hash
        if ($stageHash -ne $sourceHash) {
            throw "staged plugin checksum mismatch for $($target.Label)"
        }
        Assert-PluginManifest $stage $target "staged"
    }

    foreach ($item in $staged) {
        $target = $item.Target
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path
        $backup = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".bak.exe")
        $hadExisting = Test-Path -LiteralPath $target.Path -PathType Leaf
        $record = [PSCustomObject]@{
            Action = "Install"
            Target = $target
            Backup = $backup
            HadExisting = $hadExisting
            ExpectedHash = $item.SourceHash
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
            throw "failed to commit $($target.Label) plugin: $($_.Exception.Message)"
        }
        $record.Committed = $true
    }

    foreach ($target in $obsoleteTargets) {
        Assert-InstallDirectoryNotReparse $target.Directory
        Assert-InstallTargetSafe $target.Path
        $backup = Join-Path $target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".bak.exe")
        $hadExisting = Test-Path -LiteralPath $target.Path -PathType Leaf
        $record = [PSCustomObject]@{
            Action = "Retire"
            Target = $target
            Backup = $backup
            HadExisting = $hadExisting
            ExpectedHash = $null
            Committed = $false
        }
        $records += $record

        try {
            if ($hadExisting) {
                [IO.File]::Move($target.Path, $backup)
            }
        } catch {
            throw "failed to retire $($target.Label) plugin: $($_.Exception.Message)"
        }
        $record.Committed = $true
    }

    foreach ($target in $installTargets) {
        Assert-PluginManifest $target.Path $target "installed"
    }
    foreach ($target in $obsoleteTargets) {
        if (Test-Path -LiteralPath $target.Path) {
            throw "obsolete $($target.Label) plugin still shadows the promoted format handler"
        }
    }

    foreach ($record in $records) {
        if (Test-Path -LiteralPath $record.Backup) {
            try {
                Remove-Item -LiteralPath $record.Backup -Force -ErrorAction Stop
            } catch {
                Write-Warning "installed plugins are valid, but backup cleanup failed and was preserved at $($record.Backup): $($_.Exception.Message)"
            }
        }
    }
    foreach ($target in $obsoleteTargets) {
        Remove-EmptyInstallDirectory $target
    }
} catch {
    $installError = $_
    $rollbackErrors = @()
    [array]::Reverse($records)
    foreach ($record in $records) {
        try {
            Assert-InstallDirectoryNotReparse $record.Target.Directory
            Assert-InstallTargetSafe $record.Target.Path
            if ($record.Committed) {
                if ($record.HadExisting) {
                    if (-not (Test-Path -LiteralPath $record.Backup -PathType Leaf)) {
                        throw "recovery backup is missing: $($record.Backup)"
                    }
                    if ($record.Action -eq "Retire") {
                        if (Test-Path -LiteralPath $record.Target.Path) {
                            throw "retired target reappeared; recovery backup preserved at $($record.Backup)"
                        }
                        [IO.File]::Move($record.Backup, $record.Target.Path)
                    } elseif (Test-Path -LiteralPath $record.Target.Path -PathType Leaf) {
                        $currentHash = (Get-FileHash -LiteralPath $record.Target.Path -Algorithm SHA256).Hash
                        if ($currentHash -ne $record.ExpectedHash) {
                            throw "installed target changed; recovery backup preserved at $($record.Backup)"
                        }
                        $restoreDisplaced = Join-Path $record.Target.Directory (".plugin." + [Guid]::NewGuid().ToString("N") + ".rollback.exe")
                        $staged += [PSCustomObject]@{ Target = $record.Target; Stage = $restoreDisplaced }
                        [IO.File]::Replace($record.Backup, $record.Target.Path, $restoreDisplaced, $true)
                    } else {
                        [IO.File]::Move($record.Backup, $record.Target.Path)
                    }
                } elseif (
                    $record.Action -eq "Install" -and
                    (Test-Path -LiteralPath $record.Target.Path -PathType Leaf)
                ) {
                    $currentHash = (Get-FileHash -LiteralPath $record.Target.Path -Algorithm SHA256).Hash
                    if ($currentHash -ne $record.ExpectedHash) {
                        throw "newly installed target changed; refusing rollback removal"
                    }
                    Remove-Item -LiteralPath $record.Target.Path -Force
                }
            } elseif (Test-Path -LiteralPath $record.Backup -PathType Leaf) {
                throw "commit state is uncertain; recovery backup preserved at $($record.Backup)"
            }
        } catch {
            $rollbackErrors += "[$($record.Target.Label)] $($_.Exception.Message)"
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
foreach ($target in $obsoleteTargets) {
    Write-Host "retired: $($target.Path)"
}
Write-Host "verify discovery with: officecli plugins list"
