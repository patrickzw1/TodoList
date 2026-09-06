$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$signingDirectory = Join-Path $env:LOCALAPPDATA 'TodoListRelease\signing'
$privateKey = Join-Path $signingDirectory 'updater.key'
$passwordFile = Join-Path $signingDirectory 'updater.password.dpapi'
if (-not (Test-Path -LiteralPath $privateKey) -or -not (Test-Path -LiteralPath $passwordFile)) {
    throw 'Release signing material is missing. See docs/UPDATES.md.'
}

$variables = @('TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS', 'CARGO_ENCODED_RUSTFLAGS')
$previousEnvironment = @{}
foreach ($name in $variables) { $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
Push-Location $projectRoot
try {
    $securePassword = Get-Content -LiteralPath $passwordFile -Raw | ConvertTo-SecureString
    $env:TAURI_SIGNING_PRIVATE_KEY = $privateKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Net.NetworkCredential]::new('', $securePassword).Password
    $env:CARGO_TARGET_DIR = Join-Path $projectRoot 'target\package-build'
    $env:CARGO_BUILD_JOBS = '1'
    # Keep developer paths out of Rust panic strings and Windows debug-directory records.
    $rustFlags = if ($env:CARGO_ENCODED_RUSTFLAGS) {
        @($env:CARGO_ENCODED_RUSTFLAGS.Split([char]0x1f))
    } elseif ($env:RUSTFLAGS) {
        @($env:RUSTFLAGS -split '\s+')
    } else { @() }
    $env:CARGO_ENCODED_RUSTFLAGS = ($rustFlags + @(
        "--remap-path-prefix=$projectRoot=TodoList",
        "--remap-path-prefix=$env:USERPROFILE=build-user",
        '-Clink-arg=/PDBALTPATH:TodoList.pdb'
    )) -join [char]0x1f
    & npm.cmd run build:desktop -- --ci --config src-tauri/tauri.release.conf.json --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw 'Desktop release build failed.' }

    $version = (Get-Content -LiteralPath package.json -Raw | ConvertFrom-Json).version
    $installerName = "TodoList_${version}_x64-setup.exe"
    $bundleDirectory = Join-Path $env:CARGO_TARGET_DIR 'release\bundle\nsis'
    $installer = Join-Path $bundleDirectory $installerName
    $signatureFile = "$installer.sig"
    if (-not (Test-Path -LiteralPath $installer) -or -not (Test-Path -LiteralPath $signatureFile)) {
        throw 'Signed NSIS installer was not produced.'
    }
    $manifest = [ordered]@{
        version = $version
        notes = "TodoList $version for Windows x64. See the GitHub release notes."
        pub_date = [DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = @{
            'windows-x86_64' = @{
                signature = (Get-Content -LiteralPath $signatureFile -Raw).Trim()
                url = "https://github.com/patrickzw1/TodoList/releases/download/v$version/$installerName"
            }
        }
    }
    $manifestPath = Join-Path $bundleDirectory 'latest.json'
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
    $checksums = foreach ($file in @($installer, $signatureFile, $manifestPath)) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant(), (Split-Path -Leaf $file)
    }
    [IO.File]::WriteAllLines((Join-Path $bundleDirectory 'SHA256SUMS.txt'), $checksums, [Text.UTF8Encoding]::new($false))
    Write-Output "Release artifacts: $bundleDirectory"
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process') }
    $securePassword = $null
    Pop-Location
}
