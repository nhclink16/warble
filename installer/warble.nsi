; Warble's Windows installer: one file to send a friend. Build it with NSIS 3 (makensis), on
; Windows or Linux.
;
; On Windows, from the repo, after `cargo build --release -p warble`:
;   makensis installer\warble.nsi
; With a warble.exe built somewhere else:
;   makensis -DEXE=/path/to/warble.exe installer/warble.nsi
; Either way it writes target/Warble-Setup-<version>.exe (or -DOUT=...).
;
; It installs for the current user only, with no admin prompt, into
; %LOCALAPPDATA%\Programs\Warble: the folder scripts/install.ps1 uses, so it updates either kind
; of install in place. Settings and sounds live in %APPDATA%\Warble; neither installing nor
; uninstalling touches them. welcome.bmp is the app icon on the app's graphite body.

Unicode true
SetCompressor /SOLID lzma
SetCompressorDictSize 32
RequestExecutionLevel user
ManifestDPIAware true
ManifestSupportedOS all

!ifndef VERSION
  !searchparse /file "../Cargo.toml" 'version = "' VERSION '"'
!endif
!ifndef EXE
  !define EXE "../target/release/warble.exe"
!endif
!ifndef OUT
  !define OUT "../target/Warble-Setup-${VERSION}.exe"
!endif
!define ICO "../crates/app/assets/warble.ico"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Warble"
!define ABOUT "Voice changer and soundboard for Discord and game chat"

Name "Warble"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\Warble"
BrandingText "Warble ${VERSION}"
ShowInstDetails hide
ShowUninstDetails hide

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "Warble"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Warble Setup"
VIAddVersionKey "LegalCopyright" ""

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "${ICO}"
!define MUI_UNICON "${ICO}"
!define MUI_WELCOMEFINISHPAGE_BITMAP "welcome.bmp"

!define MUI_WELCOMEPAGE_TEXT "Warble changes your voice and plays sounds into Discord and game chat.$\r$\n$\r$\nIt needs VB-CABLE, a free virtual audio cable. If you don't have it yet, Warble shows you where to get it.$\r$\n$\r$\nClick Install to put Warble on this PC."
!define MUI_PAGE_CUSTOMFUNCTION_SHOW WelcomeShow
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_TEXT "Warble is installed. You'll find it in the Start menu."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Open Warble"
!define MUI_FINISHPAGE_RUN_FUNCTION OpenWarble
!define MUI_FINISHPAGE_SHOWREADME
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Add a desktop shortcut"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION DesktopShortcut
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP "Warble will be removed from this PC. Your sounds and settings are kept, so they come back if you install Warble again."
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Close this user's Warble, so its exe can be replaced or deleted. Warble hides to the tray when
; its window closes, so it's ended outright; it saves its settings as soon as they change.
!macro CLOSE_WARBLE un
Function ${un}CloseWarble
  ReadEnvStr $4 USERNAME
  ; Exits 0 while this user has a warble.exe running.
  StrCpy $1 '"$SYSDIR\cmd.exe" /c ""$SYSDIR\tasklist.exe" /nh /fi "imagename eq warble.exe" /fi "username eq $4" | "$SYSDIR\find.exe" /i "warble.exe""'
  nsExec::ExecToStack $1
  Pop $0
  Pop $2
  ${If} $0 != 0
    Return
  ${EndIf}
  MessageBox MB_OKCANCEL|MB_ICONINFORMATION "Warble is open. Click OK to close it and carry on." /SD IDOK IDOK close
  Quit
  close:
  nsExec::ExecToStack '"$SYSDIR\taskkill.exe" /f /im warble.exe /fi "username eq $4"'
  Pop $0
  Pop $2
  ; Wait up to 5 s for it to go, so its exe is free.
  StrCpy $3 0
  ${Do}
    Sleep 250
    nsExec::ExecToStack $1
    Pop $0
    Pop $2
    IntOp $3 $3 + 1
    ${If} $0 != 0
    ${OrIf} $3 >= 20
      ${ExitDo}
    ${EndIf}
  ${Loop}
FunctionEnd
!macroend
!insertmacro CLOSE_WARBLE ""
!insertmacro CLOSE_WARBLE "un."

Function WelcomeShow
  GetDlgItem $0 $HWNDPARENT 1
  SendMessage $0 ${WM_SETTEXT} 0 "STR:Install"
FunctionEnd

Function OpenWarble
  SetOutPath "$INSTDIR"
  Exec '"$INSTDIR\warble.exe"'
FunctionEnd

Function DesktopShortcut
  CreateShortcut "$DESKTOP\Warble.lnk" "$INSTDIR\warble.exe" "" "$INSTDIR\warble.ico" 0 SW_SHOWNORMAL "" "${ABOUT}"
FunctionEnd

Section "Warble"
  Call CloseWarble
  SetOutPath "$INSTDIR"
  File "/oname=warble.exe" "${EXE}"
  File "/oname=warble.ico" "${ICO}"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start search, PowerToys Run and Raycast find apps by their Start menu shortcut.
  CreateShortcut "$SMPROGRAMS\Warble.lnk" "$INSTDIR\warble.exe" "" "$INSTDIR\warble.ico" 0 SW_SHOWNORMAL "" "${ABOUT}"

  ; Settings > Apps lists it, with Uninstall.
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Warble"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\warble.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" $0
SectionEnd

Section "Uninstall"
  Call un.CloseWarble
  Delete "$INSTDIR\warble.exe"
  Delete "$INSTDIR\warble.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Warble.lnk"
  Delete "$DESKTOP\Warble.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
