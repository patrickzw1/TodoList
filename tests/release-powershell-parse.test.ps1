$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$paths = @(
    'scripts\build-release.ps1',
    'scripts\validate-release-ref.ps1',
    'scripts\publish-github-release.ps1'
)

foreach ($relativePath in $paths) {
    $path = Join-Path $projectRoot $relativePath
    $tokens = $null
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile($path, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) {
        $messages = @($errors | ForEach-Object { $_.Message }) -join '; '
        throw "PowerShell parser rejected ${relativePath}: $messages"
    }
}

$previousGitHubActions = $env:GITHUB_ACTIONS
$previousPrivateKey = $env:TAURI_SIGNING_PRIVATE_KEY
$previousPrivateKeyPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    $env:GITHUB_ACTIONS = 'true'
    $env:TAURI_SIGNING_PRIVATE_KEY = $null
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $null
    try {
        & (Join-Path $projectRoot 'scripts\build-release.ps1')
        throw 'Signing build unexpectedly continued without GitHub Actions Secrets.'
    } catch {
        if ($_.Exception.Message -notmatch 'GitHub Actions release signing Secrets are missing') { throw }
    }

    try {
        & (Join-Path $projectRoot 'scripts\validate-release-ref.ps1') -Tag 'not-a-release-tag'
        throw 'Invalid release tag unexpectedly passed validation.'
    } catch {
        if ($_.Exception.Message -notmatch 'stable vMAJOR.MINOR.PATCH') { throw }
    }
} finally {
    $env:GITHUB_ACTIONS = $previousGitHubActions
    $env:TAURI_SIGNING_PRIVATE_KEY = $previousPrivateKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPrivateKeyPassword
}

Write-Output 'Release PowerShell scripts parsed and fail-closed checks passed.'
