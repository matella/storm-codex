; NSIS installer hooks of the Storm Codex desktop app (bundle.windows.nsis.installerHooks).
;
; Data on uninstall: the Tauri NSIS template already asks it. Its uninstaller confirm page has a
; "Delete the application data" checkbox, unchecked by default, and removes
; $LOCALAPPDATA\${BUNDLEID} (= %LOCALAPPDATA%\com.matella.stormcodex, the data root) only when it
; is ticked and the uninstaller is not in update mode (/UPDATE, used by upgrades). A silent
; uninstall (/S) shows no page, so the box stays unticked: data is kept. Nothing here deletes data.
;
; What these hooks add: the bundled Postgres can outlive the shell (hard kill, crash). Its
; binaries live in $INSTDIR\postgres and stay locked while it runs, so an upgrade could not
; replace them and an uninstall could not remove them. Before either, stop the cluster of this
; user's data root with the bundled pg_ctl (fast shutdown: committed data is safe). pg_ctl only
; signals the postmaster recorded in that cluster's own postmaster.pid; when none runs it fails
; and the failure is ignored.
;
; Order: the template inserts these hooks BEFORE its own "app is running" check, so the hook asks
; first. In an interactive install/uninstall (not silent, not /P passive, not /UPDATE), if the app
; is running, the hook stops with a message asking the user to Quit Storm Codex (its tray Quit
; stops the server, then Postgres, cleanly) and run the installer again; nothing is stopped. An
; update started by the app (/P /UPDATE) skips the question: the app has already stopped its stack
; and is exiting, and the template closes it if it is still there.

!include LogicLib.nsh

!macro STORM_CODEX_STOP_POSTGRES
  SetShellVarContext current
  Push $0
  Push $1
  ${IfNot} ${Silent}
  ${AndIf} $PassiveMode <> 1
  ${AndIf} $UpdateMode <> 1
    ; One CSV row per matching process (it starts with a quote); otherwise an INFO line.
    nsExec::ExecToStack '"$SYSDIR\tasklist.exe" /FI "IMAGENAME eq ${MAINBINARYNAME}.exe" /NH /FO CSV'
    Pop $0 ; exit code
    Pop $1 ; output
    StrCpy $1 $1 1
    ${If} $1 == '"'
      MessageBox MB_OK|MB_ICONEXCLAMATION "Storm Codex is running. Quit it first (tray icon, then Quit) so it stops its database cleanly, then run this again."
      Pop $1
      Pop $0
      Abort
    ${EndIf}
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\postgres\bin\pg_ctl.exe"
  ${AndIf} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}\pg\postmaster.pid"
    ; pg_ctl reads its arguments (and finds its own folder) in the ANSI code page: a profile
    ; name outside it (e.g. Cyrillic on a Western Windows) must be passed as 8.3 short paths.
    ; Without 8.3 names the long path is used (and fails for such names; the app's own
    ; recovery at the next start still handles the cluster).
    Push $2
    GetFullPathName /SHORT $0 "$INSTDIR\postgres\bin"
    ${If} $0 == ""
      StrCpy $0 "$INSTDIR\postgres\bin"
    ${EndIf}
    GetFullPathName /SHORT $2 "$LOCALAPPDATA\${BUNDLEID}\pg"
    ${If} $2 == ""
      StrCpy $2 "$LOCALAPPDATA\${BUNDLEID}\pg"
    ${EndIf}
    nsExec::Exec '"$0\pg_ctl.exe" stop -D "$2" -m fast -w -t 30'
    Pop $0 ; exit code, ignored
    Pop $2
  ${EndIf}
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro STORM_CODEX_STOP_POSTGRES
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro STORM_CODEX_STOP_POSTGRES
!macroend
