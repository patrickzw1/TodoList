param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [Parameter(Mandatory = $true)]
    [string]$Repository,
    [Parameter(Mandatory = $true)]
    [string]$BundleDirectory,
    [Parameter(Mandatory = $true)]
    [string]$NotesPath
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') { throw 'GitHub Release publication is restricted to GitHub Actions.' }
if ([string]::IsNullOrWhiteSpace($env:GH_TOKEN)) { throw 'GH_TOKEN is required to publish a release.' }
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw "Invalid repository: $Repository" }

$projectRoot = Split-Path -Parent $PSScriptRoot
$resolvedBundle = [IO.Path]::GetFullPath((Join-Path $projectRoot $BundleDirectory))
$resolvedNotes = [IO.Path]::GetFullPath((Join-Path $projectRoot $NotesPath))
$version = [string]((Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version)
if ($Tag -ne "v$version") { throw "Release tag $Tag does not match package.json version $version." }
if (-not (Test-Path -LiteralPath $resolvedNotes -PathType Leaf)) { throw "Release notes are missing: $resolvedNotes" }

$installerName = "TodoList_${version}_x64-setup.exe"
$assetNames = @($installerName, "$installerName.sig", 'latest.json', 'SHA256SUMS.txt')
$assetPaths = @($assetNames | ForEach-Object { Join-Path $resolvedBundle $_ })
foreach ($assetPath in $assetPaths) {
    if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) { throw "Release asset is missing: $assetPath" }
}

function Invoke-Gh([string[]]$Arguments) {
    $output = (& gh @Arguments 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -ne 0) { throw "GitHub CLI failed: $output" }
    return $output
}

function Invoke-GhApi([string]$Endpoint) {
    $output = (& gh api $Endpoint 2>&1 | Out-String).Trim()
    if ($LASTEXITCODE -eq 0) { return $output }
    if ($output -match 'HTTP 404') { throw "GitHub API returned HTTP 404 for $Endpoint." }
    throw "GitHub API request failed for ${Endpoint}: $output"
}

function Find-ReleaseByTag {
    for ($page = 1; $page -le 100; $page++) {
        $json = Invoke-GhApi "repos/$Repository/releases?per_page=100&page=$page"
        $releases = @($json | ConvertFrom-Json)
        $matches = @($releases | Where-Object { [string]$_.tag_name -ceq $Tag })
        if ($matches.Count -gt 1) { throw "GitHub returned multiple releases for tag $Tag." }
        if ($matches.Count -eq 1) { return $matches[0] }
        if ($releases.Count -lt 100) { return $null }
    }
    throw "GitHub Release lookup exceeded 100 pages for $Tag."
}

function Get-ReleaseById([int64]$ReleaseId) {
    $json = Invoke-GhApi "repos/$Repository/releases/$ReleaseId"
    $release = $json | ConvertFrom-Json
    if (-not $release -or [int64]$release.id -ne $ReleaseId) { throw "GitHub returned an invalid release for id $ReleaseId." }
    return $release
}

function Get-Sha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        return [BitConverter]::ToString($sha256.ComputeHash($stream)).Replace('-', '')
    } finally {
        $sha256.Dispose()
        $stream.Dispose()
    }
}

function Get-RemoteTagCommit {
    $reference = (Invoke-Gh @('api', "repos/$Repository/git/ref/tags/$Tag")) | ConvertFrom-Json
    $objectType = [string]$reference.object.type
    $objectSha = [string]$reference.object.sha
    for ($depth = 0; $objectType -eq 'tag' -and $depth -lt 8; $depth++) {
        $tagObject = (Invoke-Gh @('api', "repos/$Repository/git/tags/$objectSha")) | ConvertFrom-Json
        $objectType = [string]$tagObject.object.type
        $objectSha = [string]$tagObject.object.sha
    }
    if ($objectType -ne 'commit' -or $objectSha -notmatch '^[0-9a-f]{40}$') {
        throw "Remote release tag did not resolve to a commit: $Tag"
    }
    return $objectSha
}

function Assert-ExpectedAssets($Release, [bool]$AllowSubset) {
    $remoteNames = @($Release.assets | ForEach-Object { [string]$_.name })
    if ($remoteNames.Count -ne (@($remoteNames | Select-Object -Unique)).Count) { throw 'GitHub Release has duplicate asset names.' }
    foreach ($name in $remoteNames) {
        if ($name -notin $assetNames) { throw "GitHub Release contains an unexpected asset: $name" }
    }
    if (-not $AllowSubset) {
        if ($remoteNames.Count -ne $assetNames.Count) { throw 'GitHub Release does not contain exactly four expected assets.' }
        foreach ($name in $assetNames) {
            if ($name -notin $remoteNames) { throw "GitHub Release is missing asset: $name" }
        }
    }
}

