$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$script:appVersion = [string]((Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version)
$script:expectedBuild = "todolist/$($script:appVersion)/development"
$coordinator = Join-Path $projectRoot 'src-tauri\windows\install-coordinator.ps1'
$stageRoot = Join-Path $projectRoot 'target\updater-development\release'
$stageMain = Join-Path $stageRoot 'todolist-desktop.exe'
$stageMcp = Join-Path $stageRoot 'todolist-mcp.exe'
$systemPowerShell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('todolist-updater-coordinator-' + [Guid]::NewGuid().ToString('N'))
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalDebug = $env:TODOLIST_INSTALLER_TEST_DEBUG
$originalFault = $env:TODOLIST_INSTALLER_TEST_FAULT
$originalExplorerLog = $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG
$otherProcess = $null

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
}

function Get-TestSha256([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '') }
        finally { $sha256.Dispose() }
    } finally {
        $stream.Dispose()
    }
}

function Invoke-Coordinator(
    [string]$Mode,
    [string]$InstallerPath,
    [string]$ErrorFile,
    [switch]$SuppressExplorer,
    [switch]$ExpectFailure,
    [int]$InstallerPid = 0
) {
    $arguments = @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass',
        '-File', $coordinator,
        '-Mode', $Mode,
        '-InstallDir', $script:installRoot,
        '-MainName', 'todolist-desktop.exe',
        '-Version', $script:appVersion,
        '-ExpectedBuild', $script:expectedBuild,
        '-ProductName', 'TodoList',
        '-BundleId', 'app.todolist.desktop.dev',
        '-InstallerPath', $InstallerPath,
        '-AutoUpdateFlag', '1',
        '-ErrorFile', $ErrorFile,
        '-InstallerPid', $InstallerPid
    )
    if ($Mode -eq 'Prepare') { $arguments += @('-StagedMain', $stageMain, '-StagedMcp', $stageMcp) }
    if ($SuppressExplorer) { $arguments += '-SuppressExplorer' }
    & $systemPowerShell @arguments
    if ($ExpectFailure) {
        Assert-True ($LASTEXITCODE -ne 0) "Coordinator $Mode unexpectedly succeeded"
        return
    }
    if ($LASTEXITCODE -ne 0) {
        $detail = if (Test-Path -LiteralPath $ErrorFile) { Get-Content -LiteralPath $ErrorFile -Raw } else { "exit $LASTEXITCODE" }
        throw "Coordinator $Mode failed: $detail"
    }
}

function New-UpdaterCache([string]$Suffix) {
    $root = Join-Path $script:tempRoot "TodoList-$($script:appVersion)-updater-$Suffix"
    New-Item -ItemType Directory -Force -Path $root | Out-Null
    $installer = Join-Path $root "TodoList-$($script:appVersion)-installer.exe"
    Copy-Item -LiteralPath $stageMain -Destination $installer
    return $installer
}

