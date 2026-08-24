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
; four-part numeric version for the exe's file properties; CI passes the real
; one, since VERSION itself may be a tag name like "v1.2.3" or "dev"
!ifndef VERSION_NUM
  !define VERSION_NUM "0.0.0.0"
!endif

; nsis resolves relative paths against the script's directory, not the cwd
!define ROOT       ".."

!define APPNAME    "Kanae"
!define APPEXE     "kanae.exe"
!define REGKEY     "Software\Kanae"
!define UNINSTREG  "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kanae"
!define APPPATHS   "Software\Microsoft\Windows\CurrentVersion\App Paths\${APPEXE}"
!define PROGID     "Kanae.Audio"
!define AUMID      "Kanae.Player"  ; matches setup_windows_app_identity() in src/main.rs

; mirrors AUDIO_EXTENSIONS in src/file_player.rs
!macro ForEachAudioExt MAC
  !insertmacro ${MAC} ".mp3"
  !insertmacro ${MAC} ".mp2"
  !insertmacro ${MAC} ".mp1"
  !insertmacro ${MAC} ".flac"
  !insertmacro ${MAC} ".ogg"
  !insertmacro ${MAC} ".opus"
  !insertmacro ${MAC} ".m4a"
  !insertmacro ${MAC} ".mp4"
  !insertmacro ${MAC} ".aac"
  !insertmacro ${MAC} ".alac"
  !insertmacro ${MAC} ".wav"
  !insertmacro ${MAC} ".aiff"
  !insertmacro ${MAC} ".aif"
  !insertmacro ${MAC} ".caf"
  !insertmacro ${MAC} ".mka"
  !insertmacro ${MAC} ".wma"
  !insertmacro ${MAC} ".ape"
!macroend

; offer Kanae for this type without stealing the existing default
!macro RegisterExt EXT
  WriteRegStr HKLM "Software\Classes\${EXT}\OpenWithProgIds" "${PROGID}" ""
  WriteRegStr HKLM "Software\Classes\Applications\${APPEXE}\SupportedTypes" "${EXT}" ""
  WriteRegStr HKLM "${REGKEY}\Capabilities\FileAssociations" "${EXT}" "${PROGID}"
!macroend

!macro UnregisterExt EXT
  DeleteRegValue HKLM "Software\Classes\${EXT}\OpenWithProgIds" "${PROGID}"
  DeleteRegKey /ifempty HKLM "Software\Classes\${EXT}\OpenWithProgIds"
!macroend

; palette, mirrored from the qml theme block in qml/main.qml
!define CLR_BG      "0f0f0f"  ; clrBg
!define CLR_SURFACE "161616"  ; clrSurface
!define CLR_SURF2   "1e1e1e"  ; clrSurf2
!define CLR_BORDER  "282828"  ; clrBorder
!define CLR_TEXT    "dfdfdf"  ; clrText
!define CLR_TEXT2   "686868"  ; clrText2
!define CLR_ACCENT  "bfbfbf"  ; clrAccent

Name    "${APPNAME}"
OutFile "${ROOT}\kanae-${VARIANT}-windows-x64-${VERSION}.exe"
Unicode True

InstallDir      "$PROGRAMFILES64\${APPNAME}"
InstallDirRegKey HKLM "${REGKEY}" "InstallDir"
RequestExecutionLevel admin
SetCompressor   /SOLID lzma
BrandingText    "${APPNAME} ${VERSION}"
ShowInstDetails   show
ShowUninstDetails show

VIProductVersion "${VERSION_NUM}"
VIAddVersionKey  "ProductName"     "${APPNAME}"
VIAddVersionKey  "ProductVersion"  "${VERSION}"
VIAddVersionKey  "FileVersion"     "${VERSION}"
VIAddVersionKey  "FileDescription" "${APPNAME} installer (${VARIANT})"
VIAddVersionKey  "CompanyName"     "${APPNAME}"
VIAddVersionKey  "LegalCopyright"  ""

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "WinMessages.nsh"

; MUI paints the header and the welcome/finish pages with these
!define MUI_BGCOLOR                   "${CLR_BG}"
!define MUI_TEXTCOLOR                 "${CLR_TEXT}"
!define MUI_INSTFILESPAGE_COLORS      "${CLR_TEXT2} ${CLR_BG}"
!define MUI_INSTFILESPAGE_PROGRESSBAR "colored"

!define MUI_ABORTWARNING
!define MUI_ABORTWARNING_TEXT       "Quit the ${APPNAME} installer?"

!define MUI_WELCOMEPAGE_TITLE       "${APPNAME}"
!define MUI_WELCOMEPAGE_TEXT        "A music player.$\r$\n$\r$\nThis will install ${APPNAME} ${VERSION} (${VARIANT}) on your computer. Any previous install in the same folder is replaced; your library, settings and caches are kept.$\r$\n$\r$\nClose ${APPNAME} before continuing."

!define MUI_DIRECTORYPAGE_TEXT_TOP  "${APPNAME} will be installed in the folder below. Choose another with Browse, then click Install."
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "Install folder"

