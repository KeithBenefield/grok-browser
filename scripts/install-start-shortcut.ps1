# Creates/updates a Start Menu shortcut that matches the app's AppUserModelID
# so Windows can offer "Pin to taskbar".
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File .\scripts\install-start-shortcut.ps1

$ErrorActionPreference = "Stop"
$Aumid = "KeithBenefield.GrokBrowser"
$Exe = Join-Path $env:LOCALAPPDATA "GrokBrowser\bin\Grok.exe"
$StartDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$Lnk = Join-Path $StartDir "Grok Desktop.lnk"

if (-not (Test-Path $Exe)) {
    Write-Host "Missing $Exe"
    Write-Host "Build and copy first:"
    Write-Host '  cargo build --release'
    Write-Host '  Copy-Item target\release\grok-browser.exe "$env:LOCALAPPDATA\GrokBrowser\bin\Grok.exe" -Force'
    exit 1
}

$cs = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class AppIdShortcut {
  [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellLinkW {
    void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszFile, int cch, IntPtr pfd, int fFlags);
    void GetIDList(out IntPtr ppidl);
    void SetIDList(IntPtr pidl);
    void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszName, int cch);
    void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string pszName);
    void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszDir, int cch);
    void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string pszDir);
    void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszArgs, int cch);
    void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string pszArgs);
    void GetHotkey(out short pwHotkey);
    void SetHotkey(short wHotkey);
    void GetShowCmd(out int piShowCmd);
    void SetShowCmd(int iShowCmd);
    void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder pszIconPath, int cch, out int piIcon);
    void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string pszIconPath, int iIcon);
    void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string pszPathRel, int dwReserved);
    void Resolve(IntPtr hwnd, int fFlags);
    void SetPath([MarshalAs(UnmanagedType.LPWStr)] string pszFile);
  }

  [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPropertyStore {
    void GetCount(out uint cProps);
    void GetAt(uint iProp, out PropertyKey pkey);
    void GetValue(ref PropertyKey key, out PropVariant pv);
    void SetValue(ref PropertyKey key, ref PropVariant pv);
    void Commit();
  }

  [StructLayout(LayoutKind.Sequential, Pack = 4)]
  struct PropertyKey {
    public Guid fmtid;
    public uint pid;
  }

  // x64 PROPVARIANT (24 bytes)
  [StructLayout(LayoutKind.Sequential)]
  struct PropVariant {
    public ushort vt;
    public ushort wReserved1;
    public ushort wReserved2;
    public ushort wReserved3;
    public IntPtr pointerValue;
    public IntPtr pad;
  }

  [DllImport("ole32.dll")]
  static extern int PropVariantClear(ref PropVariant pvar);

  [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
  class ShellLink { }

  [ComImport, Guid("0000010b-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IPersistFile {
    void GetClassID(out Guid pClassID);
    [PreserveSig] int IsDirty();
    void Load([In, MarshalAs(UnmanagedType.LPWStr)] string pszFileName, uint dwMode);
    void Save([In, MarshalAs(UnmanagedType.LPWStr)] string pszFileName, [In, MarshalAs(UnmanagedType.Bool)] bool fRemember);
    void SaveCompleted([In, MarshalAs(UnmanagedType.LPWStr)] string pszFileName);
    void GetCurFile([In, MarshalAs(UnmanagedType.LPWStr)] string ppszFileName);
  }

  const ushort VT_LPWSTR = 31;

  static PropVariant FromString(string s) {
    PropVariant pv = new PropVariant();
    pv.vt = VT_LPWSTR;
    pv.pointerValue = Marshal.StringToCoTaskMemUni(s);
    return pv;
  }

  static void SetProp(IPropertyStore store, Guid fmt, uint pid, string value) {
    PropertyKey pk = new PropertyKey();
    pk.fmtid = fmt;
    pk.pid = pid;
    PropVariant pv = FromString(value);
    store.SetValue(ref pk, ref pv);
    PropVariantClear(ref pv);
  }

  public static void Create(string lnkPath, string target, string workDir, string aumid) {
    IShellLinkW link = (IShellLinkW)new ShellLink();
    link.SetPath(target);
    link.SetWorkingDirectory(workDir);
    link.SetIconLocation(target, 0);
    link.SetDescription("Grok desktop shell");

    IPropertyStore store = (IPropertyStore)link;
    Guid fmt = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3");
    string cmd = target.Contains(" ") ? ("\"" + target + "\"") : target;
    SetProp(store, fmt, 5, aumid);
    SetProp(store, fmt, 2, cmd);
    SetProp(store, fmt, 3, target + ",0");
    SetProp(store, fmt, 4, "Grok");
    store.Commit();

    IPersistFile file = (IPersistFile)link;
    file.Save(lnkPath, true);
  }
}
'@

try {
    Add-Type -TypeDefinition $cs -ErrorAction Stop
    [AppIdShortcut]::Create($Lnk, $Exe, (Split-Path $Exe -Parent), $Aumid)
    Write-Host "Created (with AppUserModelID): $Lnk"
} catch {
    Write-Host "COM property store path failed: $_"
    Write-Host "Falling back to basic shortcut..."
    $wsh = New-Object -ComObject WScript.Shell
    $sc = $wsh.CreateShortcut($Lnk)
    $sc.TargetPath = $Exe
    $sc.WorkingDirectory = (Split-Path $Exe -Parent)
    $sc.Description = "Grok desktop shell"
    $sc.IconLocation = "$Exe,0"
    $sc.Save()
    Write-Host "Created (basic): $Lnk"
}

Write-Host "Target:  $Exe"
Write-Host "AUMID:   $Aumid"
Write-Host ""
Write-Host "How to pin to taskbar (Windows 11):"
Write-Host "  1. Close any running Grok window."
Write-Host "  2. Open Start and search for: Grok Desktop"
Write-Host "  3. Right-click that result -> Pin to taskbar"
Write-Host "     (If you only see Pin to Start, try: open the app, then right-click its taskbar icon.)"
Write-Host "  Do not use Brave Apps Grok (that is the browser PWA)."
