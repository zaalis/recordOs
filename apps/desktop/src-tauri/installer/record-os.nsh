; The product keeps its binary and app identifier so existing device trust survives.
; The bootstrapper passes the existing install directory explicitly during upgrades.
!macro NSIS_HOOK_PREINSTALL
  ; Abort rather than silently terminating an active session, including a portable instance.
  nsis_tauri_utils::FindProcessCurrentUser "${MAINBINARYNAME}.exe"
  Pop $0
  ${If} $0 = 0
    Abort "Fermez Record OS sur le PC avant de poursuivre l’installation."
  ${EndIf}
  ; Also support launching the NSIS maintenance payload directly.
  StrCmp $INSTDIR "$LOCALAPPDATA\Record OS" 0 record_os_path_done
    ReadRegStr $0 HKCU "Software\vision-pro\Vision-Pro" ""
    StrCmp $0 "" record_os_path_done
    IfFileExists "$0\vision-pro.exe" 0 record_os_path_done
      StrCpy $INSTDIR $0
      SetOutPath $INSTDIR
  record_os_path_done:
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Remove legacy presentation only after the replacement executable is in place.
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vision-Pro" "InstallLocation"
  StrCmp $0 '$\"$INSTDIR$\"' 0 record_os_migration_done
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Vision-Pro"
    DeleteRegKey HKCU "Software\vision-pro\Vision-Pro"
    Delete "$DESKTOP\Vision-Pro.lnk"
    Delete "$SMPROGRAMS\Vision-Pro.lnk"
  record_os_migration_done:
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Vision-Pro"
  StrCmp $0 '$\"$INSTDIR\vision-pro.exe$\"' record_os_startup_migrate
  StrCmp $0 "$INSTDIR\vision-pro.exe" record_os_startup_migrate record_os_startup_done
  record_os_startup_migrate:
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Record OS" $0
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Vision-Pro"
  record_os_startup_done:
!macroend
