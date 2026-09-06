[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Prepare', 'Verify', 'Commit', 'Rollback', 'Fail', 'Cancel', 'StopOnly', 'Cleanup')]
    [string]$Mode,
    [Parameter(Mandatory = $true)] [string]$InstallDir,
    [Parameter(Mandatory = $true)] [string]$MainName,
    [Parameter(Mandatory = $true)] [string]$Version,
    [Parameter(Mandatory = $true)] [string]$ExpectedBuild,
    [Parameter(Mandatory = $true)] [string]$ProductName,
    [Parameter(Mandatory = $true)] [string]$BundleId,
    [string]$StagedMain,
    [string]$StagedMcp,
    [string]$InstallerPath,
    [string]$AutoUpdateFlag,
    [string]$ErrorFile,
    [int]$InstallerPid,
    [switch]$SuppressExplorer
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$owner = 'app.todolist.desktop.installer-transaction.v1'
$cacheOwner = 'app.todolist.desktop.updater-cache.v1'
$installRoot = [IO.Path]::GetFullPath($InstallDir).TrimEnd([IO.Path]::DirectorySeparatorChar)
$transactionRoot = Join-Path $installRoot '.todolist-install-transaction'
$transactionFile = Join-Path $transactionRoot 'transaction.json'
$installLock = Join-Path $installRoot '.todolist-installing.json'

function Write-JsonAtomic([string]$Path, $Value) {
    $parent = Split-Path -Parent $Path
    [IO.Directory]::CreateDirectory($parent) | Out-Null
    $temporary = "$Path.tmp"
    [IO.File]::WriteAllText($temporary, ($Value | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $temporary -Destination $Path -Force
}

function Write-ErrorDetail([string]$Message) {
    if (-not [string]::IsNullOrWhiteSpace($ErrorFile)) {
        $parent = Split-Path -Parent $ErrorFile
        if ($parent) { [IO.Directory]::CreateDirectory($parent) | Out-Null }
        [IO.File]::WriteAllText($ErrorFile, $Message, [Text.UTF8Encoding]::new($false))
    }
    if ($env:TODOLIST_INSTALLER_TEST_DEBUG -eq '1' -and
        -not [string]::IsNullOrWhiteSpace($env:TODOLIST_INSTALLER_TEST_ERROR_PATH) -and
        -not (Test-Path -LiteralPath $env:TODOLIST_INSTALLER_TEST_ERROR_PATH -PathType Leaf)) {
        $testErrorPath = [IO.Path]::GetFullPath($env:TODOLIST_INSTALLER_TEST_ERROR_PATH)
        $testParent = Split-Path -Parent $testErrorPath
        if ($testParent) { [IO.Directory]::CreateDirectory($testParent) | Out-Null }
        [IO.File]::WriteAllText($testErrorPath, $Message, [Text.UTF8Encoding]::new($false))
    }
}

function Write-TestTrace([string]$Event) {
    if ($env:TODOLIST_INSTALLER_TEST_DEBUG -ne '1' -or
        [string]::IsNullOrWhiteSpace($env:TODOLIST_INSTALLER_TEST_TRACE_PATH)) { return }
    $tracePath = [IO.Path]::GetFullPath($env:TODOLIST_INSTALLER_TEST_TRACE_PATH)
    $traceParent = Split-Path -Parent $tracePath
    if ($traceParent) { [IO.Directory]::CreateDirectory($traceParent) | Out-Null }
    [IO.File]::AppendAllText($tracePath, "$Event`r`n", [Text.UTF8Encoding]::new($false))
}

function Get-Sha256([string]$Path) {
    $stream = $null
    $algorithm = $null
    try {
        $stream = [IO.File]::OpenRead($Path)
        $algorithm = [Security.Cryptography.SHA256]::Create()
        return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace('-', '').ToLowerInvariant()
    } finally {
        if ($algorithm) { $algorithm.Dispose() }
        if ($stream) { $stream.Dispose() }
    }
}

function Same-Path([string]$Left, [string]$Right) {
    if ([string]::IsNullOrWhiteSpace($Left) -or [string]::IsNullOrWhiteSpace($Right)) { return $false }
    return [string]::Equals(
        [IO.Path]::GetFullPath($Left).TrimEnd([IO.Path]::DirectorySeparatorChar),
        [IO.Path]::GetFullPath($Right).TrimEnd([IO.Path]::DirectorySeparatorChar),
        [StringComparison]::OrdinalIgnoreCase
    )
}

function Assert-PlainDirectChild([string]$Path, [string]$ExpectedParent, [string]$ExpectedLeaf) {
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $parent = Split-Path -Parent $fullPath
    $leaf = Split-Path -Leaf $fullPath
    if (-not (Same-Path $parent $ExpectedParent) -or
        -not [string]::Equals($leaf, $ExpectedLeaf, [StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝清理无法确认归属的目录：$fullPath"
    }
    if (Test-Path -LiteralPath $fullPath) {
        $item = Get-Item -LiteralPath $fullPath -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "拒绝清理非普通目录：$fullPath"
        }
    }
    return $fullPath
}

function Remove-KnownDirectory([string]$Path, [string]$ExpectedParent, [string]$ExpectedLeaf, [string[]]$AllowedFiles) {
    $fullPath = Assert-PlainDirectChild $Path $ExpectedParent $ExpectedLeaf
    if (-not (Test-Path -LiteralPath $fullPath -PathType Container)) { return }
    $entries = @(Get-ChildItem -LiteralPath $fullPath -Force)
    foreach ($entry in $entries) {
        if ($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            -not ($AllowedFiles | Where-Object { [string]::Equals($_, $entry.Name, [StringComparison]::OrdinalIgnoreCase) })) {
            throw "目录包含无法确认归属的内容，已保留：$($entry.FullName)"
        }
    }
    foreach ($entry in $entries) {
        Remove-Item -LiteralPath $entry.FullName -Force
    }
    [IO.Directory]::Delete($fullPath, $false)
}

function Remove-TransactionArtifacts {
    $allowed = @(
        'transaction.json',
        'transaction.json.tmp',
        "$MainName.previous",
        'todolist-mcp.exe.previous'
    )
    Remove-KnownDirectory $transactionRoot $installRoot '.todolist-install-transaction' $allowed
}

function Invoke-TestFault([string]$Point) {
    if ($env:TODOLIST_INSTALLER_TEST_DEBUG -eq '1' -and
        [string]::Equals($env:TODOLIST_INSTALLER_TEST_FAULT, $Point, [StringComparison]::Ordinal)) {
        throw "Injected installer test fault: $Point"
    }
}

function Get-ProcessPath($Process) {
    try { return $Process.MainModule.FileName } catch { return $null }
}

function Get-ProcessesAtPaths([string[]]$Paths) {
    $wanted = @($Paths | Where-Object { $_ } | ForEach-Object { [IO.Path]::GetFullPath($_) })
    $matches = @()
    foreach ($process in [Diagnostics.Process]::GetProcesses()) {
        $path = Get-ProcessPath $process
        if ($path -and ($wanted | Where-Object { Same-Path $_ $path })) { $matches += $process }
        else { $process.Dispose() }
    }
    return $matches
}

function Stop-TargetProcesses([string[]]$Paths) {
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $matches = @(Get-ProcessesAtPaths $Paths)
        if ($matches.Count -eq 0) { return }
        foreach ($process in $matches) {
            try {
                if ($process.MainWindowHandle -ne 0) {
                    $null = $process.CloseMainWindow()
                    if ($process.WaitForExit(750)) { continue }
                }
                $pathBeforeKill = Get-ProcessPath $process
                if ($pathBeforeKill -and ($Paths | Where-Object { Same-Path $_ $pathBeforeKill })) {
                    Stop-Process -Id $process.Id -Force -ErrorAction Stop
                }
            } finally {
                $process.Dispose()
            }
        }
        Start-Sleep -Milliseconds 250
    }
    $remaining = @(Get-ProcessesAtPaths $Paths)
    foreach ($process in $remaining) { $process.Dispose() }
    if ($remaining.Count -gt 0) {
        throw '无法停止目标安装目录中的 TodoList 进程。请稍后重试。'
    }
}

function Assert-Replaceable([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { return }
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        $stream = $null
        try {
            $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            return
        } catch {
            if ($attempt -eq 39) {
                throw "目标文件仍被占用，无法安全更新：$(Split-Path -Leaf $Path)"
            }
            Start-Sleep -Milliseconds 250
        } finally {
            if ($stream) { $stream.Dispose() }
        }
    }
}

function Get-TargetPaths {
    $paths = @(
        (Join-Path $installRoot $MainName),
        (Join-Path $installRoot 'todolist-desktop.exe'),
        (Join-Path $installRoot 'todolist-mcp.exe')
    )
    return @($paths | Select-Object -Unique)
}

function Get-McpBuild([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'TodoList MCP 文件不存在。' }
    $output = @(& $Path --print-build-identity 2>&1)
    if ($LASTEXITCODE -ne 0) { throw 'TodoList MCP 构建标识读取失败。' }
    return ($output -join "`n").Trim()
}

function Assert-ComponentBuild([string]$MainPath, [string]$McpPath) {
    if (-not (Test-Path -LiteralPath $MainPath -PathType Leaf)) { throw 'TodoList 主程序文件不存在。' }
    $productVersion = [Diagnostics.FileVersionInfo]::GetVersionInfo($MainPath).ProductVersion
    if (-not [string]::Equals($productVersion, $Version, [StringComparison]::Ordinal)) {
        throw "TodoList 主程序版本校验失败，期望 $Version。"
    }
    $mcpBuild = Get-McpBuild $McpPath
    if ($mcpBuild -ne $ExpectedBuild) {
        throw "TodoList MCP 构建校验失败，期望 $ExpectedBuild，实际 $mcpBuild。"
    }
}

function Read-Transaction {
    if (-not (Test-Path -LiteralPath $transactionFile -PathType Leaf)) { return $null }
    $transaction = Get-Content -LiteralPath $transactionFile -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($transaction.owner -ne $owner -or -not (Same-Path $transaction.installDir $installRoot)) {
        throw '发现无法确认归属的安装事务，已停止更新。'
    }
    return $transaction
}

function Invoke-RollbackInternal {
    $transaction = Read-Transaction
    if (-not $transaction) {
        Remove-Item -LiteralPath $installLock -Force -ErrorAction SilentlyContinue
        return $false
    }
    if ($transaction.state -eq 'committed') {
        Remove-Item -LiteralPath $installLock -Force -ErrorAction SilentlyContinue
        Remove-TransactionArtifacts
        return $false
    }
    $hadPreviousVersion = @($transaction.files | Where-Object { $_.existed }).Count -gt 0
    Stop-TargetProcesses (Get-TargetPaths)
    for ($index = 0; $index -lt $transaction.files.Count; $index++) {
        $file = $transaction.files[$index]
        if ($file.name -notin @($MainName, 'todolist-mcp.exe')) {
            throw '安装事务包含未知组件，无法自动恢复。'
        }
        $destination = Join-Path $installRoot $file.name
        $expectedBackup = Join-Path $transactionRoot "$($file.name).previous"
        if (-not (Same-Path $file.backup $expectedBackup)) {
            throw '安装事务的备份路径无法确认，无法自动恢复。'
        }
        $destinationExists = Test-Path -LiteralPath $destination -PathType Leaf
        $backupExists = Test-Path -LiteralPath $file.backup -PathType Leaf
        if ($backupExists) {
            if (-not $file.existed) { throw '安装事务包含意外备份，已停止自动恢复。' }
            Assert-Replaceable $file.backup
            Assert-Replaceable $destination
            if ($destinationExists) { Remove-Item -LiteralPath $destination -Force }
            Move-Item -LiteralPath $file.backup -Destination $destination -Force
        } elseif ($file.existed) {
            if (-not $destinationExists) {
                throw "旧组件及其备份均不存在，无法宣称已恢复：$($file.name)"
            }
            # A missing backup with the original still present means Prepare
            # stopped before this file was moved, or a prior rollback restored it.
        } elseif ($destinationExists) {
            Assert-Replaceable $destination
            Remove-Item -LiteralPath $destination -Force
        }
        $transaction.files[$index].moved = $false
        Write-JsonAtomic $transactionFile $transaction
    }
    Remove-TransactionArtifacts
    Remove-Item -LiteralPath $installLock -Force -ErrorAction SilentlyContinue
    return $hadPreviousVersion
}

function Get-UpdateStatePath {
    if ([string]::IsNullOrWhiteSpace($InstallerPath)) { return $null }
    $installer = [IO.Path]::GetFullPath($InstallerPath)
    $installerExists = Test-Path -LiteralPath $installer -PathType Leaf
    $directory = Split-Path -Parent $installer
    $statePath = Join-Path $directory '.todolist-update-state.json'
    $expectedAttemptId = Split-Path -Leaf $directory
    $expectedInstallerFile = "$ProductName-$Version-installer.exe"
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $expectedPrefix = "$ProductName-$Version-updater-"
    if (-not (Same-Path (Split-Path -Parent $directory) $tempRoot)) { return $null }
    if (-not $expectedAttemptId.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) { return $null }
    if (-not [string]::Equals((Split-Path -Leaf $installer), $expectedInstallerFile, [StringComparison]::OrdinalIgnoreCase)) { return $null }
    try { $null = Assert-PlainDirectChild $directory $tempRoot $expectedAttemptId } catch { return $null }
    if (Test-Path -LiteralPath $statePath -PathType Leaf) {
        try {
            $state = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
            if ($state.owner -eq $cacheOwner -and
                $state.attemptId -eq $expectedAttemptId -and
                $state.version -eq $Version -and
                $state.installerFile -eq $expectedInstallerFile -and
                (Same-Path $state.installDir $installRoot) -and
                (Same-Path (Join-Path $directory $state.installerFile) $installer) -and
                (($installerExists -and $state.installerSha256 -eq (Get-Sha256 $installer)) -or
                 (-not $installerExists -and $state.state -eq 'installed'))) {
                return $statePath
            }
        } catch { return $null }
        return $null
    }
    if ($AutoUpdateFlag -notin @('1', 'legacy')) { return $null }
    if (-not $installerExists) { return $null }
    $state = [ordered]@{
        owner = $cacheOwner
        attemptId = $expectedAttemptId
        version = $Version
        installerFile = $expectedInstallerFile
        installerSha256 = Get-Sha256 $installer
        installDir = $installRoot
        state = 'installing'
        reason = ''
        explorerOpened = $false
        createdAt = [DateTime]::UtcNow.ToString('o')
        updatedAt = [DateTime]::UtcNow.ToString('o')
    }
    Write-JsonAtomic $statePath $state
    return $statePath
}

function Set-UpdateState([string]$NextState, [string]$Reason, [bool]$OpenExplorerOnce) {
    $statePath = Get-UpdateStatePath
    if (-not $statePath) { return $null }
    $record = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
    $record.state = $NextState
    $record.reason = $Reason
    $record.updatedAt = [DateTime]::UtcNow.ToString('o')
    if ($NextState -eq 'installing') { $record.explorerOpened = $false }
    $shouldOpen = $OpenExplorerOnce -and -not $record.explorerOpened -and (Test-Path -LiteralPath $InstallerPath -PathType Leaf)
    if ($shouldOpen) { $record.explorerOpened = $true }
    Write-JsonAtomic $statePath $record
    if ($shouldOpen -and -not $SuppressExplorer) {
        if ($env:TODOLIST_INSTALLER_TEST_DEBUG -eq '1' -and
            -not [string]::IsNullOrWhiteSpace($env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG)) {
            [IO.File]::AppendAllText($env:TODOLIST_INSTALLER_TEST_EXPLORER_LOG, "$InstallerPath`r`n", [Text.UTF8Encoding]::new($false))
        } else {
            Start-Process -FilePath (Join-Path $env:WINDIR 'explorer.exe') -ArgumentList "/select,`"$InstallerPath`""
        }
    }
    return $statePath
}

function Invoke-Prepare {
    # Reset one-time failure presentation as soon as a retained installer is
    # actually retried, including failures before any old component is moved.
    $null = Set-UpdateState 'installing' '' $false
    if (-not (Test-Path -LiteralPath $StagedMain -PathType Leaf) -or -not (Test-Path -LiteralPath $StagedMcp -PathType Leaf)) {
        throw '安装器内的 TodoList 组件不完整。'
    }
    Assert-ComponentBuild $StagedMain $StagedMcp
    [IO.Directory]::CreateDirectory($installRoot) | Out-Null
    if (Test-Path -LiteralPath $transactionFile -PathType Leaf) {
        $existingTransaction = Read-Transaction
        if ($existingTransaction.state -eq 'committed') {
            Remove-Item -LiteralPath $installLock -Force -ErrorAction SilentlyContinue
            Remove-TransactionArtifacts
        } else {
            $null = Invoke-RollbackInternal
        }
    }
    [IO.Directory]::CreateDirectory($transactionRoot) | Out-Null
    $files = @()
    foreach ($name in @($MainName, 'todolist-mcp.exe')) {
        $source = Join-Path $installRoot $name
        $backup = Join-Path $transactionRoot "$name.previous"
        $files += [ordered]@{ name = $name; existed = (Test-Path -LiteralPath $source -PathType Leaf); backup = $backup; moved = $false }
    }
    $transaction = [ordered]@{
        owner = $owner
        installDir = $installRoot
        version = $Version
        expectedBuild = $ExpectedBuild
        state = 'preparing'
        files = $files
    }
    Write-JsonAtomic $transactionFile $transaction
    Write-JsonAtomic $installLock ([ordered]@{ owner = $owner; version = $Version; createdAt = [DateTime]::UtcNow.ToString('o') })
    Invoke-TestFault 'before-first-backup'
    $targetPaths = Get-TargetPaths
    Stop-TargetProcesses $targetPaths
    for ($index = 0; $index -lt $transaction.files.Count; $index++) {
        $file = $transaction.files[$index]
        $source = Join-Path $installRoot $file.name
        if (-not $file.existed) { continue }
        $moved = $false
        for ($attempt = 0; $attempt -lt 40 -and -not $moved; $attempt++) {
            Stop-TargetProcesses $targetPaths
            Assert-Replaceable $source
            try {
                Move-Item -LiteralPath $source -Destination $file.backup -Force
                $moved = $true
            } catch {
                Start-Sleep -Milliseconds 250
            }
        }
        if (-not $moved) { throw "无法安全暂存旧组件：$($file.name)" }
        if ($index -eq 0) { Invoke-TestFault 'after-first-move-before-state' }
        $transaction['files'][$index]['moved'] = $true
        Write-JsonAtomic $transactionFile $transaction
        if ($index -eq 0) { Invoke-TestFault 'between-component-moves' }
    }
    Stop-TargetProcesses $targetPaths
    $transaction['state'] = 'prepared'
    Write-JsonAtomic $transactionFile $transaction
}

function Invoke-Verify {
    $transaction = Read-Transaction
    if (-not $transaction -or $transaction.state -ne 'prepared') { throw '安装事务状态无效，无法验证组件。' }
    Assert-ComponentBuild (Join-Path $installRoot $MainName) (Join-Path $installRoot 'todolist-mcp.exe')
    $transaction.state = 'verified'
    Write-JsonAtomic $transactionFile $transaction
}

function Remove-CleanupHelperDirectory {
    if ([string]::IsNullOrWhiteSpace($PSCommandPath)) { return }
    $scriptPath = [IO.Path]::GetFullPath($PSCommandPath)
    if (-not [string]::Equals((Split-Path -Leaf $scriptPath), 'cleanup.ps1', [StringComparison]::OrdinalIgnoreCase)) { return }
    $directory = Split-Path -Parent $scriptPath
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $leaf = Split-Path -Leaf $directory
    if (-not $leaf.StartsWith('TodoList-update-cleanup-', [StringComparison]::OrdinalIgnoreCase)) { return }
    Remove-KnownDirectory $directory $tempRoot $leaf @('cleanup.ps1')
    Write-TestTrace "helper-removed|root=$directory"
}

function Remove-OwnedUpdateCache([string]$StatePath) {
    $updateRoot = Split-Path -Parent $StatePath
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $leaf = Split-Path -Leaf $updateRoot
    $allowed = @((Split-Path -Leaf $InstallerPath), '.todolist-update-state.json', '.todolist-update-state.json.tmp')
    $fullRoot = Assert-PlainDirectChild $updateRoot $tempRoot $leaf
    $entries = @(Get-ChildItem -LiteralPath $fullRoot -Force)
    foreach ($entry in $entries) {
        if ($entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            -not ($allowed | Where-Object { [string]::Equals($_, $entry.Name, [StringComparison]::OrdinalIgnoreCase) })) {
            throw "更新缓存包含无法确认归属的内容，已保留：$($entry.FullName)"
        }
        Assert-Replaceable $entry.FullName
    }
    if (Test-Path -LiteralPath $InstallerPath -PathType Leaf) {
        Remove-Item -LiteralPath $InstallerPath -Force
        Invoke-TestFault 'cleanup-after-installer-delete'
    }
    $stateTemporary = "$StatePath.tmp"
    if (Test-Path -LiteralPath $stateTemporary -PathType Leaf) { Remove-Item -LiteralPath $stateTemporary -Force }
    if (Test-Path -LiteralPath $StatePath -PathType Leaf) { Remove-Item -LiteralPath $StatePath -Force }
    [IO.Directory]::Delete($fullRoot, $false)
}

function Invoke-Cleanup {
    $statePath = Get-UpdateStatePath
    if (-not $statePath) { return }
    $updateRoot = Split-Path -Parent $statePath
    try {
        if ($InstallerPid -gt 0) {
            try { Wait-Process -Id $InstallerPid -Timeout 300 -ErrorAction Stop } catch {
                $running = Get-Process -Id $InstallerPid -ErrorAction SilentlyContinue
                if ($running) { throw '等待安装器退出超时，更新包暂未清理。' }
            }
        }
        Assert-ComponentBuild (Join-Path $installRoot $MainName) (Join-Path $installRoot 'todolist-mcp.exe')
        $state = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($state.state -ne 'installed') { throw '安装器未记录成功状态，更新包暂未清理。' }
        Invoke-TestFault 'cleanup-delete'
        Remove-OwnedUpdateCache $statePath
        Write-TestTrace "cache-removed|root=$updateRoot|installerPid=$InstallerPid"
    } catch {
        $null = Set-UpdateState 'installed' "更新已安装；专属缓存清理暂未完成，将在下次启动时重试：$($_.Exception.Message)" $false
    } finally {
        try { Remove-CleanupHelperDirectory } catch {}
    }
}

function Invoke-Commit {
    $transaction = Read-Transaction
    if (-not $transaction -or $transaction.state -ne 'verified') { throw '组件尚未通过完整性验证。' }
    Assert-ComponentBuild (Join-Path $installRoot $MainName) (Join-Path $installRoot 'todolist-mcp.exe')

    # Once both installed components pass verification, the new installation is
    # authoritative. Later cache/backup housekeeping must never turn it into an
    # installation failure or trigger rollback.
    $transaction.state = 'committed'
    Write-JsonAtomic $transactionFile $transaction
    Remove-Item -LiteralPath $installLock -Force -ErrorAction SilentlyContinue
    $cleanupErrors = @()
    $statePath = $null
    try { $statePath = Set-UpdateState 'installed' '' $false } catch { $cleanupErrors += $_.Exception.Message }
    try { Remove-TransactionArtifacts } catch { $cleanupErrors += $_.Exception.Message }
    if ($statePath) {
        $cleanupRoot = $null
        try {
            Invoke-TestFault 'cleanup-launch'
            $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
            $cleanupLeaf = 'TodoList-update-cleanup-' + [Guid]::NewGuid().ToString('N')
            $cleanupRoot = Join-Path $tempRoot $cleanupLeaf
            [IO.Directory]::CreateDirectory($cleanupRoot) | Out-Null
            $null = Assert-PlainDirectChild $cleanupRoot $tempRoot $cleanupLeaf
            $cleanupScript = Join-Path $cleanupRoot 'cleanup.ps1'
            Copy-Item -LiteralPath $PSCommandPath -Destination $cleanupScript
            $invoke = "& '" + $cleanupScript.Replace("'", "''") + "' -Mode Cleanup -InstallDir '" + $installRoot.Replace("'", "''") + "' -MainName '" + $MainName.Replace("'", "''") + "' -Version '" + $Version.Replace("'", "''") + "' -ExpectedBuild '" + $ExpectedBuild.Replace("'", "''") + "' -ProductName '" + $ProductName.Replace("'", "''") + "' -BundleId '" + $BundleId.Replace("'", "''") + "' -InstallerPath '" + $InstallerPath.Replace("'", "''") + "' -AutoUpdateFlag 'owned' -InstallerPid " + $InstallerPid
            $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke))
            Start-Process -FilePath (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') -WindowStyle Hidden -ArgumentList "-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -EncodedCommand $encoded"
            Write-TestTrace "helper-started|root=$cleanupRoot|installerPid=$InstallerPid"
            $cleanupRoot = $null
        } catch {
            $cleanupErrors += $_.Exception.Message
            if ($cleanupRoot) {
                try {
                    $cleanupLeaf = Split-Path -Leaf $cleanupRoot
                    Remove-KnownDirectory $cleanupRoot ([IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)) $cleanupLeaf @('cleanup.ps1')
                } catch {}
            }
        }
    }
    if ($cleanupErrors.Count -gt 0 -and $statePath) {
        try {
            $null = Set-UpdateState 'installed' ("更新已安装；后台清理暂未完成，将在下次启动时重试：" + ($cleanupErrors -join ' ')) $false
        } catch {
            Write-ErrorDetail ("更新已安装，但无法记录后台清理状态：" + $_.Exception.Message)
        }
    } elseif ($cleanupErrors.Count -gt 0) {
        Write-ErrorDetail ("更新已安装，但后台清理状态不可用：" + ($cleanupErrors -join ' '))
    }
}

Write-TestTrace "mode=$Mode|processPid=$PID|installerPid=$InstallerPid"
try {
    switch ($Mode) {
        'Prepare' { Invoke-Prepare }
        'Verify' { Invoke-Verify }
        'Commit' { Invoke-Commit }
        'Rollback' { $null = Invoke-RollbackInternal }
        'Fail' {
            $reason = if ($ErrorFile -and (Test-Path -LiteralPath $ErrorFile -PathType Leaf)) { (Get-Content -LiteralPath $ErrorFile -Raw -Encoding UTF8).Trim() } else { '安装程序未能完成文件写入。' }
            try {
                $restoredPreviousVersion = Invoke-RollbackInternal
                $reason += if ($restoredPreviousVersion) { ' 原版本已恢复。' } else { ' 现有版本未被更改。' }
            } catch {
                $reason += " 原版本自动恢复未完成：$($_.Exception.Message)"
            }
            $null = Set-UpdateState 'failed' $reason $true
        }
        'Cancel' {
            try {
                $null = Invoke-RollbackInternal
            } catch {
                $reason = "取消安装后的原版本恢复未完成：$($_.Exception.Message)"
                $null = Set-UpdateState 'failed' $reason $true
                throw
            }
            $null = Set-UpdateState 'cancelled' '用户取消了安装；更新包已保留，可稍后重试。' $false
        }
        'StopOnly' {
            $paths = Get-TargetPaths
            Stop-TargetProcesses $paths
            foreach ($path in $paths) { Assert-Replaceable $path }
        }
        'Cleanup' { Invoke-Cleanup }
    }
} catch {
    $primaryError = $_
    try {
        if ($Mode -in @('Prepare', 'Verify', 'Commit')) { $null = Invoke-RollbackInternal }
    } catch {}
    $detail = $primaryError.Exception.Message
    if ($env:TODOLIST_INSTALLER_TEST_DEBUG) { $detail += "`n" + $primaryError.ScriptStackTrace }
    Write-ErrorDetail $detail
    [Console]::Error.WriteLine($primaryError.Exception.Message)
    exit 1
}

exit 0
