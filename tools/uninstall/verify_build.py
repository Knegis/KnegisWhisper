"""Inspect the built helper's PE manifest and generated NSIS script; never execute it."""
import argparse
import ctypes
from pathlib import Path
import xml.etree.ElementTree as ET


def embedded_manifest(path):
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    pointer = ctypes.c_void_p
    kernel.LoadLibraryExW.argtypes = [ctypes.c_wchar_p, pointer, ctypes.c_uint32]
    kernel.LoadLibraryExW.restype = pointer
    kernel.FindResourceW.argtypes = [pointer, pointer, pointer]
    kernel.FindResourceW.restype = pointer
    kernel.LoadResource.argtypes = [pointer, pointer]
    kernel.LoadResource.restype = pointer
    kernel.LockResource.argtypes = [pointer]
    kernel.LockResource.restype = pointer
    kernel.SizeofResource.argtypes = [pointer, pointer]
    kernel.SizeofResource.restype = ctypes.c_uint32
    kernel.FreeLibrary.argtypes = [pointer]
    kernel.FreeLibrary.restype = ctypes.c_int
    # DATAFILE | IMAGE_RESOURCE maps resources without executing DLL entrypoints.
    module = kernel.LoadLibraryExW(str(path.resolve()), None, 0x22)
    if not module:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        resource = kernel.FindResourceW(module, pointer(1), pointer(24))
        if not resource:
            raise ValueError("Helper has no embedded application manifest")
        size = kernel.SizeofResource(module, resource)
        data = kernel.LockResource(kernel.LoadResource(module, resource))
        if not size or not data:
            raise ctypes.WinError(ctypes.get_last_error())
        return ctypes.string_at(data, size)
    finally:
        kernel.FreeLibrary(module)


def verify(directory):
    helper = directory / "knegiswhisper-uninstall.exe"
    root = ET.fromstring(embedded_manifest(helper))
    level = root.find(".//{urn:schemas-microsoft-com:asm.v3}requestedExecutionLevel")
    if level is None or level.get("level") != "asInvoker" or level.get("uiAccess") != "false":
        raise ValueError("Helper must explicitly start asInvoker before consent")
    if not any(e.get("name") == "Microsoft.Windows.Common-Controls" and e.get("version") == "6.0.0.0"
               for e in root.iter("{urn:schemas-microsoft-com:asm.v1}assemblyIdentity")):
        raise ValueError("Helper needs Common Controls v6")
    script = (directory / "nsis/x64/installer.nsi").read_text(encoding="utf-8")
    for required in ['"/oname=knegiswhisper-uninstall.exe"', '"NSISUninstallString"',
                     '"QuietUninstallString"', ' --quiet"', 'uninstall_running_abort:']:
        if required not in script:
            raise ValueError("Missing built installer contract: " + required)
    uninstall = script.split("Section Uninstall", 1)[1]
    for forbidden in ["DeleteAppDataCheckbox", "CheckIfAppIsRunning", "DeleteRegValue HKCU", 'RmDir /r "$APPDATA']:
        if forbidden in uninstall:
            raise ValueError("Unsafe generated uninstall operation: " + forbidden)
    print("Built helper manifest, inclusion, entry points and retention guards verified")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target_dir", type=Path)
    verify(parser.parse_args().target_dir)
