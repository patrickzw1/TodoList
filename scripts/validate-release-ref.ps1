param(
    [Parameter(Mandatory = $true)]
    [string]$Tag
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if ($Tag -notmatch '^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
        throw "Release tag must be a stable vMAJOR.MINOR.PATCH tag: $Tag"
    }
    $version = [string]((Get-Content -LiteralPath 'package.json' -Raw -Encoding UTF8 | ConvertFrom-Json).version)
    if ($Tag -ne "v$version") {
        throw "Release tag $Tag does not match package.json version $version."
    }

    & git show-ref --verify --quiet "refs/tags/$Tag"
    if ($LASTEXITCODE -ne 0) { throw "Release tag does not exist in this checkout: $Tag" }
    $tagCommit = (& git rev-parse "refs/tags/$Tag^{commit}").Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($tagCommit)) { throw "Could not resolve release tag: $Tag" }
    $headCommit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $headCommit -ne $tagCommit) {
        throw "Checked-out commit $headCommit does not match $Tag commit $tagCommit."
    }
    & git rev-parse --verify --quiet 'refs/remotes/origin/main^{commit}' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'origin/main is unavailable in the release checkout.' }
    & git merge-base --is-ancestor $tagCommit 'refs/remotes/origin/main'
    if ($LASTEXITCODE -ne 0) { throw "Release tag $Tag is not contained in origin/main." }
    & git diff --quiet --no-ext-diff
    if ($LASTEXITCODE -ne 0) { throw 'Release checkout has unstaged changes.' }
    & git diff --cached --quiet --no-ext-diff
    if ($LASTEXITCODE -ne 0) { throw 'Release checkout has staged changes.' }

    Write-Output "Validated release ref $Tag at $tagCommit for version $version."
} finally {
    Pop-Location
}
