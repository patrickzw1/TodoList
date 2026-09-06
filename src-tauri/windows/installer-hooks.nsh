!define TODOLIST_HOOK_DIR "${__FILEDIR__}"

Var TodoListAutoUpdateFlag
Var TodoListFailureFile
Var TodoListFailureReason
Var TodoListPowerShell

!macro TodoListResolvePowerShell
  StrCpy $TodoListPowerShell "$SYSDIR\WindowsPowerShell\v1.0\powershell.exe"
  ${If} ${RunningX64}
    ; NSIS is 32-bit. Sysnative reaches the native 64-bit PowerShell so exact
    ; executable-path inspection also works for 64-bit TodoList processes.
    StrCpy $TodoListPowerShell "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
  ${EndIf}
!macroend

!macro TodoListResolveAutoUpdateFlag
  StrCpy $TodoListAutoUpdateFlag ""
  ClearErrors
  ${GetOptions} $CMDLINE "/TODOLIST_AUTO_UPDATE=" $TodoListAutoUpdateFlag
  ${If} ${Errors}
  ${AndIf} $UpdateMode = 1
    ; v0.2.0's updater cannot pass the explicit flag. The coordinator still
    ; requires the exact Tauri temp layout before treating this as owned cache.
    StrCpy $TodoListAutoUpdateFlag "legacy"
  ${EndIf}
!macroend

!macro TodoListReadFailure
  StrCpy $TodoListFailureReason "TodoList 安装未完成。"
  ${If} ${FileExists} "$TodoListFailureFile"
    FileOpen $R1 "$TodoListFailureFile" r
    FileRead $R1 $TodoListFailureReason
    FileClose $R1
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\todolist-install-coordinator.ps1" "${TODOLIST_HOOK_DIR}\install-coordinator.ps1"
  File "/oname=$PLUGINSDIR\todolist-new-main.exe" "${MAINBINARYSRCPATH}"
  File "/oname=$PLUGINSDIR\todolist-new-mcp.exe" "${TODOLIST_HOOK_DIR}\..\binaries\todolist-mcp-x86_64-pc-windows-msvc.exe"
  StrCpy $TodoListFailureFile "$PLUGINSDIR\todolist-install-error.txt"
  Delete "$TodoListFailureFile"
  !insertmacro TodoListResolveAutoUpdateFlag
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode Prepare -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -StagedMain "$PLUGINSDIR\todolist-new-main.exe" -StagedMcp "$PLUGINSDIR\todolist-new-mcp.exe" -InstallerPath "$EXEPATH" -AutoUpdateFlag "$TodoListAutoUpdateFlag" -ErrorFile "$TodoListFailureFile"' $R0
  ${If} $R0 <> 0
    !insertmacro TodoListReadFailure
    Abort "$TodoListFailureReason"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Delete "$TodoListFailureFile"
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode Verify -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -InstallerPath "$EXEPATH" -AutoUpdateFlag "$TodoListAutoUpdateFlag" -ErrorFile "$TodoListFailureFile"' $R0
  ${If} $R0 <> 0
    !insertmacro TodoListReadFailure
    Abort "$TodoListFailureReason"
  ${EndIf}
!macroend

!macro NSIS_HOOK_INSTSUCCESS
  System::Call 'kernel32::GetCurrentProcessId() i .r7'
  Delete "$TodoListFailureFile"
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode Commit -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -InstallerPath "$EXEPATH" -AutoUpdateFlag "$TodoListAutoUpdateFlag" -ErrorFile "$TodoListFailureFile" -InstallerPid $7' $R0
  ${If} $R0 <> 0
    !insertmacro TodoListReadFailure
    ; Automatic and silent updates report through the retained installer,
    ; one-time Explorer selection and Settings recovery card, not a modal box.
    DetailPrint "$TodoListFailureReason"
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro TODOLIST_HOOK_INSTFAILED
  InitPluginsDir
  File "/oname=$PLUGINSDIR\todolist-install-coordinator.ps1" "${TODOLIST_HOOK_DIR}\install-coordinator.ps1"
  StrCpy $TodoListFailureFile "$PLUGINSDIR\todolist-install-error.txt"
  !insertmacro TodoListResolveAutoUpdateFlag
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode Fail -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -InstallerPath "$EXEPATH" -AutoUpdateFlag "$TodoListAutoUpdateFlag" -ErrorFile "$TodoListFailureFile"' $R0
!macroend

!macro TODOLIST_HOOK_USERABORT
  InitPluginsDir
  File "/oname=$PLUGINSDIR\todolist-install-coordinator.ps1" "${TODOLIST_HOOK_DIR}\install-coordinator.ps1"
  StrCpy $TodoListFailureFile "$PLUGINSDIR\todolist-install-error.txt"
  !insertmacro TodoListResolveAutoUpdateFlag
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode Cancel -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -InstallerPath "$EXEPATH" -AutoUpdateFlag "$TodoListAutoUpdateFlag" -ErrorFile "$TodoListFailureFile"' $R0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  InitPluginsDir
  File "/oname=$PLUGINSDIR\todolist-install-coordinator.ps1" "${TODOLIST_HOOK_DIR}\install-coordinator.ps1"
  StrCpy $TodoListFailureFile "$PLUGINSDIR\todolist-install-error.txt"
  Delete "$TodoListFailureFile"
  !insertmacro TodoListResolvePowerShell
  ExecWait '"$TodoListPowerShell" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\todolist-install-coordinator.ps1" -Mode StopOnly -InstallDir "$INSTDIR" -MainName "${MAINBINARYNAME}.exe" -Version "${VERSION}" -ExpectedBuild "todolist/${VERSION}/production" -ProductName "${PRODUCTNAME}" -BundleId "${BUNDLEID}" -ErrorFile "$TodoListFailureFile"' $R0
  ${If} $R0 <> 0
    !insertmacro TodoListReadFailure
    Abort "$TodoListFailureReason"
  ${EndIf}
!macroend