try {
    foreach ($required in @($coordinator, $stageMain, $stageMcp, $systemPowerShell)) {
        Assert-True (Test-Path -LiteralPath $required -PathType Leaf) "missing fixture $required"
    }
    New-Item -ItemType Directory -Force -Path $testRoot | Out-Null
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    Assert-True ($resolvedTestRoot.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase)) 'test root escaped the Windows temp directory'
    $script:tempRoot = Join-Path $testRoot 'temp'
    $script:installRoot = Join-Path $testRoot 'install'
    $otherRoot = Join-Path $testRoot 'other-channel'
    New-Item -ItemType Directory -Force -Path $script:tempRoot, $script:installRoot, $otherRoot | Out-Null
    $env:TEMP = $script:tempRoot
    $env:TMP = $script:tempRoot
    $env:TODOLIST_INSTALLER_TEST_DEBUG = '1'
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    $explorerLog = Join-Path $testRoot 'explorer-actions.txt'
    $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG = $explorerLog

    $targetMain = Join-Path $script:installRoot 'todolist-desktop.exe'
    $targetMcp = Join-Path $script:installRoot 'todolist-mcp.exe'
    $otherMcp = Join-Path $otherRoot 'todolist-mcp.exe'

    foreach ($fault in @('before-first-backup', 'after-first-move-before-state', 'between-component-moves')) {
        $mainBytes = "original-main-$fault"
        $mcpBytes = "original-mcp-$fault"
        [IO.File]::WriteAllText($targetMain, $mainBytes, [Text.UTF8Encoding]::new($false))
        [IO.File]::WriteAllText($targetMcp, $mcpBytes, [Text.UTF8Encoding]::new($false))
        $faultInstaller = New-UpdaterCache "rollback-$fault"
        $faultError = Join-Path $testRoot "$fault-error.txt"
        $env:TODOLIST_INSTALLER_TEST_FAULT = $fault
        Invoke-Coordinator 'Prepare' $faultInstaller $faultError -ExpectFailure
        $env:TODOLIST_INSTALLER_TEST_FAULT = $null
        Assert-True ((Get-Content -LiteralPath $targetMain -Raw) -eq $mainBytes) "main was not preserved at $fault"
        Assert-True ((Get-Content -LiteralPath $targetMcp -Raw) -eq $mcpBytes) "MCP was not preserved at $fault"
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-install-transaction'))) "transaction remained after rollback at $fault"
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-installing.json'))) "lock remained after rollback at $fault"
        Invoke-Coordinator 'Rollback' $faultInstaller $faultError
        Assert-True ((Get-Content -LiteralPath $targetMain -Raw) -eq $mainBytes) "repeated rollback changed main at $fault"
        Assert-True ((Get-Content -LiteralPath $targetMcp -Raw) -eq $mcpBytes) "repeated rollback changed MCP at $fault"
    }

    Copy-Item -LiteralPath $systemPowerShell -Destination $targetMain
    Copy-Item -LiteralPath $systemPowerShell -Destination $targetMcp
    Copy-Item -LiteralPath $systemPowerShell -Destination $otherMcp
    $holderArguments = '-NoLogo -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 120"'
    $targetMainProcess = Start-Process -FilePath $targetMain -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    $targetMcpProcess = Start-Process -FilePath $targetMcp -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    $otherProcess = Start-Process -FilePath $otherMcp -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 800

    $successInstaller = New-UpdaterCache 'success'
    $successError = Join-Path $testRoot 'success-error.txt'
    Invoke-Coordinator 'Prepare' $successInstaller $successError
    $targetMainProcess.Refresh(); $targetMcpProcess.Refresh(); $otherProcess.Refresh()
    Assert-True $targetMainProcess.HasExited 'target main process was not stopped'
    Assert-True $targetMcpProcess.HasExited 'target MCP process was not stopped'
    Assert-True (-not $otherProcess.HasExited) 'other-channel MCP process was stopped'
    Assert-True (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-installing.json')) 'install lock was not created'

    Copy-Item -LiteralPath $stageMain -Destination $targetMain
    Copy-Item -LiteralPath $stageMcp -Destination $targetMcp
    $lockError = Join-Path $testRoot 'lock-probe.txt'
    $lockProbe = Start-Process -FilePath $targetMcp -PassThru -WindowStyle Hidden -RedirectStandardError $lockError
    Assert-True ($lockProbe.WaitForExit(5000)) 'MCP restart probe did not exit while install lock existed'
    Assert-True ($lockProbe.ExitCode -ne 0) 'MCP restart probe ignored the install lock'
    Assert-True ((Get-Content -LiteralPath $lockError -Raw) -match 'being updated') 'MCP restart probe returned the wrong error'

    Invoke-Coordinator 'Verify' $successInstaller $successError
    $commitArguments = @(
        '-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $coordinator,
        '-Mode', 'Commit', '-InstallDir', $script:installRoot, '-MainName', 'todolist-desktop.exe',
        '-Version', $script:appVersion, '-ExpectedBuild', $script:expectedBuild, '-ProductName', 'TodoList',
        '-BundleId', 'app.todolist.desktop.dev', '-InstallerPath', $successInstaller, '-AutoUpdateFlag', '1',
        '-ErrorFile', $successError, '-InstallerPid', '0'
    )
    & $systemPowerShell @commitArguments
    Assert-True ($LASTEXITCODE -eq 0) 'successful commit failed'
    for ($attempt = 0; $attempt -lt 80 -and (Test-Path -LiteralPath (Split-Path -Parent $successInstaller)); $attempt++) { Start-Sleep -Milliseconds 250 }
    Assert-True (-not (Test-Path -LiteralPath (Split-Path -Parent $successInstaller))) 'successful updater cache was not removed after verification'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-installing.json'))) 'install lock survived successful commit'
    Assert-True (-not $otherProcess.HasExited) 'other-channel MCP process did not survive successful cleanup'

    $cleanupInstaller = New-UpdaterCache 'cleanup-retry'
    $cleanupError = Join-Path $testRoot 'cleanup-retry-error.txt'
    Invoke-Coordinator 'Prepare' $cleanupInstaller $cleanupError
    Copy-Item -LiteralPath $stageMain -Destination $targetMain
    Copy-Item -LiteralPath $stageMcp -Destination $targetMcp
    Invoke-Coordinator 'Verify' $cleanupInstaller $cleanupError
    $env:TODOLIST_INSTALLER_TEST_FAULT = 'cleanup-launch'
    Invoke-Coordinator 'Commit' $cleanupInstaller $cleanupError
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    $cleanupRoot = Split-Path -Parent $cleanupInstaller
    $cleanupStatePath = Join-Path $cleanupRoot '.todolist-update-state.json'
    $cleanupState = Get-Content -LiteralPath $cleanupStatePath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($cleanupState.state -eq 'installed') 'cleanup launch failure changed a successful install into failure'
    Assert-True ($cleanupState.reason -match '清理暂未完成') 'cleanup launch failure did not record pending cleanup'
    Assert-True (-not $cleanupState.explorerOpened) 'cleanup launch failure requested Explorer'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-installing.json'))) 'cleanup launch failure left the install lock active'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $script:installRoot '.todolist-install-transaction'))) 'cleanup launch failure left rollback artifacts'
    Assert-True ((Get-TestSha256 $targetMain) -eq (Get-TestSha256 $stageMain)) 'cleanup launch failure rolled back the verified main executable'
    Assert-True ((Get-TestSha256 $targetMcp) -eq (Get-TestSha256 $stageMcp)) 'cleanup launch failure rolled back the verified MCP executable'

    $unrelatedCacheFile = Join-Path $cleanupRoot 'keep-me.txt'
    [IO.File]::WriteAllText($unrelatedCacheFile, 'unrelated', [Text.UTF8Encoding]::new($false))
    Invoke-Coordinator 'Cleanup' $cleanupInstaller $cleanupError
    $cleanupState = Get-Content -LiteralPath $cleanupStatePath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($cleanupState.state -eq 'installed') 'unrelated cache content changed install success state'
    Assert-True (Test-Path -LiteralPath $unrelatedCacheFile) 'cleanup deleted unrelated cache content'
    Assert-True (Test-Path -LiteralPath $cleanupInstaller) 'cleanup partially deleted an owned installer when unrelated content existed'
    Remove-Item -LiteralPath $unrelatedCacheFile -Force

    $env:TODOLIST_INSTALLER_TEST_FAULT = 'cleanup-delete'
    Invoke-Coordinator 'Cleanup' $cleanupInstaller $cleanupError
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    $cleanupState = Get-Content -LiteralPath $cleanupStatePath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($cleanupState.state -eq 'installed') 'cache deletion failure changed install success state'
    Assert-True ($cleanupState.reason -match '清理暂未完成') 'cache deletion failure was not retained for retry'
    Assert-True (Test-Path -LiteralPath $cleanupInstaller) 'cache deletion failure removed the retryable installer'

    $env:TODOLIST_INSTALLER_TEST_FAULT = 'cleanup-after-installer-delete'
    Invoke-Coordinator 'Cleanup' $cleanupInstaller $cleanupError
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    $cleanupState = Get-Content -LiteralPath $cleanupStatePath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($cleanupState.state -eq 'installed') 'partial cache cleanup changed install success state'
    Assert-True ($cleanupState.reason -match '清理暂未完成') 'partial cache cleanup did not retain a pending marker'
    Assert-True (-not (Test-Path -LiteralPath $cleanupInstaller)) 'partial cache cleanup fixture did not remove the installer first'
    Invoke-Coordinator 'Cleanup' $cleanupInstaller $cleanupError
    Assert-True (-not (Test-Path -LiteralPath $cleanupRoot)) 'pending installed cache was not removed by a later retry'
    Assert-True (Test-Path -LiteralPath $coordinator -PathType Leaf) 'direct Cleanup invocation deleted the source coordinator directory'

    $beforeCancelMain = Get-TestSha256 $targetMain
    $beforeCancelMcp = Get-TestSha256 $targetMcp
    $cancelInstaller = New-UpdaterCache 'cancel'
    $cancelError = Join-Path $testRoot 'cancel-error.txt'
    Invoke-Coordinator 'Prepare' $cancelInstaller $cancelError
    Invoke-Coordinator 'Cancel' $cancelInstaller $cancelError -SuppressExplorer
    Assert-True ((Get-TestSha256 $targetMain) -eq $beforeCancelMain) 'cancel did not restore the main executable'
    Assert-True ((Get-TestSha256 $targetMcp) -eq $beforeCancelMcp) 'cancel did not restore the MCP executable'
    $cancelState = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $cancelInstaller) '.todolist-update-state.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($cancelState.state -eq 'cancelled') 'cancel was not recorded separately'
    Assert-True (-not $cancelState.explorerOpened) 'cancel incorrectly opened Explorer'
    Assert-True (Test-Path -LiteralPath $cancelInstaller) 'cancelled installer was not retained'

    $failedInstaller = New-UpdaterCache 'failed'
    $failedError = Join-Path $testRoot 'failed-error.txt'
    [IO.File]::WriteAllText($failedError, '目标 MCP 文件仍被占用，原版本已恢复。', [Text.UTF8Encoding]::new($false))
    Invoke-Coordinator 'Fail' $failedInstaller $failedError -SuppressExplorer
    $failedState = Get-Content -LiteralPath (Join-Path (Split-Path -Parent $failedInstaller) '.todolist-update-state.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($failedState.state -eq 'failed') 'failure was not recorded'
    Assert-True $failedState.explorerOpened 'failure did not record its one-time Explorer action'
    Assert-True ($failedState.reason -match '目标 MCP') 'failure reason was not preserved'
    Assert-True (Test-Path -LiteralPath $failedInstaller) 'failed installer was not retained'
    Assert-True (-not $otherProcess.HasExited) 'other-channel MCP process was affected by failure handling'

    $retryInstaller = New-UpdaterCache 'explorer-retry'
    $retryError = Join-Path $testRoot 'explorer-retry-error.txt'
    [IO.File]::WriteAllText($retryError, 'first failure', [Text.UTF8Encoding]::new($false))
    Invoke-Coordinator 'Fail' $retryInstaller $retryError
    Invoke-Coordinator 'Fail' $retryInstaller $retryError
    Assert-True (@(Get-Content -LiteralPath $explorerLog).Count -eq 1) 'one failure opened Explorer more than once'
    $env:TODOLIST_INSTALLER_TEST_FAULT = 'before-first-backup'
    Invoke-Coordinator 'Prepare' $retryInstaller $retryError -ExpectFailure
    $env:TODOLIST_INSTALLER_TEST_FAULT = $null
    [IO.File]::WriteAllText($retryError, 'second failure', [Text.UTF8Encoding]::new($false))
    Invoke-Coordinator 'Fail' $retryInstaller $retryError
    Invoke-Coordinator 'Fail' $retryInstaller $retryError
    Assert-True (@(Get-Content -LiteralPath $explorerLog).Count -eq 2) 'a real retry did not receive one fresh Explorer action'

    $unownedInstaller = New-UpdaterCache 'unowned-marker'
    $unownedStatePath = Join-Path (Split-Path -Parent $unownedInstaller) '.todolist-update-state.json'
    $unownedState = [ordered]@{
        owner = 'app.todolist.desktop.updater-cache.v1'
        attemptId = Split-Path -Leaf (Split-Path -Parent $unownedInstaller)
        version = $script:appVersion
        installerFile = Split-Path -Leaf $unownedInstaller
        installerSha256 = 'not-the-installer-hash'
        installDir = $script:installRoot
        state = 'installing'
        reason = 'must remain untouched'
        explorerOpened = $false
        createdAt = [DateTime]::UtcNow.ToString('o')
        updatedAt = [DateTime]::UtcNow.ToString('o')
    }
    [IO.File]::WriteAllText($unownedStatePath, ($unownedState | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    Invoke-Coordinator 'Fail' $unownedInstaller (Join-Path $testRoot 'unowned-error.txt') -SuppressExplorer
    $unownedAfter = Get-Content -LiteralPath $unownedStatePath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-True ($unownedAfter.state -eq 'installing') 'invalid existing ownership marker was overwritten'
    Assert-True ($unownedAfter.installerSha256 -eq 'not-the-installer-hash') 'invalid ownership hash was replaced'
    Assert-True (-not $unownedAfter.explorerOpened) 'invalid ownership marker triggered Explorer handling'
    Assert-True (Test-Path -LiteralPath $unownedInstaller) 'unowned installer was removed'

    Write-Output 'Windows updater coordinator acceptance passed: interruption-safe idempotent rollback, exact-path stop, restart lock, strict cleanup ownership, installed cleanup retry, per-attempt Explorer state, cancel/failure retention, unrelated process preservation.'
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $env:TODOLIST_INSTALLER_TEST_DEBUG = $originalDebug
    $env:TODOLIST_INSTALLER_TEST_FAULT = $originalFault
    $env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG = $originalExplorerLog
    if ($otherProcess -and -not $otherProcess.HasExited) { Stop-Process -Id $otherProcess.Id -Force }
    foreach ($process in @($targetMainProcess, $targetMcpProcess, $lockProbe)) {
        if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    Start-Sleep -Milliseconds 300
    if (Test-Path -LiteralPath $testRoot) {
        $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
        $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
        if ($resolvedTestRoot.StartsWith($resolvedTemp, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $testRoot -Recurse -Force
        }
    }
}
