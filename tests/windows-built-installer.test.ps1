param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath
)

$ErrorActionPreference = 'Stop'
$installer = [IO.Path]::GetFullPath($InstallerPath)
$projectRoot = Split-Path -Parent $PSScriptRoot
$appVersion = [string]((Get-Content -LiteralPath (Join-Path $projectRoot 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json).version)
$expectedProductionBuild = "todolist/$appVersion/production"
$systemPowerShell = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('todolist-built-installer-' + [Guid]::NewGuid().ToString('N'))
$otherProcess = $null
$targetMainProcess = $null
$targetMcpProcess = $null
$originalInstallerDebug = $env:TODOLIST_INSTALLER_TEST_DEBUG
$originalInstallerErrorPath = $env:TODOLIST_INSTALLER_TEST_ERROR_PATH

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
}

try {
    Assert-True (Test-Path -LiteralPath $installer -PathType Leaf) "installer not found: $installer"
    Assert-True (Test-Path -LiteralPath $systemPowerShell -PathType Leaf) 'Windows PowerShell fixture is unavailable'

    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
    Assert-True ($resolvedTestRoot.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) 'test root escaped the Windows temp directory'

    $installRoot = Join-Path $testRoot 'install'
    $otherRoot = Join-Path $testRoot 'other-channel'
    $installerError = Join-Path $testRoot 'installer-error.txt'
    $uninstallRegistry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TodoList Installer Acceptance'
    $desktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) 'TodoList Installer Acceptance.lnk'
    $startMenuShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'TodoList Installer Acceptance.lnk'
    New-Item -ItemType Directory -Force -Path $installRoot, $otherRoot | Out-Null
    $env:TODOLIST_INSTALLER_TEST_DEBUG = '1'
    $env:TODOLIST_INSTALLER_TEST_ERROR_PATH = $installerError
    $targetMain = Join-Path $installRoot 'todolist-desktop.exe'
    $targetMcp = Join-Path $installRoot 'todolist-mcp.exe'
    $otherMcp = Join-Path $otherRoot 'todolist-mcp.exe'
    $unrelatedFile = Join-Path $installRoot 'unrelated-user-file.keep'
    Copy-Item -LiteralPath $systemPowerShell -Destination $targetMain
    Copy-Item -LiteralPath $systemPowerShell -Destination $targetMcp
    Copy-Item -LiteralPath $systemPowerShell -Destination $otherMcp
    [IO.File]::WriteAllText($unrelatedFile, 'preserve me', [Text.UTF8Encoding]::new($false))

    $holderArguments = '-NoLogo -NoProfile -NonInteractive -Command "Start-Sleep -Seconds 180"'
    $targetMainProcess = Start-Process -FilePath $targetMain -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    $targetMcpProcess = Start-Process -FilePath $targetMcp -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    $otherProcess = Start-Process -FilePath $otherMcp -ArgumentList $holderArguments -WindowStyle Hidden -PassThru
    Start-Sleep -Milliseconds 800

    $installProcess = Start-Process -FilePath $installer -ArgumentList @('/S', "/D=$installRoot") -Wait -PassThru
    $installerDetail = if (Test-Path -LiteralPath $installerError -PathType Leaf) { (Get-Content -LiteralPath $installerError -Raw -Encoding UTF8).Trim() } else { 'no coordinator error was captured' }
    Assert-True ($installProcess.ExitCode -eq 0) "installer exited with $($installProcess.ExitCode): $installerDetail"
    $targetMainProcess.Refresh(); $targetMcpProcess.Refresh(); $otherProcess.Refresh()
    Assert-True $targetMainProcess.HasExited 'installer did not stop the target main process'
    Assert-True $targetMcpProcess.HasExited 'installer did not stop the target MCP process'
    Assert-True (-not $otherProcess.HasExited) 'installer stopped the same-named MCP in another directory'
    Assert-True (Test-Path -LiteralPath $targetMain -PathType Leaf) 'installed desktop executable is missing'
    Assert-True (Test-Path -LiteralPath $targetMcp -PathType Leaf) 'installed MCP executable is missing'
    Assert-True (Test-Path -LiteralPath $unrelatedFile -PathType Leaf) 'installer removed an unrelated file'
    Assert-True (([Diagnostics.FileVersionInfo]::GetVersionInfo($targetMain).ProductVersion) -eq $appVersion) 'installed desktop version is incorrect'
    $identity = (& $targetMcp '--print-build-identity' 2>&1 | Out-String).Trim()
    Assert-True ($LASTEXITCODE -eq 0) 'installed MCP identity probe failed'
    Assert-True ($identity -eq $expectedProductionBuild) "installed MCP identity is incorrect: $identity"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $installRoot '.todolist-installing.json'))) 'install lock survived the installer exit'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $installRoot '.todolist-install-transaction'))) 'install transaction survived the installer exit'
    Assert-True (Test-Path -LiteralPath $uninstallRegistry) 'isolated uninstall registry entry is missing'

    $uninstaller = Join-Path $installRoot 'uninstall.exe'
    Assert-True (Test-Path -LiteralPath $uninstaller -PathType Leaf) 'uninstaller is missing'
    $uninstallProcess = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    Assert-True ($uninstallProcess.ExitCode -eq 0) "uninstaller exited with $($uninstallProcess.ExitCode)"
    for ($attempt = 0; $attempt -lt 40 -and (Test-Path -LiteralPath $uninstaller); $attempt++) { Start-Sleep -Milliseconds 250 }
    Assert-True (-not (Test-Path -LiteralPath $targetMain)) 'desktop executable survived uninstall'
    Assert-True (-not (Test-Path -LiteralPath $targetMcp)) 'MCP executable survived uninstall'
    Assert-True (Test-Path -LiteralPath $unrelatedFile -PathType Leaf) 'uninstaller removed an unrelated file'
    Assert-True (-not $otherProcess.HasExited) 'other-channel MCP did not survive uninstall'
    Assert-True (-not (Test-Path -LiteralPath $uninstallRegistry)) 'isolated uninstall registry entry survived uninstall'
    Assert-True (-not (Test-Path -LiteralPath $desktopShortcut)) 'isolated desktop shortcut survived uninstall'
    Assert-True (-not (Test-Path -LiteralPath $startMenuShortcut)) 'isolated start-menu shortcut survived uninstall'

    Write-Output 'Built NSIS installer acceptance passed: real silent install, exact-path process coordination, matching production components, clean uninstall.'
} finally {
    $env:TODOLIST_INSTALLER_TEST_DEBUG = $originalInstallerDebug
    $env:TODOLIST_INSTALLER_TEST_ERROR_PATH = $originalInstallerErrorPath
    foreach ($process in @($targetMainProcess, $targetMcpProcess, $otherProcess)) {
        if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
    }
    Start-Sleep -Milliseconds 300
    if (Test-Path -LiteralPath $testRoot) {
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
        $resolvedTestRoot = [IO.Path]::GetFullPath($testRoot)
        if ($resolvedTestRoot.StartsWith($tempRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $testRoot -Recurse -Force
        }
    }
}
