# Starts an exe, waits for its main window, saves it as PNG with PrintWindow flag 2 (CopyFromScreen is black for GPUI), stops it.
# Usage: powershell -ExecutionPolicy Bypass -File window-shot.ps1 -Exe C:\x\teams.exe -Out C:\x\shot.png [-Arguments "--demo"] [-WaitSeconds 6] [-Width 720 -Height 640] [-Keys "^k","plat"] [-PostKeys DOWN,ENTER]
# -Keys are SendKeys strings sent to the focused window before the shot.
# -AllWindows composes every visible top-level window of the process (toasts, pill) onto a primary-screen canvas.
# -PostKeys (UP, DOWN, ENTER, TAB, ESC, BACK or characters) go straight to the window as messages, no foreground needed.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [Parameter(Mandatory = $true)][string]$Out,
    [string]$Arguments = "",
    [int]$WaitSeconds = 6,
    [int]$Width = 0,
    [int]$Height = 0,
    [string[]]$Keys = @(),
    [string[]]$PostKeys = @(),
    [switch]$KeepRunning,
    [switch]$AllWindows
)
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class Shot {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr deviceContext, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window, int x, int y, int width, int height, bool repaint);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out RECT rect);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc callback, IntPtr parameter);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr window);
  public delegate bool EnumProc(IntPtr window, IntPtr parameter);
  public static System.Collections.Generic.List<IntPtr> VisibleWindowsOf(uint processId) {
    var found = new System.Collections.Generic.List<IntPtr>();
    EnumWindows((window, parameter) => { uint owner; GetWindowThreadProcessId(window, out owner); if (owner == processId && IsWindowVisible(window)) found.Add(window); return true; }, IntPtr.Zero);
    return found;
  }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
$startInfo = @{ FilePath = $Exe; PassThru = $true }
if ($Arguments -ne "") { $startInfo.ArgumentList = $Arguments }
$process = Start-Process @startInfo
$deadline = (Get-Date).AddSeconds(30)
while ($process.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 200; $process.Refresh() }
if ($process.MainWindowHandle -eq 0) { Write-Error "no window"; Stop-Process -Id $process.Id -Force; exit 1 }
Start-Sleep -Seconds $WaitSeconds
if ($Width -gt 0 -and $Height -gt 0) {
    [void][Shot]::MoveWindow($process.MainWindowHandle, 40, 40, $Width, $Height, $true)
    Start-Sleep -Seconds 2
}
if ($Keys.Count -gt 0) {
    [void][Shot]::SetForegroundWindow($process.MainWindowHandle)
    Start-Sleep -Milliseconds 500
    foreach ($key in $Keys) { [System.Windows.Forms.SendKeys]::SendWait($key); Start-Sleep -Milliseconds 400 }
    Start-Sleep -Seconds 1
}
$virtualKeys = @{ UP = 0x26; DOWN = 0x28; ENTER = 0x0D; TAB = 0x09; ESC = 0x1B; BACK = 0x08 }
foreach ($key in ($PostKeys | ForEach-Object { $_ -split ',' })) {
    if ($virtualKeys.ContainsKey($key)) {
        [void][Shot]::PostMessage($process.MainWindowHandle, 0x100, [IntPtr]$virtualKeys[$key], [IntPtr]0x01000001)
        [void][Shot]::PostMessage($process.MainWindowHandle, 0x101, [IntPtr]$virtualKeys[$key], [IntPtr]0xC1000001)
    } else {
        foreach ($character in $key.ToCharArray()) { [void][Shot]::PostMessage($process.MainWindowHandle, 0x102, [IntPtr][int]$character, [IntPtr]0) }
    }
    Start-Sleep -Milliseconds 400
}
if ($PostKeys.Count -gt 0) { Start-Sleep -Seconds 1 }
if ($AllWindows) {
    $screen = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $canvas = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
    $canvasGraphics = [System.Drawing.Graphics]::FromImage($canvas)
    $canvasGraphics.Clear([System.Drawing.Color]::FromArgb(15, 15, 16))
    $handles = [Shot]::VisibleWindowsOf([uint32]$process.Id) | Sort-Object { $_ -eq $process.MainWindowHandle } -Descending
    foreach ($handle in $handles) {
        $windowRect = New-Object Shot+RECT
        [void][Shot]::GetWindowRect($handle, [ref]$windowRect)
        $windowWidth = $windowRect.Right - $windowRect.Left; $windowHeight = $windowRect.Bottom - $windowRect.Top
        if ($windowWidth -le 0 -or $windowHeight -le 0) { continue }
        $part = New-Object System.Drawing.Bitmap $windowWidth, $windowHeight
        $partGraphics = [System.Drawing.Graphics]::FromImage($part)
        $partContext = $partGraphics.GetHdc()
        [void][Shot]::PrintWindow($handle, $partContext, 2)
        $partGraphics.ReleaseHdc($partContext)
        $canvasGraphics.DrawImage($part, $windowRect.Left - $screen.Left, $windowRect.Top - $screen.Top)
        $partGraphics.Dispose(); $part.Dispose()
    }
    $canvas.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    $canvasGraphics.Dispose(); $canvas.Dispose()
    Write-Output "saved $($screen.Width)x$($screen.Height) with $($handles.Count) windows"
    if (-not $KeepRunning) { Stop-Process -Id $process.Id -Force }
    exit 0
}
$rect = New-Object Shot+RECT
[void][Shot]::GetWindowRect($process.MainWindowHandle, [ref]$rect)
$width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$deviceContext = $graphics.GetHdc()
[void][Shot]::PrintWindow($process.MainWindowHandle, $deviceContext, 2)
$graphics.ReleaseHdc($deviceContext)
$bitmap.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose(); $bitmap.Dispose()
Write-Output "saved ${width}x${height}"
if (-not $KeepRunning) { Stop-Process -Id $process.Id -Force }
