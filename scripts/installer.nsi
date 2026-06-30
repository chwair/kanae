; Kanae NSIS installer
;
; Build with (from repo root after windeployqt6 has populated dist\):
;   makensis /DVERSION=<version> /DVARIANT=<gui|hybrid> scripts\installer.nsi
;
; Produces: kanae-<variant>-windows-x64-<version>.exe

!ifndef VERSION
  !define VERSION "dev"
!endif
!ifndef VARIANT
  !define VARIANT "gui"
!endif

!define APPNAME    "Kanae"
!define APPEXE     "kanae.exe"
!define REGKEY     "Software\Kanae"
!define UNINSTREG  "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kanae"

Name    "${APPNAME}"
OutFile "kanae-${VARIANT}-windows-x64-${VERSION}.exe"
Unicode True

InstallDir      "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "${REGKEY}" "InstallDir"
RequestExecutionLevel admin
SetCompressor   /SOLID lzma

!include "MUI2.nsh"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN          "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT     "Launch ${APPNAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ── Install ──────────────────────────────────────────────────────────────────
Section "Kanae" SecMain
  SectionIn RO  ; required section

  SetOutPath "$INSTDIR"
  File /r "dist\*.*"

  ; Registry: install path + Add/Remove Programs entry
  WriteRegStr   HKLM "${REGKEY}"     "InstallDir"          "$INSTDIR"
  WriteRegStr   HKLM "${UNINSTREG}"  "DisplayName"         "${APPNAME}"
  WriteRegStr   HKLM "${UNINSTREG}"  "DisplayVersion"      "${VERSION}"
  WriteRegStr   HKLM "${UNINSTREG}"  "Publisher"           "Kanae"
  WriteRegStr   HKLM "${UNINSTREG}"  "UninstallString"     '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKLM "${UNINSTREG}"  "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKLM "${UNINSTREG}"  "NoModify"            1
  WriteRegDWORD HKLM "${UNINSTREG}"  "NoRepair"            1

  ; Shortcuts
  CreateDirectory "$SMPROGRAMS\${APPNAME}"
  CreateShortcut  "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk" "$INSTDIR\${APPEXE}"
  CreateShortcut  "$DESKTOP\${APPNAME}.lnk"               "$INSTDIR\${APPEXE}"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

; ── Uninstall ─────────────────────────────────────────────────────────────────
Section "Uninstall"
  Delete "$SMPROGRAMS\${APPNAME}\${APPNAME}.lnk"
  RMDir  "$SMPROGRAMS\${APPNAME}"
  Delete "$DESKTOP\${APPNAME}.lnk"

  RMDir /r "$INSTDIR"

  DeleteRegKey HKLM "${UNINSTREG}"
  DeleteRegKey HKLM "${REGKEY}"
SectionEnd
