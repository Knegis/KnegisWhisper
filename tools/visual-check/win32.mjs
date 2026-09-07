/** Queries native window state without assuming that a title is unique. */
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);

/**
 * Return every exact-title match plus first-match fields retained for callers.
 *
 * Do not replace EnumWindows with FindWindowW($null, title): PowerShell binds
 * $null to a .NET string parameter as String.Empty, which searches for a
 * window with an empty class name rather than an unspecified class name.
 */
export async function windowByTitle(title) {
  if (process.platform !== "win32") return { unsupported: true };

  const encoded = Buffer.from(String(title), "utf16le").toString("base64");
  const script = `$title = [Text.Encoding]::Unicode.GetString(
  [Convert]::FromBase64String('${encoded}')
)

Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public struct RECT {
  public int Left;
  public int Top;
  public int Right;
  public int Bottom;
}

public sealed class WindowInfo {
  public IntPtr Hwnd;
  public bool Visible;
  public RECT Rect;
  public uint Affinity;
  public bool HasAffinity;
  public uint Pid;
}

public static class W {
  public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);

  [DllImport("user32.dll")]
  public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);

  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetWindowTextLengthW(IntPtr hwnd);

  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern int GetWindowTextW(IntPtr hwnd, StringBuilder text, int count);

  [DllImport("user32.dll")]
  public static extern bool IsWindowVisible(IntPtr hwnd);

  [DllImport("user32.dll")]
  public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);

  [DllImport("user32.dll")]
  public static extern bool GetWindowDisplayAffinity(IntPtr hwnd, out uint affinity);

  [DllImport("user32.dll")]
  public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);

  public static List<WindowInfo> FindByTitle(string title) {
    var matches = new List<WindowInfo>();
    EnumWindows((hwnd, unused) => {
      var length = GetWindowTextLengthW(hwnd);
      var text = new StringBuilder(length + 1);
      GetWindowTextW(hwnd, text, text.Capacity);
      if (!String.Equals(text.ToString(), title, StringComparison.Ordinal)) return true;

      var rect = new RECT();
      var affinity = 0u;
      var pid = 0u;
      GetWindowRect(hwnd, out rect);
      GetWindowThreadProcessId(hwnd, out pid);
      var gotAffinity = GetWindowDisplayAffinity(hwnd, out affinity);
      matches.Add(new WindowInfo {
        Hwnd = hwnd,
        Visible = IsWindowVisible(hwnd),
        Rect = rect,
        Affinity = affinity,
        HasAffinity = gotAffinity,
        Pid = pid,
      });
      return true;
    }, IntPtr.Zero);
    return matches;
  }
}
'@

$matches = @([W]::FindByTitle($title) | ForEach-Object {
  @{
    hwnd = ('0x{0:X}' -f $_.Hwnd.ToInt64())
    visible = $_.Visible
    rect = @{
      left = $_.Rect.Left
      top = $_.Rect.Top
      right = $_.Rect.Right
      bottom = $_.Rect.Bottom
    }
    affinity = if ($_.HasAffinity) { $_.Affinity } else { $null }
    pid = $_.Pid
  }
})

$first = $matches | Select-Object -First 1
@{
  found = $matches.Count -gt 0
  visible = if ($first) { $first.visible } else { $false }
  rect = if ($first) { $first.rect } else { $null }
  affinity = if ($first) { $first.affinity } else { $null }
  matches = $matches
} | ConvertTo-Json -Compress -Depth 4`;

  const { stdout } = await execFileAsync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    { windowsHide: true },
  );
  return JSON.parse(stdout);
}
