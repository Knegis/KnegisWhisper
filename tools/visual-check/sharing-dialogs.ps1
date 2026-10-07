param([int]$AppProcessId, [string]$PackagePath)
# Drives only the two native file dialogs belonging to the disposable smoke test.
$PackagePath = [IO.Path]::GetFullPath($PackagePath)
$sharingTempRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (!$PackagePath.StartsWith($sharingTempRoot, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($PackagePath) -ne 'native.knegis') {
    throw 'This helper accepts only the disposable native.knegis package under TEMP.'
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class SharingDialogs {
    public delegate bool EnumProc(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr window, StringBuilder name, int size);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc callback, IntPtr data);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr window);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wparam, string value);
    [DllImport("user32.dll", EntryPoint="SendMessageW")] public static extern IntPtr SendMessageCode(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    public static bool Fill(IntPtr dialog, string path) {
        IntPtr edit=IntPtr.Zero, accept=IntPtr.Zero;
        EnumChildWindows(dialog,(window,data) => {
            var name=new StringBuilder(100); GetClassName(window,name,100);
            int id=GetDlgCtrlID(window);
            if(name.ToString()=="Edit") {
                var parent=GetParent(window);
                var parentName=new StringBuilder(100); GetClassName(parent,parentName,100);
                if((id==1001 || id==1148) && parentName.ToString()=="ComboBox") edit=window;
            }
            if(name.ToString()=="Button" && id==1) accept=window;
            return true;
        },IntPtr.Zero);
        if(edit==IntPtr.Zero || accept==IntPtr.Zero) return false;
        SendMessage(edit,0x000C,IntPtr.Zero,path);
        // The modern dialog caches its ComboBox edit value. Generate an edit
        // notification before invoking Save/Open so it reads the new path.
        SendMessageCode(edit,0x00B1,new IntPtr(-1),new IntPtr(-1));
        SendMessageCode(edit,0x0102,new IntPtr(32),IntPtr.Zero);
        SendMessageCode(edit,0x0102,new IntPtr(8),IntPtr.Zero);
        PostMessage(accept,0x00F5,IntPtr.Zero,IntPtr.Zero);
        return true;
    }
    public static string Describe(IntPtr dialog) {
        var output=new StringBuilder();
        EnumChildWindows(dialog,(window,data) => {
            var name=new StringBuilder(100); GetClassName(window,name,100);
            output.Append(name).Append(':').Append(GetDlgCtrlID(window)).Append(" parent:").Append(GetDlgCtrlID(GetParent(window))).Append(';');
            return true;
        },IntPtr.Zero);
        return output.ToString();
    }
    public static IntPtr Find(int pid) {
        IntPtr found=IntPtr.Zero;
        EnumWindows((window,data) => {
            uint owner; GetWindowThreadProcessId(window,out owner);
            var name=new StringBuilder(100); GetClassName(window,name,100);
            if(owner==pid && name.ToString()=="#32770") { found=window; return false; }
            return true;
        },IntPtr.Zero);
        return found;
    }
}
'@
$deadline = [DateTime]::UtcNow.AddSeconds(75)
$handled = 0
$previousDialog = [IntPtr]::Zero
$seenDialog = [IntPtr]::Zero
$readyAfter = [DateTime]::UtcNow
while ([DateTime]::UtcNow -lt $deadline -and $handled -lt 2) {
    $dialogHandle = [SharingDialogs]::Find($AppProcessId)
    if ($dialogHandle -eq [IntPtr]::Zero) { $previousDialog = [IntPtr]::Zero; Start-Sleep -Milliseconds 100; continue }
    if ($dialogHandle -eq $previousDialog) { Start-Sleep -Milliseconds 100; continue }
    if ($dialogHandle -ne $seenDialog) { $seenDialog = $dialogHandle; $readyAfter = [DateTime]::UtcNow.AddMilliseconds(700) }
    # Common Item Dialog creates controls before it sets their initial values.
    if ([DateTime]::UtcNow -lt $readyAfter) { Start-Sleep -Milliseconds 100; continue }
    if ([SharingDialogs]::Fill($dialogHandle, $PackagePath)) {
        Write-Output "Completed native sharing file dialog."
        $previousDialog = $dialogHandle; $handled++; Start-Sleep -Milliseconds 300; continue
    }
    Start-Sleep -Milliseconds 100
}
if ($handled -ne 2) {
    $dialogHandle = [SharingDialogs]::Find($AppProcessId)
    $controls = [SharingDialogs]::Describe($dialogHandle)
    if ($dialogHandle -ne [IntPtr]::Zero) {
        # Cancel the test's outstanding dialog so the test event loop can exit.
        [void][SharingDialogs]::PostMessage($dialogHandle, 0x0111, [IntPtr]2, [IntPtr]::Zero)
    }
    throw "Only $handled of the two smoke-test dialogs completed. Controls: $controls"
}
