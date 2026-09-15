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

function Test-RetryableRead([int]$StatusCode) {
    return $StatusCode -in @(0, 404, 408, 429, 500, 502, 503, 504)
}

function Invoke-GhApi([string]$Endpoint, [string]$Method = 'GET', [string]$InputPath = '', [string]$ContentType = 'application/json') {
    $arguments = @('api', $Endpoint, '--method', $Method, '-H', 'Accept: application/vnd.github+json', '-H', 'X-GitHub-Api-Version: 2022-11-28')
    if ($InputPath) { $arguments += @('--input', $InputPath, '-H', "Content-Type: $ContentType") }
    for ($attempt = 1; $attempt -le 4; $attempt++) {
        $previousPreference = $ErrorActionPreference
        try {
            # Windows PowerShell otherwise throws on native stderr before LASTEXITCODE is examined.
            $ErrorActionPreference = 'Continue'
            $output = (& gh @arguments 2>&1 | Out-String).Trim()
        } finally {
            $ErrorActionPreference = $previousPreference
        }
        if ($LASTEXITCODE -eq 0) { return $output }
        $statusCode = 0
        if ($output -match 'HTTP[ /](\d{3})') { $statusCode = [int]$Matches[1] }
        # Only reads are retried: a failed POST may already have created a draft or asset.
        if ($Method -ne 'GET' -or $attempt -eq 4 -or -not (Test-RetryableRead $statusCode)) {
            throw "GitHub API $Method failed for ${Endpoint} after $attempt attempt(s): $output"
        }
        Write-Warning "GitHub API GET temporarily failed for $Endpoint (HTTP $statusCode); retrying."
        Start-Sleep -Seconds ([int][Math]::Pow(2, $attempt - 1))
    }
}

function Write-JsonInput([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 5 -Compress), [Text.UTF8Encoding]::new($false))
}

function Save-ReleaseAsset([int64]$AssetId, [string]$Path) {
    $endpoint = "https://api.github.com/repos/$Repository/releases/assets/$AssetId"
    $headers = @{ Accept = 'application/octet-stream'; Authorization = "Bearer $env:GH_TOKEN"; 'X-GitHub-Api-Version' = '2022-11-28' }
    for ($attempt = 1; $attempt -le 4; $attempt++) {
        try {
            # Out-File/PowerShell redirection can corrupt binary bytes; let the HTTP client save them.
            Invoke-WebRequest -Uri $endpoint -Headers $headers -OutFile $Path -UseBasicParsing | Out-Null
            return
        } catch {
            $statusCode = 0
            if ($_.Exception.Response) { $statusCode = [int]$_.Exception.Response.StatusCode }
            if ($attempt -eq 4 -or -not (Test-RetryableRead $statusCode)) {
                throw "GitHub asset download failed for $endpoint (HTTP $statusCode) after $attempt attempt(s)."
            }
            Write-Warning "GitHub asset GET temporarily failed (HTTP $statusCode); retrying asset $AssetId."
            Start-Sleep -Seconds ([int][Math]::Pow(2, $attempt - 1))
        }
    }
}

function Find-ReleaseByTag {
    $matchedRelease = $null
    for ($page = 1; $page -le 100; $page++) {
        $json = Invoke-GhApi "repos/$Repository/releases?per_page=100&page=$page"
        # Parentheses enumerate the parsed array on both Windows PowerShell 5.1 and PowerShell 7.
        $releases = @(($json | ConvertFrom-Json))
        $matches = @($releases | Where-Object { [string]$_.tag_name -ceq $Tag })
        if ($matches.Count -gt 1 -or ($matchedRelease -and $matches.Count -gt 0)) { throw "GitHub returned multiple releases for tag $Tag." }
        if ($matches.Count -eq 1) { $matchedRelease = $matches[0] }
        if ($releases.Count -lt 100) { return $matchedRelease }
    }
    throw "GitHub Release lookup exceeded 100 pages for $Tag."
}

function Get-ReleaseById([int64]$ReleaseId) {
    $json = Invoke-GhApi "repos/$Repository/releases/$ReleaseId"
    $release = $json | ConvertFrom-Json
    if (-not $release -or [int64]$release.id -ne $ReleaseId -or [string]$release.tag_name -cne $Tag) { throw "GitHub returned an invalid release for id $ReleaseId." }
    return $release
}

