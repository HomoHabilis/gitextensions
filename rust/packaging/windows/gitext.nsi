; Windows installer of the Rust port of Git Extensions (NSIS 3).
;
; Build (on Windows or Linux, after building gitext.exe):
;   makensis /DVERSION=0.1.0 /DEXE=..\..\target\release\gitext.exe gitext.nsi
; or use ../package.sh, as the release workflow does.
; It writes GitExtensions-<version>-setup.exe next to this script (or to /DOUTFILE=...).
;
; The installation is per user: no administrator rights are needed, and the Explorer
; context menu is registered for the current user (gitext shellext install).

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
; numeric X.Y.Z of VERSION (VERSION may carry a pre-release suffix, e.g. 0.2.0-beta.1)
!ifndef NUMVERSION
  !define NUMVERSION "${VERSION}"
!endif
!ifndef EXE
  !define EXE "..\..\target\release\gitext.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "GitExtensions-${VERSION}-setup.exe"
!endif

!define APPNAME "Git Extensions"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GitExtensionsRust"
!define ICONS "..\..\crates\gitext-app\res\icons"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\GitExtensions"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "${APPNAME} ${VERSION}"

VIProductVersion "${NUMVERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "FileDescription" "${APPNAME} setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GNU General Public License v3"

!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "${ICONS}\git-extensions-logo.ico"
!define MUI_UNICON "${ICONS}\git-extensions-logo.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\gitext.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "..\..\..\LICENSE.md"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_ICONSTOP "${APPNAME} needs a 64-bit version of Windows."
    Abort
  ${EndIf}
FunctionEnd

Section "${APPNAME}" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "/oname=gitext.exe" "${EXE}"
  File "/oname=LICENSE.md" "..\..\..\LICENSE.md"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  ; the next section adds the context menu again when it is selected
  ExecWait '"$INSTDIR\gitext.exe" shellext uninstall'

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "Git Extensions"
  WriteRegStr HKCU "${UNINSTKEY}" "URLInfoAbout" "https://gitextensions.github.io"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\gitext.exe,0"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1

  ; "gitext" in Run (Win+R) and in shells started from Explorer
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\gitext.exe" "" "$INSTDIR\gitext.exe"

  ; git is not bundled
  SearchPath $0 "git.exe"
  ${If} $0 == ""
    MessageBox MB_ICONINFORMATION "${APPNAME} needs Git for Windows, which was not found on the PATH.$\r$\n$\r$\nInstall it from https://git-scm.com/download/win, or set the git executable in Settings > Git."
  ${EndIf}
SectionEnd

Section "Windows Explorer context menu" SecShell
  ExecWait '"$INSTDIR\gitext.exe" shellext install' $0
  ${If} $0 != 0
    DetailPrint "Could not install the Explorer context menu (exit code $0)."
  ${EndIf}
SectionEnd

Section "Start menu shortcut" SecStartMenu
  CreateShortCut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\gitext.exe"
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\gitext.exe"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} "The application (gitext.exe)."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecShell} "Show the Git Extensions menu when right-clicking files, folders and the background of folders in Explorer. On Windows 11 it is under 'Show more options'."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecStartMenu} "Add ${APPNAME} to the Start menu."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Add a shortcut to the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  ExecWait '"$INSTDIR\gitext.exe" shellext uninstall'
  Delete "$INSTDIR\gitext.exe"
  Delete "$INSTDIR\LICENSE.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\App Paths\gitext.exe"
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd
