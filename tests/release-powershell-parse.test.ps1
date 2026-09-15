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

Add-Type @'
using System;
public class TodoListMockHttpResponse {
    public int StatusCode { get; set; }
}
public class TodoListMockHttpException : Exception {
    public TodoListMockHttpResponse Response { get; private set; }
    public TodoListMockHttpException(int status) : base("fixture HTTP failure") {
        Response = new TodoListMockHttpResponse { StatusCode = status };
    }
}
'@

function New-MockRelease([int64]$Id, [string]$Tag, [string]$Body, [bool]$Draft = $true) {
    $state = $global:TodoListReleaseMock
    return [pscustomobject]@{
        id = $Id
        tag_name = $Tag
        draft = $Draft
        prerelease = $false
        body = $Body
        name = "TodoList $Tag"
        html_url = "https://github.com/$($state.Repository)/releases/tag/$Tag"
        upload_url = "https://uploads.github.com/repos/$($state.Repository)/releases/$Id/assets{?name,label}"
        assets = @()
    }
}

function New-MockReleaseAsset([string]$Name) {
    $state = $global:TodoListReleaseMock
    $state.NextAssetId++
    $id = $state.NextAssetId
    $bytes = [IO.File]::ReadAllBytes((Join-Path $state.BundleDirectory $Name))
    $state.AssetBytes[$id] = $bytes
    return [pscustomobject]@{ id = $id; name = $Name; size = $bytes.Length; state = 'uploaded' }
}

function Convert-MockReleaseToJson($Release) {
    return (ConvertTo-Json -InputObject $Release -Depth 7 -Compress)
}

function global:Start-Sleep {
    param([int]$Seconds)
    $global:TodoListReleaseMock.SleepSeconds.Add($Seconds)
}

