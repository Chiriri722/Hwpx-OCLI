# Read-only Sentry queries through the installed Codex Sentry plugin.
[CmdletBinding()]
param(
    [string]$TokenFile,
    [string]$SentryApi = $env:SENTRY_API,
    [string]$Python = 'python',
    [ValidateRange(1, 50)][int]$Limit = 20,
    [ValidateSet('1h', '24h', '7d', '14d')][string]$TimeRange = '24h',
    [string]$Environment = 'prod',
    [string]$Query = 'is:unresolved'
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path $PSScriptRoot -Parent
$config = Get-Content -LiteralPath (Join-Path $repoRoot '.specify/development-integrations.json') -Raw | ConvertFrom-Json

if (-not $SentryApi) {
    $codexRoot = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
    $pluginRoot = Join-Path $codexRoot 'plugins/cache/openai-curated-remote/sentry'
    $candidates = @(Get-ChildItem -LiteralPath $pluginRoot -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
        Sort-Object { [version]$_.Name } -Descending)
    foreach ($candidate in $candidates) {
        $path = Join-Path $candidate.FullName 'skills/sentry/scripts/sentry_api.py'
        if (Test-Path -LiteralPath $path -PathType Leaf) { $SentryApi = $path; break }
    }
}
if (-not $SentryApi -or -not (Test-Path -LiteralPath $SentryApi -PathType Leaf)) {
    throw 'Install the Sentry plugin or pass -SentryApi / set SENTRY_API to its scripts/sentry_api.py.'
}

$previousToken = $env:SENTRY_AUTH_TOKEN
try {
    if ($TokenFile) {
        $raw = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $TokenFile).Path).Trim()
        $tokens = @([regex]::Matches($raw, '(?:sntrys|sntryu)_[A-Za-z0-9_-]+') |
            ForEach-Object Value | Select-Object -Unique)
        $userTokens = @($tokens | Where-Object { $_.StartsWith('sntryu_') })
        if ($userTokens.Count -eq 1) { $env:SENTRY_AUTH_TOKEN = $userTokens[0] }
        elseif ($tokens.Count -eq 1) { $env:SENTRY_AUTH_TOKEN = $tokens[0] }
        elseif ($raw -match '^[A-Za-z0-9_.-]+$') { $env:SENTRY_AUTH_TOKEN = $raw }
        else { throw 'Token file must contain one unambiguous token. Contents were not printed.' }
    }
    if (-not $env:SENTRY_AUTH_TOKEN) {
        throw 'Set SENTRY_AUTH_TOKEN locally or pass -TokenFile. Never put tokens in tracked configuration.'
    }
    # Pin the official endpoint; keep the plugin default PII redaction enabled.
    & $Python $SentryApi --base-url https://sentry.io --org $config.sentry.organization --project $config.sentry.project `
        list-issues --environment $Environment --time-range $TimeRange --query $Query --limit $Limit
    if ($LASTEXITCODE -ne 0) { throw "Sentry read-only query failed (exit $LASTEXITCODE)." }
}
finally {
    $env:SENTRY_AUTH_TOKEN = $previousToken
    $raw = $null
    $tokens = $null
    $userTokens = $null
}
