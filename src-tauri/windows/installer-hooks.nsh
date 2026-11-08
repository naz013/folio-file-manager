; Folio's "Open folders with Folio" setting (src-tauri/src/default_manager) writes a verb under
; the current user's Directory and Drive classes. Uninstalling must take it out, or folders
; opened from other programs would point at a program that is gone. An update keeps it: the
; running app points it at the new program on start.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ReadRegStr $0 HKCU "Software\Classes\Directory\shell" ""
    ${If} $0 == "folio"
      DeleteRegValue HKCU "Software\Classes\Directory\shell" ""
    ${EndIf}
    DeleteRegKey HKCU "Software\Classes\Directory\shell\folio"
    ReadRegStr $0 HKCU "Software\Classes\Drive\shell" ""
    ${If} $0 == "folio"
      DeleteRegValue HKCU "Software\Classes\Drive\shell" ""
    ${EndIf}
    DeleteRegKey HKCU "Software\Classes\Drive\shell\folio"
  ${EndIf}
!macroend
