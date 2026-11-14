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
; and the failure is ignored. The template inserts PREINSTALL before its own "app is running"
; check: installing over a running app stops its database first; the app is closed right after.

!include LogicLib.nsh

!macro STORM_CODEX_STOP_POSTGRES
  SetShellVarContext current
  ${If} ${FileExists} "$INSTDIR\postgres\bin\pg_ctl.exe"
  ${AndIf} ${FileExists} "$LOCALAPPDATA\${BUNDLEID}\pg\postmaster.pid"
    Push $0
    nsExec::Exec '"$INSTDIR\postgres\bin\pg_ctl.exe" stop -D "$LOCALAPPDATA\${BUNDLEID}\pg" -m fast -w -t 30'
    Pop $0 ; exit code, ignored
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro STORM_CODEX_STOP_POSTGRES
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro STORM_CODEX_STOP_POSTGRES
!macroend
