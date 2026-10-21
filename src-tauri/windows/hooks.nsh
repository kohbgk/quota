; Quota installer hooks (included by Tauri's NSIS template).

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove Quota's Claude Code status line bridge (only if Quota installed it).
  ExecWait '"$INSTDIR\quota.exe" --quota-cleanup'
  ; Remove the "launch at startup" entry.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Quota"
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Quota"
!macroend
