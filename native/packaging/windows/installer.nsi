; installer.nsi: the per-user Windows installer of mcbench-lite (NSIS 3, Unicode).
; Built by installer.sh, which lists the payload folder into payload.nsh and runs
;   makensis -DVERSION=<x.y.z> -DPAYLOAD=<folder> -DPAYLOAD_NSH=<file>
;            -DSIZE_KB=<n> -DOUTFILE=<file> installer.nsi
;
; What it does, in order (docs/native-app.md, Packaging and distribution):
;   1. .onInit: /SOURCE=winget makes install-source "winget"; no /SOURCE makes it
;      "direct"; any other /SOURCE value stops.
;   2. The app is running (an executable of the install, or the updater's .old
;      copy of one, cannot be opened for writing): stop, change nothing.
;   3. An earlier install that was interrupted while moving files left
;      .install.old: every file in it whose place in the install folder is
;      empty is moved back; then .install.old and .install.tmp are deleted.
;   4. Every new file (the payload, install-source, uninstall.exe) is written
;      into $INSTDIR\.install.tmp. Nothing of an existing install is touched
;      before all of them are written.
;   5. Every existing file of those names is renamed into $INSTDIR\.install.old,
;      then every new file is renamed from .install.tmp into the install
;      folder. A failed rename moves everything back and stops.
;   6. The updater's leftovers are deleted, then .install.old.
;   7. The Start menu shortcut and the uninstall entry under HKCU are written.
;
; Exit codes: 0 installed; 2 stopped by this script with a sentence (shown in a
; message box, or in silent mode written to the installer's standard error
; when the caller gave it one, for example Start-Process -RedirectStandardError);
; 1 cancelled by the person.

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!macro REQUIRE DEFINE
  !ifndef ${DEFINE}
    !error "installer.nsi is built by installer.sh, which defines ${DEFINE} (and VERSION, PAYLOAD, PAYLOAD_NSH, SIZE_KB, OUTFILE)"
  !endif
!macroend
!insertmacro REQUIRE VERSION
!insertmacro REQUIRE PAYLOAD
!insertmacro REQUIRE PAYLOAD_NSH
!insertmacro REQUIRE SIZE_KB
!insertmacro REQUIRE OUTFILE

!define NAME "mcbench-lite"
!define PUBLISHER "adonis-singh"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}"
!define ICON "${__FILEDIR__}/../icon/mcbench-lite.ico"
; The folders of steps 3 to 5, inside the install folder (one volume, so
; every move is a rename).
!define TMP_DIR ".install.tmp"
!define OLD_DIR ".install.old"

Name "${NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${NAME}"
BrandingText "${NAME} ${VERSION}"
ShowInstDetails nevershow
ShowUninstDetails nevershow
AllowSkipFiles off

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${NAME} ${VERSION} setup (x64)"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "MIT License"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
; PAYLOAD_FILE_LIST: one "!insertmacro ${MACRO} <name>" per payload file,
; written by installer.sh from the payload folder.
!include "${PAYLOAD_NSH}"

!define MUI_ICON "${ICON}"
!define MUI_UNICON "${ICON}"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!define MUI_FINISHPAGE_TITLE "${NAME} was removed"
!define MUI_FINISHPAGE_TEXT "Your chats, worlds and settings are kept in $LOCALAPPDATA\${NAME}. To delete them too, delete that folder, for example with this command in a Command Prompt:$\r$\n$\r$\nrmdir /s /q $\"%LOCALAPPDATA%\${NAME}$\"$\r$\n$\r$\nTwo things of yours were left as they are: Claude Code's transcripts of the app's chats, in the folder under $PROFILE\.claude\projects whose name ends in -cache-mcbench-claude-cwd, and your Codex sign-in, $PROFILE\.codex\auth.json."
!insertmacro MUI_UNPAGE_FINISH
!insertmacro MUI_LANGUAGE "English"

Var Source

; Stops with SENTENCE and exit code 2: a message box, or in silent mode the
; installer's standard error when it has one (an NSIS installer is a GUI
; program and has no console of its own).
!macro STOP_WITH SENTENCE
  ${If} ${Silent}
    System::Call 'kernel32::GetStdHandle(i -12) p.r9'
    ${If} $9 P<> 0
    ${AndIf} $9 P<> -1
      FileWrite $9 "${SENTENCE}$\r$\n"
    ${EndIf}
  ${Else}
    MessageBox MB_OK|MB_ICONSTOP "${SENTENCE}"
  ${EndIf}
  SetErrorLevel 2
  Quit
!macroend

