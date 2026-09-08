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

function New-MockReleaseAssets {
    $state = $global:TodoListReleaseMock
    $index = 0
    return @($state.AssetNames | ForEach-Object {
        $index++
        [pscustomobject]@{
            id = 7000 + $index
            name = $_
            size = (Get-Item -LiteralPath (Join-Path $state.BundleDirectory $_)).Length
        }
    })
}

function Convert-MockReleaseToJson($Release) {
    $snapshot = [ordered]@{
        id = [int64]$Release.id
        tag_name = [string]$Release.tag_name
        draft = [bool]$Release.draft
        prerelease = [bool]$Release.prerelease
        body = [string]$Release.body
        html_url = [string]$Release.html_url
        assets = @($Release.assets | ForEach-Object {
            [ordered]@{
                id = [int64]$_.id
                name = [string]$_.name
                size = [int64]$_.size
            }
        })
    }
    return ($snapshot | ConvertTo-Json -Depth 5 -Compress)
}

function global:gh {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    $state = $global:TodoListReleaseMock
    $state.Calls.Add(($Arguments -join ' '))
    $global:LASTEXITCODE = 0

    if ($Arguments[0] -eq 'api') {
        $endpoint = $Arguments[1]
        if ($endpoint -eq "repos/$($state.Repository)/releases/tags/$($state.Tag)") {
            $state.ByTagCalls++
            $global:LASTEXITCODE = 1
            Write-Output 'gh: Not Found (HTTP 404)'
            return
        }
        if ($endpoint -eq "repos/$($state.Repository)/git/ref/tags/$($state.Tag)") {
            Write-Output (@{ object = @{ type = 'tag'; sha = $state.TagObjectSha } } | ConvertTo-Json -Compress)
            return
        }
        if ($endpoint -eq "repos/$($state.Repository)/git/tags/$($state.TagObjectSha)") {
            Write-Output (@{ object = @{ type = 'commit'; sha = $state.Commit } } | ConvertTo-Json -Compress)
            return
        }
        if ($endpoint -like "repos/$($state.Repository)/releases?per_page=100&page=*") {
            $state.ListCalls++
            if ($null -eq $state.Release) { Write-Output '[]' }
            else { Write-Output ('[' + (Convert-MockReleaseToJson $state.Release) + ']') }
            return
        }
        if ($state.Release -and $endpoint -eq "repos/$($state.Repository)/releases/$($state.Release.id)") {
            Write-Output (Convert-MockReleaseToJson $state.Release)
            return
        }
        $global:LASTEXITCODE = 1
        Write-Output "gh: unexpected API endpoint $endpoint (HTTP 500)"
        return
    }

    if ($Arguments[0] -eq 'release' -and $Arguments[1] -eq 'create') {
        $state.CreateCalls++
        $notesIndex = [Array]::IndexOf($Arguments, '--notes-file')
        $state.Release = [pscustomobject]@{
            id = $state.ReleaseId
            tag_name = $state.Tag
            draft = $true
            prerelease = $false
            body = Get-Content -LiteralPath $Arguments[$notesIndex + 1] -Raw -Encoding UTF8
            html_url = "https://github.com/$($state.Repository)/releases/tag/$($state.Tag)"
            assets = @(New-MockReleaseAssets)
        }
        Write-Output $state.Release.html_url
        return
    }
    if ($Arguments[0] -eq 'release' -and $Arguments[1] -eq 'edit') {
        $notesIndex = [Array]::IndexOf($Arguments, '--notes-file')
        if ($notesIndex -ge 0) { $state.Release.body = Get-Content -LiteralPath $Arguments[$notesIndex + 1] -Raw -Encoding UTF8 }
        if ($Arguments -contains '--draft=false') {
            $state.PublishCalls++
            $state.Release.draft = $false
        }
        Write-Output $state.Release.html_url
        return
    }
    if ($Arguments[0] -eq 'release' -and $Arguments[1] -eq 'upload') {
        $state.Release.assets = @(New-MockReleaseAssets)
        return
    }
    if ($Arguments[0] -eq 'release' -and $Arguments[1] -eq 'download') {
        $directoryIndex = [Array]::IndexOf($Arguments, '--dir')
        $destination = $Arguments[$directoryIndex + 1]
        foreach ($name in $state.AssetNames) {
            Copy-Item -LiteralPath (Join-Path $state.BundleDirectory $name) -Destination (Join-Path $destination $name)
        }
        if ($state.CorruptDownload) {
            [IO.File]::AppendAllText((Join-Path $destination $state.AssetNames[0]), 'corrupt')
        }
        return
    }

    $global:LASTEXITCODE = 1
    Write-Output "gh: unexpected command $($Arguments -join ' ')"
}