!define MUI_FINISHPAGE_TITLE        "${APPNAME} is ready"
!define MUI_FINISHPAGE_TEXT         "${APPNAME} has been installed on your computer."
!define MUI_FINISHPAGE_RUN          "$INSTDIR\${APPEXE}"
!define MUI_FINISHPAGE_RUN_TEXT     "Launch ${APPNAME}"
!define MUI_FINISHPAGE_LINK         "github.com/chwair/kanae"
!define MUI_FINISHPAGE_LINK_LOCATION "https://github.com/chwair/kanae"
!define MUI_FINISHPAGE_NOREBOOTSUPPORT

; the directory and instfiles pages are plain win32 dialogs, so MUI's colors
; don't reach them; recolor their controls as each one is shown
!define MUI_PAGE_CUSTOMFUNCTION_SHOW DirectoryPageShow
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!define MUI_PAGE_CUSTOMFUNCTION_SHOW InstFilesPageShow
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP  "${APPNAME} will be removed from the folder below. Your library, settings and caches are left alone."
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.DirectoryPageShow
!insertmacro MUI_UNPAGE_CONFIRM
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.InstFilesPageShow
!insertmacro MUI_UNPAGE_INSTFILES

!define MUI_CUSTOMFUNCTION_GUIINIT   DarkFrame
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.DarkFrame

!insertmacro MUI_LANGUAGE "English"

; ── Theme ────────────────────────────────────────────────────────────────────
; $R0 holds the inner dialog. Statics are drawn transparent so they sit on the
; dialog's own background; push buttons stay native, Windows themes those.
!macro _DarkStatic ID
  GetDlgItem $R1 $R0 ${ID}
  SetCtlColors $R1 "${CLR_TEXT}" transparent
!macroend

!macro _DarkPage TYPE
Function ${TYPE}DirectoryPageShow
  FindWindow $R0 "#32770" "" $HWNDPARENT
  SetCtlColors $R0 "${CLR_TEXT}" "${CLR_BG}"
  !insertmacro _DarkStatic 1006  ; blurb
  !insertmacro _DarkStatic 1020  ; "Install folder" group box
  !insertmacro _DarkStatic 1023  ; space required
  !insertmacro _DarkStatic 1024  ; space available
  GetDlgItem $R1 $R0 1019        ; the path edit box
  SetCtlColors $R1 "${CLR_TEXT}" "${CLR_SURFACE}"
FunctionEnd

Function ${TYPE}InstFilesPageShow
  FindWindow $R0 "#32770" "" $HWNDPARENT
  SetCtlColors $R0 "${CLR_TEXT}" "${CLR_BG}"
  !insertmacro _DarkStatic 1004  ; current-file status line
FunctionEnd
!macroend

!insertmacro _DarkPage ""
!insertmacro _DarkPage "un."

; the outer frame: dialog background plus the branding line along the bottom
!macro _DarkFrame
  SetCtlColors $HWNDPARENT "${CLR_TEXT}" "${CLR_BG}"
  GetDlgItem $R0 $HWNDPARENT 1256
  SetCtlColors $R0 "${CLR_TEXT2}" "${CLR_BG}"
!macroend

Function DarkFrame
  !insertmacro _DarkFrame
FunctionEnd

Function un.DarkFrame
  !insertmacro _DarkFrame
FunctionEnd

; ── System PATH (hybrid only) ────────────────────────────────────────────────
; The hybrid build runs as a TUI when started from a terminal, so the install
; dir goes on the machine PATH to make "kanae" work from cmd/PowerShell. The
; GUI-only build has nothing to offer a shell, so it relies on App Paths alone.
!if "${VARIANT}" == "hybrid"

!include "WordFunc.nsh"
!insertmacro WordAdd
!insertmacro un.WordAdd

!define ENVREG "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"

; $0 = current PATH, $1 = new PATH, $2 = length of current PATH.
; NSIS strings are capped at 1024 chars, so a longer PATH comes back from
; ReadRegStr truncated; writing that back would eat the tail of it.
!macro _PathTooLong
  StrLen $2 $0
  IntCmp $2 1000 0 +3 0
    DetailPrint "System PATH is too long to edit safely; skipping PATH update."
    Return
!macroend

; nothing to do when the entry is already there (reinstall) or already gone
!macro _CommitPath MSG
  StrCmp $1 $0 +4
    DetailPrint "${MSG}"
    WriteRegExpandStr HKLM "${ENVREG}" "Path" "$1"
    ; tell already-running shells and Explorer to reread the environment
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

Function AddToSystemPath
  ReadRegStr $0 HKLM "${ENVREG}" "Path"
  !insertmacro _PathTooLong
  ${WordAdd} "$0" ";" "+$INSTDIR" $1
  !insertmacro _CommitPath "Adding $INSTDIR to the system PATH"
FunctionEnd

Function un.RemoveFromSystemPath
  ReadRegStr $0 HKLM "${ENVREG}" "Path"
  !insertmacro _PathTooLong
  ${un.WordAdd} "$0" ";" "-$INSTDIR" $1
  !insertmacro _CommitPath "Removing $INSTDIR from the system PATH"
