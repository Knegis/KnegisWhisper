import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import generate


class TemplateContracts(unittest.TestCase):
    def test_direct_uninstaller_has_no_user_data_deletion(self):
        text = generate.generate()
        self.assertNotIn("DeleteAppDataCheckbox", text)
        self.assertNotIn('RmDir /r "$APPDATA', text)
        self.assertNotIn('RmDir /r "$LOCALAPPDATA', text)
        self.assertIn("keeps user data and shared caches", text)
        uninstall = text.split("Section Uninstall", 1)[1]
        self.assertNotIn("CheckIfAppIsRunning", uninstall)
        self.assertNotIn("DeleteRegValue HKCU", uninstall)
        self.assertIn('nsis_tauri_utils::FindProcess', uninstall)

    def test_registration_and_upgrades_have_separate_entry_points(self):
        text = generate.generate()
        self.assertIn('"UninstallString" "$\\"$INSTDIR\\knegiswhisper-uninstall.exe$\\""', text)
        self.assertIn('"QuietUninstallString" "$\\"$INSTDIR\\knegiswhisper-uninstall.exe$\\" --quiet"', text)
        self.assertEqual(text.count('ReadRegStr $R1 SHCTX "${UNINSTKEY}" "NSISUninstallString"'), 2)
        self.assertIn('StrCpy $R1 "$R1 /UPDATE"', text)
        self.assertIn('${AndIf} $0 <> 3010', text)

    def test_cli_upgrade_requires_review(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "apps/desktop").mkdir(parents=True)
            (root / "apps/desktop/package-lock.json").write_text(json.dumps({
                "packages": {"node_modules/@tauri-apps/cli": {"version": "99.0.0"}}
            }))
            with patch.object(generate, "ROOT", root), self.assertRaisesRegex(ValueError, "Tauri CLI changed"):
                generate.generate()

    def test_template_tampering_requires_review(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "upstream.nsi"
            path.write_text("different template")
            with patch.object(generate, "UPSTREAM", path), self.assertRaisesRegex(ValueError, "hash changed"):
                generate.generate()


if __name__ == "__main__":
    unittest.main()
