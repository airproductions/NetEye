param(
  [Parameter(Mandatory)][string]$Out,
  [string]$Exe = "$PSScriptRoot\..\target\debug\vantage.exe",
  [string]$Page = "",
  [string]$Appearance = "",
  [int]$Wait = 6
)
# Dev helper: launches a debug build, captures its window with PrintWindow, saves a PNG, closes it.
# Vantage is single-instance, so a machine-wide mutex makes parallel callers take turns.
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class VantageShot {
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint f);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  public struct RECT { public int L, T, R, B; }
}
"@
[VantageShot]::SetProcessDPIAware() | Out-Null
$lock = New-Object System.Threading.Mutex($false, "Local\VantageShotLock")
$lock.WaitOne() | Out-Null
try {
  Get-Process vantage -ErrorAction SilentlyContinue | Stop-Process -Force
  Start-Sleep -Milliseconds 500
  $env:VANTAGE_START_PAGE = $Page
  $env:VANTAGE_APPEARANCE = $Appearance
  $p = Start-Process (Resolve-Path $Exe) -PassThru
  Start-Sleep -Seconds $Wait
  $p.Refresh()
  $h = $p.MainWindowHandle
  if ($h -eq [IntPtr]::Zero) { Write-Output "no window"; exit 1 }
  $r = New-Object VantageShot+RECT
  [VantageShot]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $dc = $g.GetHdc()
  [VantageShot]::PrintWindow($h, $dc, 2) | Out-Null
  $g.ReleaseHdc($dc)
  $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Output "saved $Out $($bmp.Width)x$($bmp.Height)"
  Stop-Process -Id $p.Id -Force
} finally {
  $lock.ReleaseMutex()
}
