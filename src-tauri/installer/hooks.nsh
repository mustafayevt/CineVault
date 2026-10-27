; Installer hooks for the NSIS setup (bundle.windows.nsis.installerHooks).
;
; The setup's own switches (Tauri's NSIS template): /S silent, /P passive (progress window only,
; no pages), /R start the app when the install finished (with /S or /P), /UPDATE update mode (no
; uninstall of the old version, no new shortcuts), /NS no shortcuts, /D=<folder> (last) the
; folder. "Update now" in the app runs the setup with /P /R /UPDATE /D=<its folder> and quits.

; Update mode: the app that started the setup is quitting. Wait for its executable to be free
; (up to 90 s) before the template's running-app check, which would otherwise end the app by
; force in passive mode, before it has paused its jobs and closed the library.
!macro NSIS_HOOK_PREINSTALL
  ${If} $UpdateMode = 1
    StrCpy $R9 0
    cv_wait_for_app:
    ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
      ClearErrors
      ; A running program cannot be opened for writing.
      FileOpen $R8 "$INSTDIR\${MAINBINARYNAME}.exe" a
      ${If} ${Errors}
        ${If} $R9 = 0
          DetailPrint "Waiting for ${PRODUCTNAME} to close..."
        ${EndIf}
        IntOp $R9 $R9 + 1
        ${If} $R9 < 450
          Sleep 200
          Goto cv_wait_for_app
        ${EndIf}
      ${Else}
        FileClose $R8
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; An update replaces cinevault-app.exe in place, and Explorer keeps the icon it cached for the
; pinned taskbar and Start shortcuts, which then show a blank page. Telling the shell that file
; associations and icons changed makes it draw them again.
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

; Reinstalling over an existing copy with the wizard runs the old uninstaller first, in place
; (`uninstall.exe _?=<folder>`), and the template's uninstaller then unpins and deletes the Start
; menu and desktop shortcuts, so the pinned icon is lost. A real uninstall runs from a copy in
; the temp folder instead, so an uninstaller running from the install folder is a reinstall:
; treat it as an update, which keeps the shortcuts, pins and app data.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $EXEPATH == "$INSTDIR\uninstall.exe"
    StrCpy $UpdateMode 1
  ${EndIf}
!macroend