FunctionEnd

!endif

; ── Install ──────────────────────────────────────────────────────────────────
Section "Kanae" SecMain
  SectionIn RO  ; required section

  ; Upgrade path: clear out any previous install first so stale Qt DLLs and
  ; QML plugins from an older version can't be loaded by the new binary.
  ; User data is untouched (Kanae keeps caches/config under %APPDATA%).
  IfFileExists "$INSTDIR\${APPEXE}" 0 +2
    RMDir /r "$INSTDIR"

  SetOutPath "$INSTDIR"
  File /r "${ROOT}\dist\*"

  ; Registry: install path + Add/Remove Programs entry
  WriteRegStr   HKLM "${REGKEY}"     "InstallDir"          "$INSTDIR"
  WriteRegStr   HKLM "${UNINSTREG}"  "DisplayName"         "${APPNAME}"
  WriteRegStr   HKLM "${UNINSTREG}"  "DisplayVersion"      "${VERSION}"
  WriteRegStr   HKLM "${UNINSTREG}"  "DisplayIcon"         "$INSTDIR\${APPEXE}"
  WriteRegStr   HKLM "${UNINSTREG}"  "Publisher"           "Kanae"
  WriteRegStr   HKLM "${UNINSTREG}"  "URLInfoAbout"        "https://github.com/chwair/kanae"
  WriteRegStr   HKLM "${UNINSTREG}"  "UninstallString"     '"$INSTDIR\Uninstall.exe"'
  WriteRegStr   HKLM "${UNINSTREG}"  "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegStr   HKLM "${UNINSTREG}"  "InstallLocation"     "$INSTDIR"
  WriteRegDWORD HKLM "${UNINSTREG}"  "NoModify"            1
  WriteRegDWORD HKLM "${UNINSTREG}"  "NoRepair"            1

  ; App Paths: lets "kanae" work from Win+R and the shell's path lookup
  WriteRegStr HKLM "${APPPATHS}" ""     "$INSTDIR\${APPEXE}"
  WriteRegStr HKLM "${APPPATHS}" "Path" "$INSTDIR"

  ; PATH: lets "kanae" work from cmd/PowerShell, where the hybrid build's TUI is
!if "${VARIANT}" == "hybrid"
  Call AddToSystemPath
!endif

  ; The file type Kanae hands to Windows, and the verb used to launch it.
  WriteRegStr HKLM "Software\Classes\${PROGID}" ""                 "Audio File"
  WriteRegStr HKLM "Software\Classes\${PROGID}" "FriendlyTypeName" "Audio File"
  WriteRegStr HKLM "Software\Classes\${PROGID}" "AppUserModelID"   "${AUMID}"
  WriteRegStr HKLM "Software\Classes\${PROGID}\DefaultIcon"        "" "$INSTDIR\${APPEXE},0"
  WriteRegStr HKLM "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${APPEXE}" "%1"'

  ; The application itself, which is what puts Kanae in the "Open with" list.
  WriteRegStr HKLM "Software\Classes\Applications\${APPEXE}" "FriendlyAppName" "${APPNAME}"
  WriteRegStr HKLM "Software\Classes\Applications\${APPEXE}" "AppUserModelID"  "${AUMID}"
  WriteRegStr HKLM "Software\Classes\Applications\${APPEXE}\DefaultIcon"        "" "$INSTDIR\${APPEXE},0"
  WriteRegStr HKLM "Software\Classes\Applications\${APPEXE}\shell\open\command" "" '"$INSTDIR\${APPEXE}" "%1"'

  ; Capabilities + RegisteredApplications is what Settings > Default apps reads.
  WriteRegStr HKLM "${REGKEY}\Capabilities" "ApplicationName"        "${APPNAME}"
  WriteRegStr HKLM "${REGKEY}\Capabilities" "ApplicationDescription" "A music player."
  WriteRegStr HKLM "${REGKEY}\Capabilities" "ApplicationIcon"        "$INSTDIR\${APPEXE},0"
  WriteRegStr HKLM "Software\RegisteredApplications" "${APPNAME}" "${REGKEY}\Capabilities"

  !insertmacro ForEachAudioExt RegisterExt

  ; tell the shell the association data changed, so it picks this up without a
  ; sign-out (SHCNE_ASSOCCHANGED, SHCNF_IDLIST)
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'

  ; Installed size, shown in Add/Remove Programs.
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKLM "${UNINSTREG}" "EstimatedSize" $0

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

!if "${VARIANT}" == "hybrid"
  Call un.RemoveFromSystemPath
!endif

  RMDir /r "$INSTDIR"

  !insertmacro ForEachAudioExt UnregisterExt
  DeleteRegValue HKLM "Software\RegisteredApplications" "${APPNAME}"
  DeleteRegKey HKLM "Software\Classes\${PROGID}"
  DeleteRegKey HKLM "Software\Classes\Applications\${APPEXE}"
  DeleteRegKey HKLM "${APPPATHS}"
  DeleteRegKey HKLM "${UNINSTREG}"
  DeleteRegKey HKLM "${REGKEY}"

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
SectionEnd
