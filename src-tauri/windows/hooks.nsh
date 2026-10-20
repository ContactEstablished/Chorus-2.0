; Chorus 2.0 installer hooks (P14). Included by Tauri's installer.nsi; the four supported macros are
; NSIS_HOOK_PREINSTALL, POSTINSTALL, PREUNINSTALL and POSTUNINSTALL.

; Tauri's installer remembers where Chorus was installed in
; HKCU\Software\<manufacturer>\<product name> (it is read back as the default folder on the next
; install), and its uninstaller only removes that key when "delete app data" is ticked. S8 found the
; key left behind after a normal uninstall, so remove it here. It is this product's key only:
; Chorus 1 lives under a different product name, so its own key and data are never touched.
; Skipped during an update (/UPDATE): the updater runs this uninstaller in place and the installer
; writes the key again straight after.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
    ; And the manufacturer's key, if nothing else of ours is under it.
    DeleteRegKey /ifempty SHCTX "${MANUKEY}"
  ${EndIf}
!macroend
