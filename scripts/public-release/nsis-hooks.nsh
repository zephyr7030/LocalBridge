; LocalBridge installer lifecycle hooks.
; User data is preserved by default. Interactive uninstallers ask explicitly and
; default to No; silent uninstallers require /DELETEUSERDATA=1.
!include "FileFunc.nsh"
Var LocalBridgeDeleteUserData

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $LocalBridgeDeleteUserData "0"
  IfSilent LocalBridgeSilentUninstall LocalBridgePromptDeleteUserData
LocalBridgePromptDeleteUserData:
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "同时删除已保存的 LocalBridge Runtime API Key？" IDYES LocalBridgeDeleteUserDataNow IDNO LocalBridgeKeepUserData
LocalBridgeSilentUninstall:
  ${GetOptions} $CMDLINE "/DELETEUSERDATA=" $LocalBridgeDeleteUserData
  StrCmp $LocalBridgeDeleteUserData "1" LocalBridgeDeleteUserDataNow LocalBridgeKeepUserData
LocalBridgeDeleteUserDataNow:
  System::Call 'advapi32::CredDeleteW(w "LocalBridge/RuntimeApiKey/runtime-api-key", i 1, i 0) i .r0'
LocalBridgeKeepUserData:
!macroend