function Invoke-MockedPublish([bool]$ExistingDraft, [bool]$CorruptDownload) {
    $fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('todolist-release-publish-test-' + [Guid]::NewGuid().ToString('N'))
    try {
        $fixtureScripts = Join-Path $fixtureRoot 'scripts'
        $fixtureBundle = Join-Path $fixtureRoot 'target\package-build\release\bundle\nsis'
        $fixtureDocs = Join-Path $fixtureRoot 'docs'
        New-Item -ItemType Directory -Path $fixtureScripts, $fixtureBundle, $fixtureDocs | Out-Null
        Copy-Item -LiteralPath (Join-Path $projectRoot 'scripts\publish-github-release.ps1') -Destination $fixtureScripts
        $version = '9.8.7'
        $tag = "v$version"
        $repository = 'example/TodoList'
        $installerName = "TodoList_${version}_x64-setup.exe"
        $assetNames = @($installerName, "$installerName.sig", 'latest.json', 'SHA256SUMS.txt')
        [IO.File]::WriteAllText((Join-Path $fixtureRoot 'package.json'), "{`"version`":`"$version`"}")
        [IO.File]::WriteAllText((Join-Path $fixtureDocs "RELEASE_NOTES_v$version.md"), "# Fixture $tag")
        foreach ($name in $assetNames) { [IO.File]::WriteAllText((Join-Path $fixtureBundle $name), "fixture-$name") }

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
        } finally {
            Pop-Location
        }

        $global:TodoListReleaseMock = @{
            Repository = $repository
            Tag = $tag
            TagObjectSha = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            Commit = $commit
            ReleaseId = 4242
            BundleDirectory = $fixtureBundle
            AssetNames = $assetNames
            Release = $null
            CorruptDownload = $CorruptDownload
            Calls = [Collections.Generic.List[string]]::new()
            ByTagCalls = 0
            ListCalls = 0
            CreateCalls = 0
            PublishCalls = 0
        }
        $marker = "<!-- todolist-release-workflow tag=$tag commit=$commit -->"
        if ($ExistingDraft) {
            $global:TodoListReleaseMock.Release = [pscustomobject]@{
                id = 4242
                tag_name = $tag
                draft = $true
                prerelease = $false
                body = "# Existing draft`r`n`r`n$marker`r`n"
                html_url = "https://github.com/$repository/releases/tag/$tag"
                assets = @(New-MockReleaseAssets)
            }
            $byTagResult = (& gh api "repos/$repository/releases/tags/$tag" 2>&1 | Out-String).Trim()
            if ($LASTEXITCODE -ne 1 -or $byTagResult -notmatch 'HTTP 404') { throw 'Mock did not reproduce tag lookup 404.' }
        }

        $previousGitHubActionsForPublish = $env:GITHUB_ACTIONS
        $previousGhToken = $env:GH_TOKEN
        try {
            $env:GITHUB_ACTIONS = 'true'
            $env:GH_TOKEN = 'fixture-token'
            & (Join-Path $fixtureScripts 'publish-github-release.ps1') `
                -Tag $tag `
                -Repository $repository `
                -BundleDirectory 'target/package-build/release/bundle/nsis' `
                -NotesPath "docs/RELEASE_NOTES_v$version.md" | Out-Null
            return $global:TodoListReleaseMock
        } finally {
            $env:GITHUB_ACTIONS = $previousGitHubActionsForPublish
            $env:GH_TOKEN = $previousGhToken
        }
    } finally {
        if (Test-Path -LiteralPath $fixtureRoot) {
            $tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
            $resolvedFixture = [IO.Path]::GetFullPath($fixtureRoot)
            if ($resolvedFixture.StartsWith($tempBase + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
            }
        }
    }
}

$created = Invoke-MockedPublish $false $false
if ($created.CreateCalls -ne 1 -or $created.PublishCalls -ne 1 -or $created.Release.draft) {
    throw 'A newly created draft was not found through the release list and published.'
}
$existing = Invoke-MockedPublish $true $false
if ($existing.ByTagCalls -ne 1 -or $existing.CreateCalls -ne 0 -or $existing.PublishCalls -ne 1 -or $existing.Release.draft) {
    throw 'A listed draft was not reused after the by-tag endpoint returned HTTP 404.'
}
try {
    Invoke-MockedPublish $false $true | Out-Null
    throw 'A corrupted downloaded asset was published unexpectedly.'
} catch {
    if ($_.Exception.Message -notmatch 'Uploaded asset checksum mismatch') { throw }
    if ($global:TodoListReleaseMock.PublishCalls -ne 0 -or -not $global:TodoListReleaseMock.Release.draft) {
        throw 'A failed asset verification did not remain an unpublished draft.'
    }
}

Remove-Item Function:\gh -ErrorAction SilentlyContinue
$global:TodoListReleaseMock = $null
Write-Output 'Release PowerShell scripts parsed; fail-closed and mocked draft publication checks passed.'