; The app is running when one of its executables, or the updater's .old copy
; of one (the app runs from it between a swap and its restart), exists and
; cannot be opened for writing: Windows refuses to open a running image for
; writing. Opening for append writes nothing and changes no time.
!macro RUNNING_CHECK_FILE FILE
  !insertmacro RUNNING_CHECK_ONE "$INSTDIR\${FILE}"
  !insertmacro RUNNING_CHECK_ONE "$INSTDIR\${FILE}.old"
!macroend
!macro RUNNING_CHECK_ONE PATH
  ${If} ${FileExists} "${PATH}"
    ClearErrors
    FileOpen $8 "${PATH}" a
    ${If} ${Errors}
      StrCpy $7 "${PATH}"
    ${Else}
      FileClose $8
    ${EndIf}
  ${EndIf}
!macroend
; Leaves in $7 the path of a running executable, or "" when none runs.
!macro RUNNING_CHECK
  StrCpy $7 ""
  !insertmacro PAYLOAD_FILE_LIST RUNNING_CHECK_FILE
!macroend

; What the updater (native/crates/updater/src/swap.rs and flow.rs) leaves
; inside the install folder: <entry>.old for each top-level payload entry an
; update replaced (a file or a folder), removed by the app at its next start.
!macro DELETE_UPDATER_LEFTOVERS FILE
  Delete "$INSTDIR\${FILE}.old"
  RMDir /r "$INSTDIR\${FILE}.old"
!macroend

