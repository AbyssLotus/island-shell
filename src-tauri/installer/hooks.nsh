; Installer hooks for Island Shell (Tauri runs these before it checks for a running copy).
;
; Island Shell hides the Windows taskbar while it runs and puts it back when it quits. If the
; installer killed it, the taskbar would stay hidden, so a running copy is asked to quit first.
; --quit reaches whichever copy is running, wherever it was started from, through the
; single-instance channel, and does nothing when none is.

!macro NSIS_HOOK_PREINSTALL
  ; The new program, unpacked to the installer's temp folder just to send --quit.
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=island-quit.exe "${MAINBINARYSRCPATH}"
  ExecWait '"$PLUGINSDIR\island-quit.exe" --quit'
  Sleep 1500
  Delete "$PLUGINSDIR\island-quit.exe"
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\island-shell.exe" 0 island_done
    ExecWait '"$INSTDIR\island-shell.exe" --quit'
    Sleep 1500
    ; Belt and braces: put the taskbar back from the setting saved at first start.
    ExecWait '"$INSTDIR\island-shell.exe" --restore-taskbar'
  island_done:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; A way back to the taskbar if Island Shell ever can't put it back itself (a crash, a forced close).
  CreateShortcut "$SMPROGRAMS\Restore the taskbar (Island Shell).lnk" "$INSTDIR\island-shell.exe" "--restore-taskbar" "$INSTDIR\island-shell.exe" 0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$SMPROGRAMS\Restore the taskbar (Island Shell).lnk"
!macroend
