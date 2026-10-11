; Tangent — Windows installer.
;
; Built from macOS with `makensis` (brew install makensis); scripts/build-installer.sh
; passes SRCDIR, OUTFILE and VERSION on the command line.
;
; It installs the application, to $PROGRAMFILES64\Tangent.
;
; Installers before 4.77.0 could also write $COMMONFILES64\VST3\Tangent.vst3
; (C:\Program Files\Common Files\VST3, which every program that loads VST3
; scans without being told to). This installer and its uninstaller only
; remove that leftover, so nothing keeps finding it.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"

!ifndef VERSION
  !error "VERSION not defined (pass -DVERSION=x.y.z)"
!endif

; The binary version field NSIS writes into the .exe resource, which must be
; four numbers and nothing else. A pre-release version like "5.0.0-beta.2" is a
; perfectly good VERSION and an illegal VIProductVersion, so the caller passes
; the numeric part separately rather than this file trying to parse it. Falls
; back to VERSION for a plain x.y.z, which is every release before 5.0.
!ifndef NUMVERSION
  !define NUMVERSION "${VERSION}"
!endif
!ifndef SRCDIR
  !error "SRCDIR not defined"
!endif
!ifndef OUTFILE
  !error "OUTFILE not defined"
!endif

Name "Tangent ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$PROGRAMFILES64\Tangent"
InstallDirRegKey HKLM "Software\Tangent" "InstallDir"
; The destination is under Program Files, so this needs admin. Asking up
; front is better than failing halfway through with a file-copy error.
RequestExecutionLevel admin
SetCompressor /SOLID lzma

; **The uninstaller is signed here, at build time, like every other PE we
; ship.** NSIS 3.08+ generates the complete uninstaller while compiling and
; runs this command on it before packing it into the installer, so the
; Uninstall.exe that WriteUninstaller puts on the user's disk is the signed
; one. Smart App Control checks every program each time it starts and has no
; per-app override, so an unsigned uninstaller is one Windows refuses to run:
; an app that cannot be removed. `= 0` because without it makensis ignores
; the command's exit code, and a failed signing must fail the build.
; build-installer.sh passes SIGNCMD (scripts/sign-windows.sh), which leaves
; the file unsigned and says so when no signing account is configured.
!ifdef SIGNCMD
  !uninstfinalize '"${SIGNCMD}" "%1" Uninstall.exe' = 0
!endif

VIProductVersion "${NUMVERSION}.0"
VIAddVersionKey "ProductName" "Tangent"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "Ganten"
VIAddVersionKey "FileDescription" "Tangent installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT"

; **The installer and uninstaller wear the app's own logo**, not NSIS's
; default. The caller passes the icon (`-DICON=…/assets/tangent.ico`); the
; same `assets/tangent.ico` build.rs embeds into `tangent.exe`, so the setup,
; the app and the Add/Remove entry are one mark. There is no fallback to name:
; the staged payload holds no `.ico` at all, so a build without `-DICON` stops
; here and says why, rather than deep inside MUI2 over a file that never existed.
!ifndef ICON
  !error "ICON not defined (build-installer.sh passes -DICON)"
!endif
!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\tangent.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Tangent now"
!define MUI_FINISHPAGE_TEXT "Hold H in Tangent to see every keyboard shortcut."

; Source paths are resolved by makensis on the BUILD host, which here is
; macOS, so they use forward slashes. Destination paths below are Windows
; paths and keep their backslashes — the two are not the same thing and
; mixing them up produces "open failed" on a path with both.
!insertmacro MUI_PAGE_LICENSE "${SRCDIR}/LICENSE.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
; The one choice an uninstall offers: whether the settings go too. Off by
; default, and a page rather than a question box so the words can be read
; before anything is ticked. See the two uninstall sections below.
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Section "Tangent application" SEC_APP
  SectionIn 1
  SetOutPath "$INSTDIR"
  File /r "${SRCDIR}/app/*.*"

  ; The Tangent.vst3 an installer before 4.77.0 could leave in the system
  ; VST3 folder: nothing else ever removes it, so every program that scans
  ; that folder (Tangent's own scanner too) kept finding it. Upgrades remove
  ; it.
  RMDir /r "$COMMONFILES64\VST3\Tangent.vst3"

  CreateDirectory "$SMPROGRAMS\Tangent"
  CreateShortCut "$SMPROGRAMS\Tangent\Tangent.lnk" "$INSTDIR\tangent.exe"
  CreateShortCut "$SMPROGRAMS\Tangent\Uninstall Tangent.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKLM "Software\Tangent" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "DisplayName" "Tangent"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "Publisher" "Ganten"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "DisplayIcon" "$INSTDIR\tangent.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent" \
    "NoRepair" 1
SectionEnd

Section -Uninstaller
  ; Always written, so there is always a way back out. SetOutPath makes sure
  ; $INSTDIR exists before the uninstaller is written into it.
  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
SectionEnd

; **${RunningX64} is a plug-in call.** x64.nsh spells it as System::Call, so
; the installer unpacks NSIS's System.dll into $PLUGINSDIR and loads it, and
; Smart App Control judges a DLL like any program: refused, it would make this
; test fail on a 64-bit machine. build-installer.sh hands makensis signed
; copies of System.dll and nsDialogs.dll (the finish page's). A call into any
; other plug-in packs that one UNSIGNED, which check-artifacts.sh refuses: add
; it to build-installer.sh's list.
Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "Tangent needs 64-bit Windows."
    Abort
  ${EndIf}
FunctionEnd

Section "un.Tangent" SEC_UN_APP
  SectionIn RO
  RMDir /r "$COMMONFILES64\VST3\Tangent.vst3"
  Delete "$SMPROGRAMS\Tangent\Tangent.lnk"
  Delete "$SMPROGRAMS\Tangent\Uninstall Tangent.lnk"
  RMDir "$SMPROGRAMS\Tangent"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tangent"
  DeleteRegKey HKLM "Software\Tangent"
  ; Settings are deliberately NOT removed here. They live in
  ; %USERPROFILE%\.config\tangent and include taught chord names and a
  ; supporter key, none of which the user asked to throw away by uninstalling
  ; a program. The section below is the one place they go, and it is off
  ; unless ticked.
SectionEnd

; **Off by default, and its own tick.** A settings file survives an
; uninstall on purpose (above) - but a settings file that has gone wrong
; survives it too, and then re-breaks the next install "between updates",
; which is how a Windows tester met an audio system that could not open
; and could not be escaped by reinstalling. The way out is offered where a
; person looking for it would look, and it takes a deliberate tick.
Section /o "un.Also remove my settings and taught chords" SEC_UN_SETTINGS
  RMDir /r "$PROFILE\.config\tangent"
SectionEnd

!insertmacro MUI_UNFUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_UN_APP} \
    "The program and its shortcuts, and a folder older Tangent installers left in Common Files. Always removed."
  !insertmacro MUI_DESCRIPTION_TEXT ${SEC_UN_SETTINGS} \
    "Everything under %USERPROFILE%\.config\tangent: settings, taught chord names, the supporter key, logs. Leave this off to keep them for the next install."
!insertmacro MUI_UNFUNCTION_DESCRIPTION_END
