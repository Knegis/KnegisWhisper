"""Execute the generated removal fragment against disposable files, never an installed app.

On Windows, requires NSIS_MAKENSIS or the compiler cached by a Tauri NSIS build.
No registry, credentials, elevation or profile cleanup is involved.
"""
import contextlib
import ctypes
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import generate


@contextlib.contextmanager
def deny_delete_sharing(path):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32,
                                  ctypes.c_uint32, ctypes.c_void_p,
                                  ctypes.c_uint32, ctypes.c_uint32, ctypes.c_void_p]
    kernel.CreateFileW.restype = ctypes.c_void_p
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    kernel.CloseHandle.restype = ctypes.c_int
    # GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING. Readers may run; rename may not.
    handle = kernel.CreateFileW(str(path), 0x80000000, 1, None, 3, 0, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        yield
    finally:
        kernel.CloseHandle(handle)


@unittest.skipUnless(sys.platform == "win32", "Native NSIS file-sharing regression")
class WindowsEntryPointRecovery(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        compiler = Path(os.environ.get("NSIS_MAKENSIS", str(
            Path(os.environ["LOCALAPPDATA"]) / "tauri/NSIS/makensis.exe")))
        if not compiler.is_file():
            raise RuntimeError("Build the NSIS installer first or set NSIS_MAKENSIS")
        cls.temporary = tempfile.TemporaryDirectory(prefix="knegis-uninstall-regression-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.root = Path(cls.temporary.name).resolve()
        text = generate.generate()
        start = text.index("  ; Begin recoverable uninstall entry-point removal")
        end = text.index("  ; End recoverable uninstall entry-point removal", start)
        # Compile the exact generated production fragment, including error
        # branches and delayed-delete instructions, rather than reimplementing it.
        fragment = text[start:end]
        script = r'''Unicode true
Name "Knegis disposable entry-point regression"
OutFile "fixture.exe"
RequestExecutionLevel user
SilentInstall silent
!include "LogicLib.nsh"
Section
  ${If} $EXEFILE == "knegiswhisper-uninstall.exe"
    SetErrorLevel 0
    Quit
  ${EndIf}
  StrCpy $INSTDIR "$EXEDIR\payload"
''' + fragment + r'''
  SetErrorLevel 0
SectionEnd
'''
        (cls.root / "fixture.nsi").write_text(script, encoding="utf-8")
        result = subprocess.run([str(compiler), "/V2", "fixture.nsi"], cwd=cls.root,
                                capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise RuntimeError(result.stdout + result.stderr)

    def setUp(self):
        self.temporary_case = tempfile.TemporaryDirectory(dir=self.root)
        self.addCleanup(self.temporary_case.cleanup)
        self.case = Path(self.temporary_case.name).resolve()
        assert self.case.is_relative_to(self.root)
        self.runner = self.case / "fixture.exe"
        shutil.copy2(self.root / "fixture.exe", self.runner)
        self.payload = self.case / "payload"
        self.payload.mkdir()
        self.helper = self.payload / "knegiswhisper-uninstall.exe"
        self.uninstaller = self.payload / "uninstall.exe"
        shutil.copy2(self.runner, self.helper)
        self.uninstaller.write_bytes(b"fictional uninstall image")
        self.sentinel = self.case / "unrelated"
        self.sentinel.write_bytes(b"retain")
        self.originals = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in self.payload.iterdir()}

    def run_fragment(self):
        return subprocess.run([str(self.runner), "/S"], cwd=self.case, timeout=30).returncode

    def assert_retryable(self):
        self.assertEqual({p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                          for p in self.payload.iterdir()}, self.originals)
        # Both registry command forms would still resolve to a loadable helper.
        # This does not simulate ARP/UAC: it only verifies the paths survived.
        for args in [[], ["--quiet"]]:
            self.assertEqual(subprocess.run([str(self.helper), *args], timeout=30).returncode, 0)
        self.assertEqual(self.sentinel.read_bytes(), b"retain")

    def assert_completed(self):
        self.assertEqual(list(self.payload.iterdir()), [])
        self.assertEqual(self.sentinel.read_bytes(), b"retain")

    def test_locked_uninstaller_keeps_helper_and_retry_works(self):
        with deny_delete_sharing(self.uninstaller):
            self.assertEqual(self.run_fragment(), 2)
            self.assert_retryable()
            self.assertEqual(self.run_fragment(), 2)
            self.assert_retryable()
        self.assertEqual(self.run_fragment(), 0)
        self.assert_completed()

    def test_locked_helper_restores_uninstaller_and_retry_works(self):
        with deny_delete_sharing(self.helper):
            self.assertEqual(self.run_fragment(), 2)
            self.assert_retryable()
        self.assertEqual(self.run_fragment(), 0)
        self.assert_completed()

    def test_success_removes_both_images_and_keeps_sibling(self):
        self.assertEqual(self.run_fragment(), 0)
        self.assert_completed()


if __name__ == "__main__":
    unittest.main()