; What the updater leaves beside the install folder, in %LOCALAPPDATA%\Programs:
; every file and folder whose name starts with ".mcbench-lite." (a dot, the
; install folder's name, a dot). The prefix is the app's own: the updater names
; everything it keeps beside the payload that way (native/crates/updater:
; swap.rs's beside, Installed::updater_files: the waiting download, its feed
; entry and partial download, the unpack folder, the held download, the lock
; file and the gate). A list of names is not used because that list grows with
; the updater's releases, and an uninstaller of an older release must still
; remove what a newer app made. The install folder itself, "mcbench-lite", has
; no leading dot and does not match. The running app holds the lock file open,
; so the running check above refuses first.
!macro DELETE_UPDATER_BESIDE
  ${GetParent} "$INSTDIR" $0
  StrCpy $4 ".${NAME}."
  StrLen $5 $4
  FindFirst $2 $1 "$0\.${NAME}.*"
  ${DoWhile} $1 != ""
    ; Windows' * also matches ".mcbench-lite" with no second dot: the name must
    ; start with the whole prefix and be longer than it.
    StrCpy $3 $1 $5
    StrLen $6 $1
    ${If} $3 == $4
    ${AndIf} $6 > $5
      ${If} ${FileExists} "$0\$1\*.*"
        RMDir /r "$0\$1"
      ${Else}
        Delete "$0\$1"
      ${EndIf}
    ${EndIf}
    FindNext $2 $1
  ${Loop}
  FindClose $2
!macroend

; Step 3 and the rollback of step 5, per file with an old one aside: whatever
; is in its place (a new file that was moved in) goes back to .install.tmp, and
; the old one comes back from .install.old.
!macro RESTORE_FILE FILE
  ${If} ${FileExists} "$INSTDIR\${OLD_DIR}\${FILE}"
    ${If} ${FileExists} "$INSTDIR\${FILE}"
      Rename "$INSTDIR\${FILE}" "$INSTDIR\${TMP_DIR}\${FILE}"
    ${EndIf}
    Rename "$INSTDIR\${OLD_DIR}\${FILE}" "$INSTDIR\${FILE}"
  ${EndIf}
!macroend

; The rollback of step 5 after a failed move-in, per file with no old one aside:
; it was new to the install folder and was moved in, so it goes back to
; .install.tmp. (After a failed aside no new file was moved in, and a file
; without an old one aside is an old one that was not yet moved.)
!macro REMOVE_NEW_FILE FILE
  ${If} ${FileExists} "$INSTDIR\${FILE}"
  ${AndIfNot} ${FileExists} "$INSTDIR\${OLD_DIR}\${FILE}"
    Rename "$INSTDIR\${FILE}" "$INSTDIR\${TMP_DIR}\${FILE}"
  ${EndIf}
!macroend

; Step 5, the first half: an existing file goes aside.
!macro ASIDE_FILE FILE
  ${If} ${FileExists} "$INSTDIR\${FILE}"
    ClearErrors
    Rename "$INSTDIR\${FILE}" "$INSTDIR\${OLD_DIR}\${FILE}"
    ${If} ${Errors}
      StrCpy $6 "$INSTDIR\${FILE}"
      Goto move_failed
    ${EndIf}
  ${EndIf}
!macroend

; Step 5, the second half: the new file moves in.
!macro MOVE_IN_FILE FILE
  ClearErrors
  Rename "$INSTDIR\${TMP_DIR}\${FILE}" "$INSTDIR\${FILE}"
  ${If} ${Errors}
    StrCpy $6 "$INSTDIR\${FILE}"
    Goto move_failed
  ${EndIf}
!macroend

; The payload's files and the two the installer adds.
!macro ALL_FILES MACRO
  !insertmacro PAYLOAD_FILE_LIST ${MACRO}
  !insertmacro ${MACRO} "install-source"
  !insertmacro ${MACRO} "uninstall.exe"
!macroend

Function .onInit
  ${GetParameters} $0
  ClearErrors
  ${GetOptions} $0 "/SOURCE=" $1
  ${If} ${Errors}
    StrCpy $Source "direct"
  ${ElseIf} $1 == "winget"
    StrCpy $Source "winget"
  ${Else}
    !insertmacro STOP_WITH "The installer was given /SOURCE=$1, and the only value it takes is /SOURCE=winget."
  ${EndIf}
FunctionEnd

Section "Install"
  ; Step 2.
  !insertmacro RUNNING_CHECK
  ${If} $7 != ""
    !insertmacro STOP_WITH "mcbench-lite is running ($7): quit it, then run the installer again. Nothing was changed."
  ${EndIf}

  ; Step 3.
  ${If} ${FileExists} "$INSTDIR\${OLD_DIR}\*.*"
    CreateDirectory "$INSTDIR\${TMP_DIR}"
    !insertmacro ALL_FILES RESTORE_FILE
  ${EndIf}
  RMDir /r "$INSTDIR\${OLD_DIR}"
  RMDir /r "$INSTDIR\${TMP_DIR}"
  ${If} ${FileExists} "$INSTDIR\${OLD_DIR}\*.*"
  ${OrIf} ${FileExists} "$INSTDIR\${TMP_DIR}\*.*"
    !insertmacro STOP_WITH "The installer cannot delete $INSTDIR\${OLD_DIR} or $INSTDIR\${TMP_DIR}, which an earlier install left."
  ${EndIf}

  ; Step 4.
  ClearErrors
  CreateDirectory "$INSTDIR\${TMP_DIR}"
  SetOutPath "$INSTDIR\${TMP_DIR}"
  File "${PAYLOAD}/*"
  FileOpen $0 "$INSTDIR\${TMP_DIR}\install-source" w
  FileWrite $0 "$Source$\n"
  FileClose $0
  WriteUninstaller "$INSTDIR\${TMP_DIR}\uninstall.exe"
  ${If} ${Errors}
    SetOutPath "$INSTDIR"
    RMDir /r "$INSTDIR\${TMP_DIR}"
    !insertmacro STOP_WITH "The installer could not write the new files into $INSTDIR\${TMP_DIR}; the installed app was not changed."
  ${EndIf}
  ; The working folder leaves .install.tmp, so it can be renamed from.
  SetOutPath "$INSTDIR"

  ; Step 5.
  CreateDirectory "$INSTDIR\${OLD_DIR}"
  StrCpy $5 "aside"
  !insertmacro ALL_FILES ASIDE_FILE
  StrCpy $5 "move-in"
  !insertmacro ALL_FILES MOVE_IN_FILE
  Goto moved
  move_failed:
    ${If} $5 == "move-in"
      !insertmacro ALL_FILES REMOVE_NEW_FILE
    ${EndIf}
    !insertmacro ALL_FILES RESTORE_FILE
    RMDir /r "$INSTDIR\${TMP_DIR}"
    RMDir "$INSTDIR\${OLD_DIR}"
    !insertmacro STOP_WITH "The installer could not move $6 into place; the installed app was put back as it was."
  moved:

  ; Step 6.
  !insertmacro PAYLOAD_FILE_LIST DELETE_UPDATER_LEFTOVERS
  RMDir /r "$INSTDIR\${OLD_DIR}"
  RMDir "$INSTDIR\${TMP_DIR}"

  ; Step 7. No desktop shortcut.
  CreateShortcut "$SMPROGRAMS\${NAME}.lnk" "$INSTDIR\mcbench-lite.exe" "" "$INSTDIR\uninstall.exe" 0
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\uninstall.exe,0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" ${SIZE_KB}
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

; ---------------------------------------------------------------- uninstaller

!macro UN_DELETE_FILE FILE
  Delete "$INSTDIR\${FILE}"
!macroend

; What the app registers with Windows at run time (stage three's answers of
; 2026-11-17): its WSL distro mcbench-lite, imported by the app with
; wsl --import, whose disk is %LOCALAPPDATA%\mcbench-lite\sandbox\.wsl\ext4.vhdx.
; It holds nothing of the person's and the app re-imports it when needed. It is
; unregistered only when a distro of that name is registered for this user AND
; its BasePath is under %LOCALAPPDATA%\mcbench-lite\; a distro of that name
; anywhere else is not the app's and is left alone; nothing happens when WSL is
; not installed (no Lxss key). The app registers nothing else with Windows.
; Neither the installer nor the uninstaller installs, enables or checks WSL, and
; neither asks for an administrator: setting WSL up is the app's setup screen's.
Function un.UnregisterWslDistro
  StrCpy $0 0
  StrCpy $3 ""
  ${Do}
    EnumRegKey $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Lxss" $0
    ${If} $1 == ""
      ${Break}
    ${EndIf}
    ReadRegStr $2 HKCU "Software\Microsoft\Windows\CurrentVersion\Lxss\$1" "DistributionName"
    ${If} $2 == "${NAME}"
      ReadRegStr $3 HKCU "Software\Microsoft\Windows\CurrentVersion\Lxss\$1" "BasePath"
      ${Break}
    ${EndIf}
    IntOp $0 $0 + 1
  ${Loop}
  ${If} $3 == ""
    Return
  ${EndIf}
  ; WSL writes BasePath with the \\?\ prefix.
  StrCpy $4 $3 4
  ${If} $4 == "\\?\"
    StrCpy $3 $3 "" 4
  ${EndIf}
  StrCpy $5 "$LOCALAPPDATA\${NAME}\"
  StrLen $6 $5
  StrCpy $4 $3 $6
  ; StrCmp (==) ignores case, as Windows paths do.
  ${If} $4 != $5
    Return
  ${EndIf}
  ; The uninstaller is a 32-bit program: wsl.exe is in the 64-bit System32,
  ; which it reaches as Sysnative.
  ExecWait '"$WINDIR\Sysnative\wsl.exe" --unregister ${NAME}' $0
  ${If} $0 != 0
    !insertmacro STOP_WITH "wsl.exe --unregister ${NAME} failed with exit code $0, so nothing was removed; run it yourself, then the uninstaller again."
  ${EndIf}
FunctionEnd

; Deletes the folders in $TEMP that match PATTERN and whose names are LENGTH
; characters long (0: any length); what the app leaves there after a crash.
!macro UN_DELETE_TEMP PATTERN LENGTH
  FindFirst $0 $1 "$TEMP\${PATTERN}"
  ${DoWhile} $1 != ""
    StrLen $2 $1
    ${If} ${LENGTH} == 0
    ${OrIf} $2 == ${LENGTH}
      ${If} ${FileExists} "$TEMP\$1\*.*"
        RMDir /r "$TEMP\$1"
      ${EndIf}
    ${EndIf}
    FindNext $0 $1
  ${Loop}
  FindClose $0
!macroend

Section "Uninstall"
  !insertmacro RUNNING_CHECK
  ${If} $7 != ""
    !insertmacro STOP_WITH "mcbench-lite is running ($7): quit it, then run the uninstaller again. Nothing was removed."
  ${EndIf}

  Call un.UnregisterWslDistro

  ; The install folder: exactly what the installer and the updater put there.
  !insertmacro PAYLOAD_FILE_LIST DELETE_UPDATER_LEFTOVERS
  !insertmacro ALL_FILES UN_DELETE_FILE
  RMDir /r "$INSTDIR\${TMP_DIR}"
  RMDir /r "$INSTDIR\${OLD_DIR}"
  RMDir "$INSTDIR"
  !insertmacro DELETE_UPDATER_BESIDE
  ; The installer's InstallDir made %LOCALAPPDATA%\Programs and the parent of the
  ; uninstall entry when they did not exist; each goes only when it is empty.
  RMDir "$LOCALAPPDATA\Programs"

  Delete "$SMPROGRAMS\${NAME}.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey /ifempty HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall"

  ; What the app leaves outside its data folder (stage three's answers of
  ; 2026-11-17): the folder Claude Code is started in, and leftovers in %TEMP%
  ; after a crash.
  RMDir /r "$PROFILE\.cache\mcbench"
  !insertmacro UN_DELETE_TEMP "mcb-??????" 10
  !insertmacro UN_DELETE_TEMP "mcbench-codex-??????" 20
  !insertmacro UN_DELETE_TEMP "mcbench-engine-child-*" 0
  !insertmacro UN_DELETE_TEMP "mcbench-lite-logs" 0
SectionEnd
