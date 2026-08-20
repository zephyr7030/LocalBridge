; LocalBridge installer lifecycle hooks.
; Delete the current user's LocalBridge Runtime API Key before uninstalling files.
; CredDeleteW returns zero for missing/inaccessible credentials; cleanup is best-effort
; so an already-absent credential must never block uninstall.
!macro NSIS_HOOK_PREUNINSTALL
  System::Call 'advapi32::CredDeleteW(w "LocalBridge/RuntimeApiKey/runtime-api-key", i 1, i 0) i .r0'
!macroend
