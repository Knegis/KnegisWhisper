"""Rebase our small NSIS patch explicitly when the pinned Tauri CLI changes."""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
UPSTREAM = Path(__file__).with_name("upstream.nsi")
OUTPUT = ROOT / "apps/desktop/src-tauri/installer.nsi"
SHA256 = "20f4ecc730defb71f1342eaeaec4021df13be3d843abba0effe88ea5835fa079"


def generate():
    raw = UPSTREAM.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SHA256:
        raise ValueError("Upstream NSIS template hash changed; review the rebase")
    lock = json.loads((ROOT / "apps/desktop/package-lock.json").read_text())
    if lock["packages"]["node_modules/@tauri-apps/cli"]["version"] != "2.11.4":
        raise ValueError("Tauri CLI changed; rebase and test the uninstall template")
    text = raw.decode()
    start = text.index("; 1. Confirm uninstall page")
    end = text.index("; 2. Uninstalling Page", start)
    text = text[:start] + '''; 1. Direct NSIS uninstall always retains user data.
!define MUI_UNCONFIRMPAGE_TEXT_TOP "This removes KnegisWhisper and keeps user data and shared caches. Use Windows Installed apps to choose data removal."
!define MUI_PAGE_CUSTOMFUNCTION_PRE un.SkipIfPassive
!insertmacro MUI_UNPAGE_CONFIRM

''' + text[end:]
    start = text.index("  ; Delete app data if the checkbox is selected")
    end = text.index("  !ifmacrodef NSIS_HOOK_POSTUNINSTALL", start)
    text = text[:start] + text[end:]
    # Uninstall must not kill an active recording, including the race after the
    # helper preflight and direct/silent invocations of the NSIS executable.
    start = text.index("Section Uninstall")
    head, uninstall = text[:start], text[start:]
    uninstall = uninstall.replace('  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"', '''  nsis_tauri_utils::FindProcess "${MAINBINARYNAME}.exe"
  Pop $R0
  ${If} $R0 <> 1
    IfSilent uninstall_running_abort
    ${IfThen} $PassiveMode <> 1 ${|} MessageBox MB_OK "Close KnegisWhisper before uninstalling, then retry." ${|}
    uninstall_running_abort:
    SetErrorLevel 2
    Quit
  ${EndIf}''')
    # Check each known packaged file immediately after removal. Leave the
    # registration in place when a file remains, so the user can retry.
    for path in [r'$INSTDIR\${MAINBINARYNAME}.exe', r'$INSTDIR\\{{this.[1]}}']:
        old_delete = f'Delete "{path}"'
        uninstall = uninstall.replace(old_delete, old_delete + f'''
    ${{If}} ${{FileExists}} "{path}"
      SetErrorLevel 2
      Quit
    ${{EndIf}}''')
    text = head + uninstall
    # HKCU in NSIS belongs to the elevated account. Personal registry cleanup
    # belongs to the unelevated coordinator, just like AppData and credentials.
    start = text.index("  ; Removes the Autostart entry")
    end = text.index("  !ifmacrodef NSIS_HOOK_POSTUNINSTALL", start)
    text = text[:start] + text[end:]
    old = r'  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\uninstall.exe$\""'
    assert text.count(old) == 1
    text = text.replace(old, r'''  ; The asInvoker helper obtains consent before asking for elevation.
  WriteRegStr SHCTX "${UNINSTKEY}" "UninstallString" "$\"$INSTDIR\knegiswhisper-uninstall.exe$\""
  WriteRegStr SHCTX "${UNINSTKEY}" "QuietUninstallString" "$\"$INSTDIR\knegiswhisper-uninstall.exe$\" --quiet"
  WriteRegStr SHCTX "${UNINSTKEY}" "NSISUninstallString" "$\"$INSTDIR\uninstall.exe$\""''')
    old = 'ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"'
    assert text.count(old) == 2
    text = text.replace(old, '''ReadRegStr $R1 SHCTX "${UNINSTKEY}" "NSISUninstallString"
  ${If} $R1 == ""
    ReadRegStr $R1 SHCTX "${UNINSTKEY}" "UninstallString"
  ${EndIf}''')
    # Running helper/uninstaller images cannot be deleted on Windows. Rename
    # to unique tombstones before scheduling deletion, so a reinstall before
    # reboot cannot have its NEW helper removed by the pending operation.
    old = r'    Delete "$INSTDIR\\{{this}}"'
    assert text.count(old) == 1
    text = text.replace(old, r'''    ${If} "{{this}}" == "knegiswhisper-uninstall.exe"
      GetTempFileName $R8 "$INSTDIR"
      Delete "$R8"
      Rename "$INSTDIR\\{{this}}" "$R8"
      ${If} ${FileExists} "$INSTDIR\\{{this}}"
        SetErrorLevel 2
        Quit
      ${EndIf}
      Delete /REBOOTOK "$R8"
    ${Else}
      Delete "$INSTDIR\\{{this}}"
    ${EndIf}''')
    old = r'  Delete "$INSTDIR\uninstall.exe"'
    assert text.count(old) == 1
    text = text.replace(old, r'''  GetTempFileName $R8 "$INSTDIR"
  Delete "$R8"
  Rename "$INSTDIR\uninstall.exe" "$R8"
  ${If} ${FileExists} "$INSTDIR\uninstall.exe"
    SetErrorLevel 2
    Quit
  ${EndIf}
  Delete /REBOOTOK "$R8"''')
    text = text.replace('  RMDir "$INSTDIR"', '  RMDir /REBOOTOK "$INSTDIR"')
    # A previous coordinator may have left a running image awaiting reboot.
    # Treat Windows' success-with-reboot code as success during reinstall too.
    text = text.replace('    ${If} $0 <> 0\n', '    ${If} $0 <> 0\n    ${AndIf} $0 <> 3010\n', 1)
    text += '''
; The coordinator reports pending file removal honestly and never restarts.
Function un.onUninstSuccess
  IfRebootFlag 0 +2
    SetErrorLevel 3010
FunctionEnd
'''
    return "; Generated by tools/uninstall/generate.py. Do not edit by hand.\n" + text


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    generated = generate()
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != generated:
            raise SystemExit("Stale installer.nsi; run python tools/uninstall/generate.py")
    else:
        OUTPUT.write_text(generated, encoding="utf-8", newline="\n")
