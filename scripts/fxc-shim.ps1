# fxc.exe stand-in: compiles HLSL via d3dcompiler_47.dll (ships with Windows). Args as fxc: /T /E /Fh /Vn /O3 <file>
$FxcArgs = $args
$target = $null; $entry = $null; $header = $null; $varName = $null; $source = $null
for ($index = 0; $index -lt $FxcArgs.Count; $index++) {
    switch ($FxcArgs[$index]) {
        '/T' { $target = $FxcArgs[++$index] }
        '/E' { $entry = $FxcArgs[++$index] }
        '/Fh' { $header = $FxcArgs[++$index] }
        '/Vn' { $varName = $FxcArgs[++$index] }
        '/O3' { }
        default { $source = $FxcArgs[$index] }
    }
}
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class D3DC {
  [DllImport("d3dcompiler_47.dll", CharSet=CharSet.Unicode)]
  public static extern int D3DCompileFromFile(string file, IntPtr defines, IntPtr include, [MarshalAs(UnmanagedType.LPStr)] string entry, [MarshalAs(UnmanagedType.LPStr)] string target, uint flags1, uint flags2, out IntPtr code, out IntPtr errors);
  [DllImport("d3dcompiler_47.dll")] public static extern int D3DDisassemble(IntPtr a, IntPtr b, uint c, IntPtr d, out IntPtr e);
  public static byte[] Blob(IntPtr blob) {
    IntPtr vtable = Marshal.ReadIntPtr(blob);
    var getPointer = (GetPtr)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vtable, 3 * IntPtr.Size), typeof(GetPtr));
    var getSize = (GetSize)Marshal.GetDelegateForFunctionPointer(Marshal.ReadIntPtr(vtable, 4 * IntPtr.Size), typeof(GetSize));
    IntPtr data = getPointer(blob); int size = (int)getSize(blob);
    byte[] bytes = new byte[size]; Marshal.Copy(data, bytes, 0, size); return bytes;
  }
  [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate IntPtr GetPtr(IntPtr self);
  [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate UIntPtr GetSize(IntPtr self);
}
'@
$code = [IntPtr]::Zero; $errors = [IntPtr]::Zero
# 1 = D3D_COMPILE_STANDARD_FILE_INCLUDE; flags: optimization level 3 = 1 shl 15
$result = [D3DC]::D3DCompileFromFile($source, [IntPtr]::Zero, [IntPtr]1, $entry, $target, 32768, 0, [ref]$code, [ref]$errors)
if ($result -lt 0) {
    if ($errors -ne [IntPtr]::Zero) { [Console]::Error.WriteLine([Text.Encoding]::ASCII.GetString([D3DC]::Blob($errors))) }
    [Console]::Error.WriteLine("D3DCompileFromFile failed: 0x{0:X8}" -f $result)
    exit 1
}
$bytes = [D3DC]::Blob($code)
$lines = for ($offset = 0; $offset -lt $bytes.Length; $offset += 8) {
    '    ' + ((($bytes[$offset..([Math]::Min($offset + 7, $bytes.Length - 1))]) | ForEach-Object { '0x{0:x2}' -f $_ }) -join ', ')
}
$text = "const BYTE $varName[] =`n{`n" + ($lines -join ",`n") + "`n};`n"
[IO.File]::WriteAllText($header, $text)