function global:gh {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    $state = $global:TodoListReleaseMock
    $global:LASTEXITCODE = 0
    if ($Arguments[0] -ne 'api') {
        $global:LASTEXITCODE = 1
        Write-Output 'gh: tag-based release commands must not be used'
        return
    }
    $endpoint = $Arguments[1]
    $methodIndex = [Array]::IndexOf($Arguments, '--method')
    $method = $Arguments[$methodIndex + 1]
    $inputIndex = [Array]::IndexOf($Arguments, '--input')
    $inputPath = ''
    if ($inputIndex -ge 0) { $inputPath = $Arguments[$inputIndex + 1] }
    $state.Calls.Add([pscustomobject]@{ Method = $method; Endpoint = $endpoint; Download = $false; InputPath = $inputPath })

    if ($method -eq 'GET' -and $endpoint -eq "repos/$($state.Repository)/git/ref/tags/$($state.Tag)") {
        $state.RefCalls++
        $sha = $state.TagObjectSha
        Write-Output (@{ object = @{ type = 'tag'; sha = $sha } } | ConvertTo-Json -Compress)
        return
    }
    if ($method -eq 'GET' -and $endpoint -eq "repos/$($state.Repository)/git/tags/$($state.TagObjectSha)") {
        $sha = $state.Commit
        if ($state.Options.MovedTag -and $state.RefCalls -gt 1) { $sha = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' }
        Write-Output (@{ object = @{ type = 'commit'; sha = $sha } } | ConvertTo-Json -Compress)
        return
    }
    if ($method -eq 'GET' -and $endpoint -match '^repos/[^/]+/[^/]+/releases\?per_page=100&page=(\d+)$') {
        $state.ListCalls++
        $page = [int]$Matches[1]
        if ($state.CreateCalls -gt 0) {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: the new draft is deliberately never visible in the list (HTTP 500)'
            return
        }
        if ($page -le $state.ListPages.Count) { Write-Output (ConvertTo-Json -InputObject @($state.ListPages[$page - 1]) -Depth 7 -Compress) }
        else { Write-Output '[]' }
        return
    }
    if ($method -eq 'POST' -and $endpoint -eq "repos/$($state.Repository)/releases") {
        $state.CreateCalls++
        $request = Get-Content -LiteralPath $inputPath -Raw -Encoding UTF8 | ConvertFrom-Json
        if (-not $request.draft -or $request.prerelease -or $request.tag_name -cne $state.Tag -or $request.target_commitish -ne $state.Commit) {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: invalid draft creation payload (HTTP 422)'
            return
        }
        $state.Release = New-MockRelease $state.ReleaseId $state.Tag ([string]$request.body)
        if ($state.Options.BadUploadUrl) { $state.Release.upload_url = 'https://untrusted.invalid/upload' }
        if ($state.Options.CreateError) {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: response lost after draft creation (HTTP 503)'
            return
        }
        Write-Output (Convert-MockReleaseToJson $state.Release)
        return
    }
    if ($state.Release -and $endpoint -eq "repos/$($state.Repository)/releases/$($state.ReleaseId)") {
        if ($method -eq 'GET') {
            $state.IdGetCalls++
            if ($state.Options.IdGetErrors -and $state.IdGetCalls -le @($state.Options.IdGetErrors).Count) {
                $global:LASTEXITCODE = 1
                Write-Output ("gh: fixture " + $state.Options.IdGetErrors[$state.IdGetCalls - 1])
                return
            }
            $snapshot = (Convert-MockReleaseToJson $state.Release) | ConvertFrom-Json
            if ($state.Options.BadReadId) { $snapshot.id++ }
            if ($state.Options.BadReadTag) { $snapshot.tag_name = 'v0.0.0' }
            if ($state.Options.ChangedAsset -and $state.DownloadCalls -ge 4 -and $snapshot.draft) { $snapshot.assets[0].id += 1000 }
            Write-Output (Convert-MockReleaseToJson $snapshot)
            return
        }
        if ($method -eq 'PATCH') {
            $request = Get-Content -LiteralPath $inputPath -Raw -Encoding UTF8 | ConvertFrom-Json
            if (-not $state.Release.draft) {
                $global:LASTEXITCODE = 1
                Write-Output 'gh: attempted to overwrite a published release (HTTP 422)'
                return
            }
            if ($request.draft -eq $false) {
                if ($state.DownloadCalls -lt 4 -or $request.prerelease -ne $false -or $request.make_latest -cne 'true') {
                    $global:LASTEXITCODE = 1
                    Write-Output 'gh: premature or invalid publication (HTTP 422)'
                    return
                }
                $state.PublishCalls++
                $state.Release.draft = $false
            } else {
                $state.MetadataCalls++
                $state.Release.body = [string]$request.body
                $state.Release.prerelease = [bool]$request.prerelease
            }
            Write-Output (Convert-MockReleaseToJson $state.Release)
            return
        }
    }
    if ($method -eq 'DELETE' -and $endpoint -match '^repos/[^/]+/[^/]+/releases/assets/(\d+)$') {
        $id = [int64]$Matches[1]
        $owned = @($state.Release.assets | Where-Object { [int64]$_.id -eq $id })
        if ($owned.Count -ne 1 -or -not $state.Release.draft) {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: attempted to delete an unrelated or published asset (HTTP 422)'
            return
        }
        $state.DeleteCalls++
        $state.Release.assets = @($state.Release.assets | Where-Object { [int64]$_.id -ne $id })
        $state.AssetBytes.Remove($id)
        return
    }
    if ($method -eq 'POST' -and $endpoint -match '^https://uploads.github.com/repos/[^/]+/[^/]+/releases/(\d+)/assets\?name=(.+)$') {
        if ([int64]$Matches[1] -ne $state.ReleaseId -or -not $state.Release.draft -or $Arguments -notcontains 'Content-Type: application/octet-stream') {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: upload did not bind the correct draft ID and binary content type (HTTP 422)'
            return
        }
        $name = [Uri]::UnescapeDataString($Matches[2])
        if ($name -cnotin $state.AssetNames -or (Split-Path -Leaf $inputPath) -cne $name) {
            $global:LASTEXITCODE = 1
            Write-Output 'gh: unexpected upload asset (HTTP 422)'
            return
        }
        $state.UploadCalls++
        $asset = New-MockReleaseAsset $name
        if ($state.Options.BadSize -and $state.UploadCalls -eq 4) { $asset.size++ }
        $state.Release.assets = @($state.Release.assets) + @($asset)
        Write-Output (ConvertTo-Json -InputObject $asset -Compress)
        return
    }
    $global:LASTEXITCODE = 1
    Write-Output "gh: unexpected or unbound API endpoint $method $endpoint (HTTP 400)"
}

function global:Invoke-WebRequest {
    param([string]$Uri, [hashtable]$Headers, [string]$OutFile, [switch]$UseBasicParsing)
    $state = $global:TodoListReleaseMock
    $state.Calls.Add([pscustomobject]@{ Method = 'GET'; Endpoint = $Uri; Download = $true; InputPath = '' })
    $state.DownloadAttempts++
    if ($state.Options.DownloadErrors -and $state.DownloadAttempts -le @($state.Options.DownloadErrors).Count) {
        throw [TodoListMockHttpException]::new([int]$state.Options.DownloadErrors[$state.DownloadAttempts - 1])
    }
    if ($Headers.Accept -cne 'application/octet-stream' -or $Headers.Authorization -cne 'Bearer fixture-token' -or $Uri -notmatch '^https://api.github.com/repos/[^/]+/[^/]+/releases/assets/(\d+)$') {
        throw 'Binary download did not use the authenticated asset-ID endpoint.'
    }
    $id = [int64]$Matches[1]
    $asset = @($state.Release.assets | Where-Object { [int64]$_.id -eq $id })
    if ($asset.Count -ne 1 -or -not $state.AssetBytes.ContainsKey($id)) { throw 'Download used an asset outside the selected release.' }
    $state.DownloadCalls++
    $bytes = [byte[]]$state.AssetBytes[$id].Clone()
    if ($state.Options.CorruptDownload -and $state.DownloadCalls -eq 1) { $bytes[0] = $bytes[0] -bxor 1 }
    [IO.File]::WriteAllBytes($OutFile, $bytes)
}

function Invoke-MockedPublish([hashtable]$Options = @{}) {
    $fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('todolist-release-publish-test-' + [Guid]::NewGuid().ToString('N'))
    try {
        $fixtureScripts = Join-Path $fixtureRoot 'scripts'
        $fixtureBundle = Join-Path $fixtureRoot 'target/package-build/release/bundle/nsis'
        $fixtureDocs = Join-Path $fixtureRoot 'docs'
        New-Item -ItemType Directory -Path $fixtureScripts, $fixtureBundle, $fixtureDocs | Out-Null
        Copy-Item -LiteralPath (Join-Path $projectRoot 'scripts/publish-github-release.ps1') -Destination $fixtureScripts
        $version = '9.8.7'
        $tag = "v$version"
        $repository = 'example/TodoList'
        $installerName = "TodoList_" + $version + '_x64-setup.exe'
        $assetNames = @($installerName, "$installerName.sig", 'latest.json', 'SHA256SUMS.txt')
        [IO.File]::WriteAllText((Join-Path $fixtureRoot 'package.json'), ('{"version":"' + $version + '"}'))
        [IO.File]::WriteAllText((Join-Path $fixtureDocs "RELEASE_NOTES_v$version.md"), "# Fixture $tag")
        foreach ($name in $assetNames) {
            # Includes non-text bytes, CR/LF and a trailing zero so a text redirection cannot pass.
            $bytes = [byte[]](@(0, 255, 128, 13, 10) + @([Text.Encoding]::UTF8.GetBytes("fixture-$name")) + @(0))
            [IO.File]::WriteAllBytes((Join-Path $fixtureBundle $name), $bytes)
        }

        Push-Location $fixtureRoot
        try {
            & git init --quiet
            & git config user.name fixture
            & git config user.email fixture@example.invalid
            & git config commit.gpgSign false
            & git config tag.gpgSign false
            & git add package.json
            & git commit --quiet -m fixture
            & git tag -a $tag -m $tag
            $commit = (& git rev-parse HEAD).Trim()
        } finally { Pop-Location }

        $global:TodoListReleaseMock = @{
            Repository = $repository; Tag = $tag; TagObjectSha = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            Commit = $commit; ReleaseId = [int64]4242; BundleDirectory = $fixtureBundle; AssetNames = $assetNames
            Release = $null; ListPages = @(); AssetBytes = @{}; NextAssetId = [int64]7000; Options = $Options
            Calls = [Collections.Generic.List[object]]::new(); SleepSeconds = [Collections.Generic.List[int]]::new()
            RefCalls = 0; ListCalls = 0; CreateCalls = 0; IdGetCalls = 0; MetadataCalls = 0
            UploadCalls = 0; DeleteCalls = 0; DownloadCalls = 0; DownloadAttempts = 0; PublishCalls = 0; Error = ''
        }
        $state = $global:TodoListReleaseMock
        $marker = "<!-- todolist-release-workflow tag=$tag commit=$commit -->"
        if ($Options.Existing) {
            $body = "# Existing draft" + [Environment]::NewLine + $marker
            if ($Options.Unowned) { $body = '# Custom draft' }
            if ($Options.WrongMarker) { $body = $marker.Replace($commit, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb') }
            if ($Options.DuplicateMarker) { $body += $marker }
            $state.Release = New-MockRelease $state.ReleaseId $tag $body (-not $Options.Published)
            $assetCount = 4
            if ($Options.Subset) { $assetCount = 2 }
            $state.Release.assets = @($assetNames[0..($assetCount - 1)] | ForEach-Object { New-MockReleaseAsset $_ })
            if ($Options.Subset) { $state.Release.assets[0].state = 'starter' }
            if ($Options.CustomAsset) { $state.Release.assets[0].name = 'custom.txt' }
            if ($Options.DuplicateAsset) { $state.Release.assets[1].name = $state.Release.assets[0].name }
            if ($Options.DuplicateAssetId) { $state.Release.assets[1].id = $state.Release.assets[0].id }

            $unrelated = New-MockRelease 8888 'v1.0.0' '# unrelated published release' $false
            if ($Options.Pagination -or $Options.CrossPageDuplicate) {
                $firstPage = @()
                for ($i = 0; $i -lt 100; $i++) { $firstPage += New-MockRelease (9000 + $i) "v0.0.$i" '# unrelated' $false }
                if ($Options.CrossPageDuplicate) { $firstPage[0] = $state.Release }
                $state.ListPages = @($firstPage, @($state.Release))
            } elseif ($Options.DuplicateRelease) { $state.ListPages = @(,@($state.Release, $state.Release)) }
            else { $state.ListPages = @(,@($unrelated, $state.Release)) }
        }

        $previousGitHubActionsForPublish = $env:GITHUB_ACTIONS
        $previousGhToken = $env:GH_TOKEN
        try {
            $env:GITHUB_ACTIONS = 'true'
            $env:GH_TOKEN = 'fixture-token'
            $publishArguments = @{
                Tag = $tag; Repository = $repository; BundleDirectory = 'target/package-build/release/bundle/nsis'
                NotesPath = "docs/RELEASE_NOTES_v$version.md"
            }
            try { & (Join-Path $fixtureScripts 'publish-github-release.ps1') @publishArguments | Out-Null }
            catch { $state.Error = $_.Exception.Message }
            return $state
        } finally {
            $env:GITHUB_ACTIONS = $previousGitHubActionsForPublish
            $env:GH_TOKEN = $previousGhToken
        }
    } finally {
        if (Test-Path -LiteralPath $fixtureRoot) {
            $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
            $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
            if (-not $resolvedFixture.StartsWith($tempBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup path.' }
            Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
        }
    }
}

$cases = @(
    @{ Name = 'create response ID with list permanently hiding new draft'; Options = @{}; Creates = 1; Deletes = 0; Lists = 1 },
    @{ Name = 'existing owned draft among multiple releases'; Options = @{ Existing = $true }; Creates = 0; Deletes = 4; Lists = 1 },
    @{ Name = 'existing partial draft recovery including starter asset'; Options = @{ Existing = $true; Subset = $true }; Creates = 0; Deletes = 2; Lists = 1 },
    @{ Name = '100-entry page followed by existing draft'; Options = @{ Existing = $true; Pagination = $true }; Creates = 0; Deletes = 4; Lists = 2 },
    @{ Name = 'temporary release GET failures'; Options = @{ IdGetErrors = @('HTTP 404', 'HTTP 503') }; Creates = 1; Deletes = 0; Lists = 1; Sleeps = @(1, 2) },
    @{ Name = 'temporary transport GET failure'; Options = @{ IdGetErrors = @('connection reset') }; Creates = 1; Deletes = 0; Lists = 1; Sleeps = @(1) },
    @{ Name = 'temporary binary asset GET failures'; Options = @{ DownloadErrors = @(404, 503) }; Creates = 1; Deletes = 0; Lists = 1; Sleeps = @(1, 2) },
    @{ Name = 'release GET retry exhaustion'; Options = @{ IdGetErrors = @('HTTP 503', 'HTTP 503', 'HTTP 503', 'HTTP 503') }; Error = 'after 4 attempt'; Gets = 4; Sleeps = @(1, 2, 4) },
    @{ Name = 'release GET 401'; Options = @{ IdGetErrors = @('HTTP 401') }; Error = 'after 1 attempt'; Gets = 1 },
    @{ Name = 'release GET 403'; Options = @{ IdGetErrors = @('HTTP 403') }; Error = 'after 1 attempt'; Gets = 1 },
    @{ Name = 'binary GET retry exhaustion'; Options = @{ DownloadErrors = @(503, 503, 503, 503) }; Error = 'after 4 attempt'; Downloads = 4; Sleeps = @(1, 2, 4) },
    @{ Name = 'binary GET 401'; Options = @{ DownloadErrors = @(401) }; Error = 'after 1 attempt'; Downloads = 1 },
    @{ Name = 'binary GET 403'; Options = @{ DownloadErrors = @(403) }; Error = 'after 1 attempt'; Downloads = 1 },
    @{ Name = 'uncertain create failure never repeats POST'; Options = @{ CreateError = $true }; Error = 'POST failed.*after 1 attempt'; Creates = 1 },
    @{ Name = 'published release rejected'; Options = @{ Existing = $true; Published = $true }; Error = 'published GitHub Release'; NoWrites = $true },
    @{ Name = 'unowned draft rejected'; Options = @{ Existing = $true; Unowned = $true }; Error = 'unowned draft'; NoWrites = $true },
    @{ Name = 'wrong commit ownership marker rejected'; Options = @{ Existing = $true; WrongMarker = $true }; Error = 'unowned draft'; NoWrites = $true },
    @{ Name = 'duplicate ownership marker rejected'; Options = @{ Existing = $true; DuplicateMarker = $true }; Error = 'unowned draft'; NoWrites = $true },
    @{ Name = 'custom draft asset rejected'; Options = @{ Existing = $true; CustomAsset = $true }; Error = 'unexpected asset'; NoWrites = $true },
    @{ Name = 'duplicate draft asset name rejected'; Options = @{ Existing = $true; DuplicateAsset = $true }; Error = 'duplicate asset names'; NoWrites = $true },
    @{ Name = 'duplicate draft asset ID rejected'; Options = @{ Existing = $true; DuplicateAssetId = $true }; Error = 'duplicate asset IDs'; NoWrites = $true },
    @{ Name = 'duplicate releases on same page rejected'; Options = @{ Existing = $true; DuplicateRelease = $true }; Error = 'multiple releases'; NoWrites = $true },
    @{ Name = 'duplicate releases across pages rejected'; Options = @{ Existing = $true; CrossPageDuplicate = $true }; Error = 'multiple releases'; NoWrites = $true },
    @{ Name = 'untrusted upload URL rejected'; Options = @{ BadUploadUrl = $true }; Error = 'invalid release upload URL' },
    @{ Name = 'incorrect release ID rejected'; Options = @{ BadReadId = $true }; Error = 'invalid release for id' },
    @{ Name = 'incorrect release tag rejected'; Options = @{ BadReadTag = $true }; Error = 'invalid release for id' },
    @{ Name = 'uploaded size mismatch rejected'; Options = @{ BadSize = $true }; Error = 'invalid uploaded asset' },
    @{ Name = 'equal-size corrupted downloaded bytes rejected'; Options = @{ CorruptDownload = $true }; Error = 'checksum mismatch' },
    @{ Name = 'asset replacement after verification rejected'; Options = @{ ChangedAsset = $true }; Error = 'asset changed after download verification' },
    @{ Name = 'remote tag movement before publication rejected'; Options = @{ MovedTag = $true }; Error = 'Remote .* moved after checkout' }
)

try {
    foreach ($case in $cases) {
        $state = Invoke-MockedPublish $case.Options
        if ($case.Error) {
            if ($state.Error -notmatch $case.Error) { throw ("{0}: expected {1}; actual: {2}" -f $case.Name, $case.Error, $state.Error) }
            if ($state.PublishCalls -ne 0) { throw "$($case.Name): failed verification published a release." }
            if ($state.Release -and -not $case.Options.Published -and -not $state.Release.draft) { throw "$($case.Name): failure did not retain a draft." }
        } else {
            if ($state.Error) { throw "$($case.Name): $($state.Error)" }
            if ($state.PublishCalls -ne 1 -or $state.Release.draft -or $state.UploadCalls -ne 4 -or $state.DownloadCalls -ne 4) { throw "$($case.Name): did not verify and publish exactly four assets." }
        }
        foreach ($field in @(@('Creates', 'CreateCalls'), @('Deletes', 'DeleteCalls'), @('Lists', 'ListCalls'), @('Gets', 'IdGetCalls'), @('Downloads', 'DownloadAttempts'))) {
            if ($case.ContainsKey($field[0]) -and $state[$field[1]] -ne $case[$field[0]]) { throw "$($case.Name): unexpected $($field[1])." }
        }
        $actualSleeps = @($state.SleepSeconds) -join ','
        $expectedSleeps = @($case.Sleeps) -join ','
        if ($actualSleeps -cne $expectedSleeps) { throw "$($case.Name): unexpected backoff $actualSleeps (expected $expectedSleeps)." }
        if ($case.NoWrites -and @($state.Calls | Where-Object { $_.Method -ne 'GET' }).Count -ne 0) { throw "$($case.Name): refused release was modified." }
        if (@($state.Calls | Where-Object { $_.Endpoint -match '/releases/tags/' }).Count -gt 0) { throw "$($case.Name): tag lookup was used." }
        foreach ($call in $state.Calls) {
            if ($call.Endpoint -notmatch ('^(?:https://(?:api|uploads)\.github\.com/)?repos/' + [regex]::Escape($state.Repository) + '/')) { throw "$($case.Name): operation bound another repository." }
            if ($call.Endpoint -match '/releases/(\d+)(?:/assets|$)' -and [int64]$Matches[1] -ne $state.ReleaseId) { throw "$($case.Name): operation bound another release ID." }
        }
        Write-Output "PASS: $($case.Name)"
    }
} finally {
    Remove-Item Function:/gh -ErrorAction SilentlyContinue
    Remove-Item Function:/Start-Sleep -ErrorAction SilentlyContinue
    Remove-Item Function:/Invoke-WebRequest -ErrorAction SilentlyContinue
    $global:TodoListReleaseMock = $null
}
Write-Output "Release PowerShell scripts parsed; fail-closed checks and $($cases.Count) mocked publication cases passed on PowerShell $($PSVersionTable.PSVersion)."
