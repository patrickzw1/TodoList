$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$appVersion = [string]((Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version)
$expectedProductionBuild = "todolist/$appVersion/production"
$sourceHarness = Join-Path $PSScriptRoot 'fixtures\windows-installer-hooks-harness.nsi'
$sourceHooks = Join-Path $projectRoot 'src-tauri\windows\installer-hooks.nsh'
$sourceCoordinator = Join-Path $projectRoot 'src-tauri\windows\install-coordinator.ps1'
$productionMain = Join-Path $projectRoot 'target\package-build\release\todolist-desktop.exe'
$productionMcp = Join-Path $projectRoot 'target\package-build\release\todolist-mcp.exe'
$developmentMcp = Join-Path $projectRoot 'src-tauri\binaries\todolist-mcp-x86_64-pc-windows-msvc.exe'
$makensis = Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'
$systemTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
$testRoot = Join-Path $systemTemp ('todolist-nsis-hooks-' + [Guid]::NewGuid().ToString('N'))
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalDebug = $env:TODOLIST_INSTALLER_TEST_DEBUG
$originalTrace = $env:TODOLIST_INSTALLER_TEST_TRACE_PATH
$originalExplorerLog = $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG
$originalErrorPath = $env:TODOLIST_INSTALLER_TEST_ERROR_PATH
$originalFault = $env:TODOLIST_INSTALLER_TEST_FAULT
$ownedInstallerProcesses = @()
$helperRoots = @()

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
}

function Get-TestSha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '') }
        finally { $algorithm.Dispose() }
    } finally {
        $stream.Dispose()
    }
}

function Assert-DirectTempChild([string]$Path, [string]$Parent, [string]$Prefix) {
    $resolved = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar)
    Assert-True ([string]::Equals((Split-Path -Parent $resolved), $Parent, [StringComparison]::OrdinalIgnoreCase)) "$resolved is not a direct child of $Parent"
    Assert-True ((Split-Path -Leaf $resolved).StartsWith($Prefix, [StringComparison]::OrdinalIgnoreCase)) "$resolved does not have prefix $Prefix"
    return $resolved
}

function Copy-FixtureSource([string]$FixtureRoot) {
    $fixtureHarness = Join-Path $FixtureRoot 'tests\fixtures\windows-installer-hooks-harness.nsi'
    $fixtureHooks = Join-Path $FixtureRoot 'src-tauri\windows\installer-hooks.nsh'
    $fixtureCoordinator = Join-Path $FixtureRoot 'src-tauri\windows\install-coordinator.ps1'
    $fixtureMain = Join-Path $FixtureRoot 'target\installer-acceptance\release\todolist-desktop.exe'
    $fixtureMcp = Join-Path $FixtureRoot 'src-tauri\binaries\todolist-mcp-x86_64-pc-windows-msvc.exe'
    foreach ($directory in @(
        (Split-Path -Parent $fixtureHarness),
        (Split-Path -Parent $fixtureHooks),
        (Split-Path -Parent $fixtureMain),
        (Split-Path -Parent $fixtureMcp)
    )) {
        [IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    Copy-Item -LiteralPath $sourceHarness -Destination $fixtureHarness
    Copy-Item -LiteralPath $sourceHooks -Destination $fixtureHooks
    Copy-Item -LiteralPath $sourceCoordinator -Destination $fixtureCoordinator
    Copy-Item -LiteralPath $productionMain -Destination $fixtureMain
    Copy-Item -LiteralPath $productionMcp -Destination $fixtureMcp
    return $fixtureHarness
}

function Start-Harness([string]$Installer, [string]$InstallRoot, [int]$TimeoutMilliseconds = 45000) {
    $process = Start-Process -FilePath $Installer -ArgumentList @('/S', '/TODOLIST_AUTO_UPDATE=1', "/D=$InstallRoot") -WindowStyle Hidden -PassThru
    $script:ownedInstallerProcesses += $process
    if (-not $process.WaitForExit($TimeoutMilliseconds)) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        throw "NSIS harness timed out after $TimeoutMilliseconds ms"
    }
    $process.Refresh()
    return $process
}

function Wait-Until([scriptblock]$Condition, [int]$TimeoutMilliseconds, [string]$Message) {
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMilliseconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (& $Condition) { return }
        Start-Sleep -Milliseconds 200
    }
    throw $Message
}

