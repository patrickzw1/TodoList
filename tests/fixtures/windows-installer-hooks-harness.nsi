Unicode true
SilentInstall silent
AutoCloseWindow true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!include LogicLib.nsh
!include FileFunc.nsh
!include x64.nsh

!ifndef HARNESS_OUTFILE
  !error "HARNESS_OUTFILE is required"
!endif
!ifndef APP_VERSION
  !error "APP_VERSION is required"
!endif

!define PRODUCTNAME "TodoList"
!define VERSION "${APP_VERSION}"
!define BUNDLEID "app.todolist.desktop.installer-hooks-acceptance"
!define MAINBINARYNAME "todolist-desktop"
!define MAINBINARYSRCPATH "${__FILEDIR__}\..\..\target\installer-acceptance\release\todolist-desktop.exe"

!include "${__FILEDIR__}\..\..\src-tauri\windows\installer-hooks.nsh"

Var UpdateMode

Name "TodoList Installer Hooks Acceptance"
OutFile "${HARNESS_OUTFILE}"
InstallDir "$TEMP\TodoList-installer-hooks-placeholder"
ShowInstDetails nevershow

Section "Hook transaction" SEC_HOOK_TRANSACTION
  StrCpy $UpdateMode 1
  !insertmacro NSIS_HOOK_PREINSTALL

  SetOutPath "$INSTDIR"
  SetOverwrite on
  File "/oname=todolist-desktop.exe" "${MAINBINARYSRCPATH}"
  File "/oname=todolist-mcp.exe" "${TODOLIST_HOOK_DIR}\..\binaries\todolist-mcp-x86_64-pc-windows-msvc.exe"

  !insertmacro NSIS_HOOK_POSTINSTALL
SectionEnd

Function .onInstSuccess
  !insertmacro NSIS_HOOK_INSTSUCCESS
FunctionEnd

Function .onInstFailed
  !insertmacro TODOLIST_HOOK_INSTFAILED
FunctionEnd
