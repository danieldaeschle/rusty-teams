# Read or set the Windows default audio endpoints (console and communications role) for the call end-to-end test.
# get <render|capture>            prints "<console id>|<communications id>"
# set <render|capture> <name part or id>|<communications id>   sets both roles
param([string]$Action, [string]$Flow, [string]$Target)
Add-Type -TypeDefinition @"
using System; using System.Runtime.InteropServices;
[Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceEnumerator { int EnumAudioEndpoints(int dataFlow, int state, out IMMDeviceCollection devices); int GetDefaultAudioEndpoint(int dataFlow, int role, out IMMDevice device); }
[Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDeviceCollection { int GetCount(out int count); int Item(int index, out IMMDevice device); }
[Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IMMDevice { int Activate(); int OpenPropertyStore(int access, out IPropertyStore store); int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id); }
[Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IPropertyStore { int GetCount(out int count); int GetAt(int index, out PropertyKey key); int GetValue(ref PropertyKey key, out PropVariant value); }
[Guid("f8679f50-850a-41cf-9c72-430f290290c8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IPolicyConfig { int A(); int B(); int C(); int D(); int E(); int F(); int G(); int H(); int I(); int J();
  int SetDefaultEndpoint([MarshalAs(UnmanagedType.LPWStr)] string id, int role); }
[StructLayout(LayoutKind.Sequential)] public struct PropertyKey { public Guid fmtid; public int pid; }
[StructLayout(LayoutKind.Explicit)] public struct PropVariant { [FieldOffset(0)] public short vt; [FieldOffset(8)] public IntPtr pointer; }
[ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumerator {}
[ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")] class PolicyConfigClient {}
public static class DefaultAudio {
  const int Console = 0, Communications = 2, Active = 1;
  static IMMDeviceEnumerator Enumerator() { return (IMMDeviceEnumerator)new MMDeviceEnumerator(); }
  static string Id(IMMDevice device) { string id; device.GetId(out id); return id; }
  static string Name(IMMDevice device) {
    IPropertyStore store; device.OpenPropertyStore(0, out store);
    var key = new PropertyKey { fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"), pid = 14 }; PropVariant value; store.GetValue(ref key, out value);
    return Marshal.PtrToStringUni(value.pointer);
  }
  public static string Get(int flow) {
    IMMDevice console, communications;
    Enumerator().GetDefaultAudioEndpoint(flow, Console, out console);
    Enumerator().GetDefaultAudioEndpoint(flow, Communications, out communications);
    return Id(console) + "|" + Id(communications);
  }
  public static string Resolve(int flow, string target) {
    if (target.StartsWith("{")) return target;
    IMMDeviceCollection devices; Enumerator().EnumAudioEndpoints(flow, Active, out devices);
    int count; devices.GetCount(out count);
    for (int index = 0; index < count; index++) {
      IMMDevice device; devices.Item(index, out device);
      if (Name(device).Contains(target)) return Id(device);
    }
    throw new Exception("no active endpoint named " + target);
  }
  public static void Set(int flow, string consoleTarget, string communicationsTarget) {
    var policy = (IPolicyConfig)new PolicyConfigClient();
    policy.SetDefaultEndpoint(Resolve(flow, consoleTarget), Console);
    policy.SetDefaultEndpoint(Resolve(flow, communicationsTarget), Communications);
  }
}
"@
$dataFlow = if ($Flow -eq 'capture') { 1 } else { 0 }
if ($Action -eq 'get') {
  [DefaultAudio]::Get($dataFlow)
} elseif ($Action -eq 'set') {
  $parts = $Target.Split('|')
  $communications = if ($parts.Length -gt 1) { $parts[1] } else { $parts[0] }
  [DefaultAudio]::Set($dataFlow, $parts[0], $communications)
  [DefaultAudio]::Get($dataFlow)
} else {
  Write-Error 'usage: default-audio.ps1 get|set render|capture [target]'
  exit 1
}