function Read-Trace([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return @() }
    return @(Get-Content -LiteralPath $Path -Encoding UTF8 | Where-Object { $_ })
}

function Get-TraceField([string]$Line, [string]$Name) {
    foreach ($part in $Line.Split('|')) {
        if ($part.StartsWith("$Name=", [StringComparison]::Ordinal)) { return $part.Substring($Name.Length + 1) }
    }
    return $null
}

try {
    foreach ($required in @($sourceHarness, $sourceHooks, $sourceCoordinator, $productionMain, $productionMcp, $developmentMcp, $makensis)) {
        Assert-True (Test-Path -LiteralPath $required -PathType Leaf) "missing fixture input: $required"
    }
    Assert-True (([Diagnostics.FileVersionInfo]::GetVersionInfo($productionMain).ProductVersion) -eq $appVersion) 'production desktop fixture has the wrong version'
    $productionIdentity = (& $productionMcp --print-build-identity 2>&1 | Out-String).Trim()
    Assert-True ($LASTEXITCODE -eq 0 -and $productionIdentity -eq $expectedProductionBuild) "production MCP fixture has the wrong identity: $productionIdentity"
    $workspaceMcpHash = Get-TestSha256 $developmentMcp
    $workspaceMcpIdentity = (& $developmentMcp --print-build-identity 2>&1 | Out-String).Trim()
    Assert-True ($LASTEXITCODE -eq 0) 'workspace staged MCP identity probe failed'

    $resolvedTestRoot = Assert-DirectTempChild $testRoot $systemTemp 'todolist-nsis-hooks-'
    [IO.Directory]::CreateDirectory($resolvedTestRoot) | Out-Null
    $fixtureRoot = Join-Path $resolvedTestRoot 'source'
    $fixtureHarness = Copy-FixtureSource $fixtureRoot
    $compiledHarness = Join-Path $resolvedTestRoot 'TodoList-hooks-harness.exe'
    & $makensis '/V2' "/DHARNESS_OUTFILE=$compiledHarness" "/DAPP_VERSION=$appVersion" $fixtureHarness
    Assert-True ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $compiledHarness -PathType Leaf)) 'NSIS hook harness compilation failed'
    Assert-True ((Get-TestSha256 $developmentMcp) -eq $workspaceMcpHash) 'fixture compilation changed the workspace staged MCP bytes'
    Assert-True ((& $developmentMcp --print-build-identity 2>&1 | Out-String).Trim() -eq $workspaceMcpIdentity) 'fixture compilation changed the workspace staged MCP identity'

    $fixtureTemp = Join-Path $resolvedTestRoot 'temp'
    [IO.Directory]::CreateDirectory($fixtureTemp) | Out-Null
    $env:TEMP = $fixtureTemp
    $env:TMP = $fixtureTemp
    $env:TODOLIST_INSTALLER_TEST_DEBUG = '1'
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null

    $successInstall = Join-Path $resolvedTestRoot 'success-install'
    [IO.Directory]::CreateDirectory($successInstall) | Out-Null
    [IO.File]::WriteAllText((Join-Path $successInstall 'todolist-desktop.exe'), 'old-main', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $successInstall 'todolist-mcp.exe'), 'old-mcp', [Text.UTF8Encoding]::new($false))
    $successAttempt = Join-Path $fixtureTemp ("TodoList-$appVersion-updater-nsis-success-" + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($successAttempt) | Out-Null
    $successInstaller = Join-Path $successAttempt "TodoList-$appVersion-installer.exe"
    Copy-Item -LiteralPath $compiledHarness -Destination $successInstaller
    $successTrace = Join-Path $resolvedTestRoot 'success-trace.log'
    $successExplorer = Join-Path $resolvedTestRoot 'success-explorer.log'
    $env:TODOLIST_INSTALLER_TEST_TRACE_PATH = $successTrace
    $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG = $successExplorer
    $env:TODOLIST_INSTALLER_TEST_ERROR_PATH = Join-Path $resolvedTestRoot 'success-error.log'

    $successProcess = Start-Harness $successInstaller $successInstall
    Assert-True ($successProcess.ExitCode -eq 0) "success NSIS harness exited with $($successProcess.ExitCode)"
    Assert-True (([Diagnostics.FileVersionInfo]::GetVersionInfo((Join-Path $successInstall 'todolist-desktop.exe')).ProductVersion) -eq $appVersion) 'success harness installed the wrong desktop version'
    $installedIdentity = (& (Join-Path $successInstall 'todolist-mcp.exe') --print-build-identity 2>&1 | Out-String).Trim()
    Assert-True ($LASTEXITCODE -eq 0 -and $installedIdentity -eq $expectedProductionBuild) "success harness installed the wrong MCP: $installedIdentity"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $successInstall '.todolist-installing.json'))) 'success harness left the install lock'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $successInstall '.todolist-install-transaction'))) 'success harness left the transaction directory'

    Wait-Until { -not (Test-Path -LiteralPath $successAttempt) } 30000 'success updater cache was not removed after the real installer exited'
    Wait-Until { (Read-Trace $successTrace | Where-Object { $_ -like 'helper-removed|*' }).Count -eq 1 } 30000 'cleanup helper did not report self-removal'
    $successLines = Read-Trace $successTrace
    foreach ($mode in @('Prepare', 'Verify', 'Commit', 'Cleanup')) {
        Assert-True (($successLines | Where-Object { $_ -like "mode=$mode|*" }).Count -eq 1) "real hook trace did not contain exactly one $mode invocation"
    }
    $commitLine = $successLines | Where-Object { $_ -like 'mode=Commit|*' } | Select-Object -First 1
    $cleanupLine = $successLines | Where-Object { $_ -like 'mode=Cleanup|*' } | Select-Object -First 1
    $commitInstallerPid = [int](Get-TraceField $commitLine 'installerPid')
    $cleanupInstallerPid = [int](Get-TraceField $cleanupLine 'installerPid')
    Assert-True ($commitInstallerPid -gt 0 -and $commitInstallerPid -eq $successProcess.Id) 'Commit did not receive the real nonzero NSIS process id'
    Assert-True ($cleanupInstallerPid -eq $successProcess.Id) 'Cleanup did not wait on the same NSIS process id'
    Assert-True (($successLines | Where-Object { $_ -like 'cache-removed|*' }).Count -eq 1) 'real success hook did not report owned cache removal'
    $helperLine = $successLines | Where-Object { $_ -like 'helper-started|*' } | Select-Object -First 1
    $helperRoot = Get-TraceField $helperLine 'root'
    $helperRoots += Assert-DirectTempChild $helperRoot ([IO.Path]::GetFullPath($fixtureTemp).TrimEnd([IO.Path]::DirectorySeparatorChar)) 'TodoList-update-cleanup-'
    Assert-True (-not (Test-Path -LiteralPath $helperRoot)) 'cleanup helper directory survived success'
    Assert-True (-not (Test-Path -LiteralPath $successExplorer)) 'successful install unexpectedly requested Explorer'

    $failureInstall = Join-Path $resolvedTestRoot 'failure-install'
    [IO.Directory]::CreateDirectory($failureInstall) | Out-Null
    $oldMain = 'failure-original-main'
    $oldMcp = 'failure-original-mcp'
    [IO.File]::WriteAllText((Join-Path $failureInstall 'todolist-desktop.exe'), $oldMain, [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $failureInstall 'todolist-mcp.exe'), $oldMcp, [Text.UTF8Encoding]::new($false))
    $failureAttempt = Join-Path $fixtureTemp ("TodoList-$appVersion-updater-nsis-failure-" + [Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($failureAttempt) | Out-Null
    $failureInstaller = Join-Path $failureAttempt "TodoList-$appVersion-installer.exe"
    Copy-Item -LiteralPath $compiledHarness -Destination $failureInstaller
    $failureTrace = Join-Path $resolvedTestRoot 'failure-trace.log'
    $failureExplorer = Join-Path $resolvedTestRoot 'failure-explorer.log'
    $env:TODOLIST_INSTALLER_TEST_TRACE_PATH = $failureTrace
    $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG = $failureExplorer
    $env:TODOLIST_INSTALLER_TEST_ERROR_PATH = Join-Path $resolvedTestRoot 'failure-error.log'
    $env:TODOLIST_INSTALLER_TEST_FAULT = 'after-first-move-before-state'
    try {
        $failureProcess = Start-Harness $failureInstaller $failureInstall
    } finally {
        $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    }
    Assert-True ($failureProcess.ExitCode -ne 0) 'fault-injected NSIS harness unexpectedly succeeded'
    Assert-True ((Get-Content -LiteralPath (Join-Path $failureInstall 'todolist-desktop.exe') -Raw) -eq $oldMain) 'real failed hook did not restore the original desktop file'
    Assert-True ((Get-Content -LiteralPath (Join-Path $failureInstall 'todolist-mcp.exe') -Raw) -eq $oldMcp) 'real failed hook changed the original MCP file'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failureInstall '.todolist-installing.json'))) 'failed hook left the install lock'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failureInstall '.todolist-install-transaction'))) 'failed hook left the transaction directory'
    Assert-True (Test-Path -LiteralPath $failureInstaller -PathType Leaf) 'failed hook did not retain its owned installer'
    $failureState = Get-Content -LiteralPath (Join-Path $failureAttempt '.todolist-update-state.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($failureState.state -eq 'failed') 'real failed hook did not record failed state'
    Assert-True $failureState.explorerOpened 'real failed hook did not record the Explorer action'
    Assert-True (@(Get-Content -LiteralPath $failureExplorer -Encoding UTF8).Count -eq 1) 'real failed hook did not record exactly one Explorer action'
    $failureLines = Read-Trace $failureTrace
    Assert-True (($failureLines | Where-Object { $_ -like 'mode=Prepare|*' }).Count -eq 1) 'failed hook did not invoke Prepare once'
    Assert-True (($failureLines | Where-Object { $_ -like 'mode=Fail|*' }).Count -eq 1) 'failed hook did not invoke Fail once'
    Assert-True (($failureLines | Where-Object { $_ -like 'mode=Verify|*' -or $_ -like 'mode=Commit|*' }).Count -eq 0) 'failed hook advanced past Prepare'

    Write-Output "Real NSIS hook acceptance passed: successPid=$($successProcess.Id), commitInstallerPid=$commitInstallerPid, cleanupInstallerPid=$cleanupInstallerPid, installedMcp=$installedIdentity, cacheRemoved=$(-not (Test-Path -LiteralPath $successAttempt)), helperRemoved=$(-not (Test-Path -LiteralPath $helperRoot)), failureExit=$($failureProcess.ExitCode), explorerActions=1, rollbackPreserved=true."
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $env:TODOLIST_INSTALLER_TEST_DEBUG = $originalDebug
    $env:TODOLIST_INSTALLER_TEST_TRACE_PATH = $originalTrace
    $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG = $originalExplorerLog
    $env:TODOLIST_INSTALLER_TEST_ERROR_PATH = $originalErrorPath
    $env:TODOLIST_INSTALLER_TEST_FAULT = $originalFault

    foreach ($process in $ownedInstallerProcesses) {
        if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    foreach ($helperRoot in $helperRoots) {
        if ([string]::IsNullOrWhiteSpace($helperRoot)) { continue }
        $helperLeaf = Split-Path -Leaf $helperRoot
        $helperParent = Split-Path -Parent ([IO.Path]::GetFullPath($helperRoot))
        if ($helperLeaf.StartsWith('TodoList-update-cleanup-', [StringComparison]::OrdinalIgnoreCase) -and
            [string]::Equals($helperParent, [IO.Path]::GetFullPath((Join-Path $testRoot 'temp')).TrimEnd([IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase) -and
            (Test-Path -LiteralPath $helperRoot)) {
            Remove-Item -LiteralPath $helperRoot -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Milliseconds 300
    if (Test-Path -LiteralPath $testRoot) {
        $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot).TrimEnd([IO.Path]::DirectorySeparatorChar)
        $resolvedSystemTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
        if ([string]::Equals((Split-Path -Parent $resolvedTestRoot), $resolvedSystemTemp, [StringComparison]::OrdinalIgnoreCase) -and
            (Split-Path -Leaf $resolvedTestRoot).StartsWith('todolist-nsis-hooks-', [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedTestRoot -Recurse -Force
        }
    }
}