function Assert-OwnedDraft($Release) {
    if ([int64]$Release.id -le 0 -or [string]$Release.tag_name -cne $Tag) { throw 'GitHub returned an invalid draft release identity.' }
    if (-not $Release.draft) { throw "A published GitHub Release already exists for $Tag; it will not be overwritten." }
    $markers = [regex]::Matches([string]$Release.body, '<!--\s*todolist-release-workflow\b.*?-->', [Text.RegularExpressions.RegexOptions]::Singleline)
    if ($markers.Count -ne 1 -or $markers[0].Value -cne $marker) { throw "An unowned draft GitHub Release already exists for $Tag." }
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
    $reference = (Invoke-GhApi "repos/$Repository/git/ref/tags/$Tag") | ConvertFrom-Json
    $objectType = [string]$reference.object.type
    $objectSha = [string]$reference.object.sha
    for ($depth = 0; $objectType -eq 'tag' -and $depth -lt 8; $depth++) {
        $tagObject = (Invoke-GhApi "repos/$Repository/git/tags/$objectSha") | ConvertFrom-Json
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
        if ($name -cnotin $assetNames) { throw "GitHub Release contains an unexpected asset: $name" }
    }
    $assetIds = @($Release.assets | ForEach-Object { [int64]$_.id })
    if (@($assetIds | Where-Object { $_ -le 0 }).Count -gt 0 -or $assetIds.Count -ne @($assetIds | Select-Object -Unique).Count) {
        throw 'GitHub Release has invalid or duplicate asset IDs.'
    }
    if (-not $AllowSubset) {
        if ($remoteNames.Count -ne $assetNames.Count) { throw 'GitHub Release does not contain exactly four expected assets.' }
        foreach ($name in $assetNames) {
            if ($name -cnotin $remoteNames) { throw "GitHub Release is missing asset: $name" }
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
    $requestPath = Join-Path $temporaryRoot 'release-request.json'
    $downloadDirectory = Join-Path $temporaryRoot 'downloaded-assets'
    New-Item -ItemType Directory -Path $temporaryRoot, $downloadDirectory | Out-Null
    $notes = (Get-Content -LiteralPath $resolvedNotes -Raw -Encoding UTF8).TrimEnd()
    $body = "$notes`r`n`r`n$marker`r`n"

    $release = Find-ReleaseByTag
    if ($release) {
        Assert-OwnedDraft $release
        $releaseId = [int64]$release.id
        $release = Get-ReleaseById $releaseId
        Assert-OwnedDraft $release
        Assert-ExpectedAssets $release $true
        Write-JsonInput $requestPath @{ name = "TodoList $Tag"; body = $body; draft = $true; prerelease = $false }
        $release = (Invoke-GhApi "repos/$Repository/releases/$releaseId" 'PATCH' $requestPath) | ConvertFrom-Json
        if ([int64]$release.id -ne $releaseId) { throw 'GitHub returned a different release after metadata update.' }
    } else {
        Write-JsonInput $requestPath @{ tag_name = $Tag; target_commitish = $localTagCommit; name = "TodoList $Tag"; body = $body; draft = $true; prerelease = $false }
        # POST returns the created release ID. Do not rediscover it through a possibly stale list.
        $release = (Invoke-GhApi "repos/$Repository/releases" 'POST' $requestPath) | ConvertFrom-Json
        $releaseId = [int64]$release.id
    }

    Assert-OwnedDraft $release
    Assert-ExpectedAssets $release $true
    $uploadUrl = ([string]$release.upload_url) -replace '\{.*\}$', ''
    if ($uploadUrl -ine "https://uploads.github.com/repos/$Repository/releases/$releaseId/assets") { throw 'GitHub returned an invalid release upload URL.' }
    foreach ($name in $assetNames) {
        $release = Get-ReleaseById $releaseId
        Assert-OwnedDraft $release
        Assert-ExpectedAssets $release $true
        $existingAsset = @($release.assets | Where-Object { [string]$_.name -ceq $name })
        if ($existingAsset.Count -eq 1) {
            Invoke-GhApi "repos/$Repository/releases/assets/$($existingAsset[0].id)" 'DELETE' | Out-Null
        }
        $localPath = Join-Path $resolvedBundle $name
        $asset = (Invoke-GhApi ($uploadUrl + '?name=' + [Uri]::EscapeDataString($name)) 'POST' $localPath 'application/octet-stream') | ConvertFrom-Json
        if ([int64]$asset.id -le 0 -or [string]$asset.name -cne $name -or [int64]$asset.size -ne (Get-Item -LiteralPath $localPath).Length) {
            throw "GitHub returned an invalid uploaded asset: $name"
        }
    }

    $release = Get-ReleaseById $releaseId
    Assert-OwnedDraft $release
    Assert-ExpectedAssets $release $false
    foreach ($asset in $release.assets) {
        $localPath = Join-Path $resolvedBundle ([string]$asset.name)
        if ([int64]$asset.size -ne (Get-Item -LiteralPath $localPath).Length) { throw "Uploaded asset size mismatch: $($asset.name)" }
        if ([string]$asset.state -cne 'uploaded') { throw "GitHub Release asset is not fully uploaded: $($asset.name)" }
    }
    $verifiedAssets = @($release.assets)

    foreach ($asset in $release.assets) {
        Save-ReleaseAsset ([int64]$asset.id) (Join-Path $downloadDirectory ([string]$asset.name))
    }
    foreach ($name in $assetNames) {
        $localPath = Join-Path $resolvedBundle $name
        $downloadedPath = Join-Path $downloadDirectory $name
        if (-not (Test-Path -LiteralPath $downloadedPath -PathType Leaf)) { throw "Could not download draft asset for verification: $name" }
        $localHash = Get-Sha256 $localPath
        $downloadedHash = Get-Sha256 $downloadedPath
        if ($localHash -ne $downloadedHash) { throw "Uploaded asset checksum mismatch: $name" }
    }

    $release = Get-ReleaseById $releaseId
    Assert-OwnedDraft $release
    Assert-ExpectedAssets $release $false
    foreach ($asset in $release.assets) {
        $verified = @($verifiedAssets | Where-Object { [string]$_.name -ceq [string]$asset.name })[0]
        if ([int64]$asset.id -ne [int64]$verified.id -or [int64]$asset.size -ne [int64]$verified.size -or [string]$asset.state -cne 'uploaded') {
            throw "Draft asset changed after download verification: $($asset.name)"
        }
    }
    $remoteTagCommit = Get-RemoteTagCommit
    if ($remoteTagCommit -ne $localTagCommit) { throw "Remote $Tag moved after checkout; refusing to publish." }
    Write-JsonInput $requestPath @{ draft = $false; prerelease = $false; make_latest = 'true' }
    $published = (Invoke-GhApi "repos/$Repository/releases/$releaseId" 'PATCH' $requestPath) | ConvertFrom-Json
    if ([int64]$published.id -ne $releaseId) { throw 'GitHub returned a different release after publication.' }
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