Push-Location $projectRoot
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ('todolist-github-release-' + [Guid]::NewGuid().ToString('N'))
try {
    $localTagCommit = (& git rev-parse "refs/tags/$Tag^{commit}").Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($localTagCommit)) { throw "Could not resolve local release tag: $Tag" }
    $headCommit = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $headCommit -ne $localTagCommit) { throw "Local $Tag does not resolve to the checked-out commit." }
    $remoteTagCommit = Get-RemoteTagCommit
    if ($remoteTagCommit -ne $localTagCommit) { throw "Remote $Tag moved after checkout; refusing to publish." }
    $marker = "<!-- todolist-release-workflow tag=$Tag commit=$localTagCommit -->"
    $bodyPath = Join-Path $temporaryRoot 'release-notes.md'
    $downloadDirectory = Join-Path $temporaryRoot 'downloaded-assets'
    New-Item -ItemType Directory -Path $temporaryRoot, $downloadDirectory | Out-Null
    $notes = (Get-Content -LiteralPath $resolvedNotes -Raw -Encoding UTF8).TrimEnd()
    [IO.File]::WriteAllText($bodyPath, "$notes`r`n`r`n$marker`r`n", [Text.UTF8Encoding]::new($false))

    $release = Find-ReleaseByTag
    if ($release) {
        if (-not $release.draft) { throw "A published GitHub Release already exists for $Tag; it will not be overwritten." }
        if ([string]$release.body -notlike "*$marker*") { throw "An unowned draft GitHub Release already exists for $Tag." }
        $releaseId = [int64]$release.id
        Assert-ExpectedAssets $release $true
        Invoke-Gh @('release', 'edit', $Tag, '--repo', $Repository, '--title', "TodoList $Tag", '--notes-file', $bodyPath) | Out-Null
        Invoke-Gh (@('release', 'upload', $Tag) + $assetPaths + @('--repo', $Repository, '--clobber')) | Out-Null
    } else {
        Invoke-Gh (@('release', 'create', $Tag) + $assetPaths + @('--repo', $Repository, '--draft', '--verify-tag', '--title', "TodoList $Tag", '--notes-file', $bodyPath)) | Out-Null
        $release = Find-ReleaseByTag
        if (-not $release) { throw "GitHub Release list did not contain the new draft for $Tag after creation." }
        $releaseId = [int64]$release.id
    }

    $release = Get-ReleaseById $releaseId
    if (-not $release -or -not $release.draft) { throw 'Expected a draft GitHub Release after asset upload.' }
    if ([string]$release.body -notlike "*$marker*") { throw 'Draft ownership marker is missing after asset upload.' }
    Assert-ExpectedAssets $release $false
    foreach ($asset in $release.assets) {
        $localPath = Join-Path $resolvedBundle ([string]$asset.name)
        if ([int64]$asset.size -ne (Get-Item -LiteralPath $localPath).Length) { throw "Uploaded asset size mismatch: $($asset.name)" }
    }

    Invoke-Gh @('release', 'download', $Tag, '--repo', $Repository, '--dir', $downloadDirectory) | Out-Null
    foreach ($name in $assetNames) {
        $localPath = Join-Path $resolvedBundle $name
        $downloadedPath = Join-Path $downloadDirectory $name
        if (-not (Test-Path -LiteralPath $downloadedPath -PathType Leaf)) { throw "Could not download draft asset for verification: $name" }
        $localHash = Get-Sha256 $localPath
        $downloadedHash = Get-Sha256 $downloadedPath
        if ($localHash -ne $downloadedHash) { throw "Uploaded asset checksum mismatch: $name" }
    }

    Invoke-Gh @('release', 'edit', $Tag, '--repo', $Repository, '--draft=false', '--prerelease=false', '--latest') | Out-Null
    $published = Get-ReleaseById $releaseId
    if (-not $published -or $published.draft -or $published.prerelease) { throw 'GitHub Release was not published as a normal release.' }
    Assert-ExpectedAssets $published $false
    Write-Output "Published and verified GitHub Release: $($published.html_url)"
} finally {
    Pop-Location
    if (Test-Path -LiteralPath $temporaryRoot) {
        $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
        $resolvedTemporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
        if ($resolvedTemporaryRoot.StartsWith($tempBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
        }
    }
}
