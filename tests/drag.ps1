# Verifies that a corner does not fire while a mouse button is held, which is
# what stops Windows Snap (dragging a window into a corner) from also
# triggering the corner's action.
#
# Three things are checked, and the second is the one that matters most:
#
#   1. Button held, parked in the corner        -> must NOT fire
#   2. Button released, still in the corner     -> must STILL NOT fire
#   3. Left the corner and returned, no button  -> must fire
#
# Without (2) the corner would fire at the exact moment you release to complete
# a snap, which is the worst possible time: you would get the snap AND the
# action.
#
# Uses the MIDDLE button rather than the left. The suppression code treats all
# three buttons identically, so this exercises the same path, and a synthetic
# middle-click at a screen corner cannot drag, activate, or dismiss anything
# the way a left-click could.

$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"

$proj    = Split-Path $PSScriptRoot -Parent
$marker  = Join-Path $PSScriptRoot 'drag-fired.txt'
$cfg     = Get-HcConfigPath
$backup  = Join-Path $PSScriptRoot 'config.dragbackup.json'

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class M {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int dx, int dy, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr h, IntPtr c, MonEnum f, IntPtr d);
  [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);

  public const uint MIDDLEDOWN = 0x0020, MIDDLEUP = 0x0040;

  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO {
    public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
  }
  public delegate bool MonEnum(IntPtr m, IntPtr hdc, IntPtr r, IntPtr d);

  public static List<string> Rects() {
    var o = new List<string>();
    EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (m, hdc, r, d) => {
      var mi = new MONITORINFO(); mi.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
      if (GetMonitorInfo(m, ref mi))
        o.Add(string.Format("{0},{1},{2},{3}",
          mi.rcMonitor.L, mi.rcMonitor.T, mi.rcMonitor.R, mi.rcMonitor.B));
      return true;
    }, IntPtr.Zero);
    return o;
  }
}
"@

# Before any coordinate call: an unaware process has its cursor coordinates
# virtualised, so it and the (per-monitor-v2) app disagree about where the
# corners are.
[void][M]::SetProcessDpiAwarenessContext([IntPtr](-4))

Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 600

if (-not (Backup-HcConfig $backup)) { Write-Host 'RESULT: SKIP - stale backup present'; exit 1 }
Remove-Item $marker -ErrorAction SilentlyContinue

$L = [int]::MaxValue; $T = [int]::MaxValue; $R = [int]::MinValue; $B = [int]::MinValue
foreach ($line in [M]::Rects()) {
  $n = $line -split ','
  Write-Host ("  monitor: {0},{1} .. {2},{3}" -f $n[0], $n[1], $n[2], $n[3])
  $L = [Math]::Min($L, [int]$n[0]); $T = [Math]::Min($T, [int]$n[1])
  $R = [Math]::Max($R, [int]$n[2]); $B = [Math]::Max($B, [int]$n[3])
}
Write-Host ("union rect: {0},{1} .. {2},{3}" -f $L, $T, $R, $B)

# A silently wrong union means the test parks somewhere that is not a corner
# and "passes" without exercising anything. Fail loudly instead.
if ($R -le $L -or $B -le $T) {
  Restore-HcConfig $backup
  Write-Host "RESULT: FAIL - bogus union rect"; exit 1
}

Write-HcConfig @{
  enabled               = $true
  corners               = @(
    @{ kind = 'custom'; command = 'cmd.exe'; args = ('/c echo fired > "{0}"' -f $marker) },
    @{ kind = 'none' }, @{ kind = 'none' }, @{ kind = 'none' }
  )
  dwellMs               = 150
  cooldownMs            = 700
  cornerSize            = 12
  monitorMode           = 'outerCorners'
  suppressFullscreen    = $false
  suppressWhileDragging = $true
  modifier              = 'none'
}

Start-Process -FilePath (Join-Path $proj 'target\release\cantos.exe')
Start-Sleep -Seconds 2

$orig = New-Object 'M+P'; [void][M]::GetCursorPos([ref]$orig)

# Hold position against whatever physical mouse input is happening.
function Hold($x, $y, $ms) {
  $n = [int]($ms / 100)
  for ($i = 0; $i -lt $n; $i++) { [void][M]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 100 }
}

$cx = [int](($L + $R) / 2); $cy = [int](($T + $B) / 2)
$held = $false
try {
  Hold $cx $cy 500                      # start well away from any corner

  Write-Host "--- 1: middle button held, parked in the corner ---"
  [M]::mouse_event([M]::MIDDLEDOWN, 0, 0, 0, [IntPtr]::Zero); $held = $true
  Hold ($L + 1) ($T + 1) 1800
  $fired1 = Test-Path $marker
  Write-Host ("  fired: {0} (want False)" -f $fired1)

  Write-Host "--- 2: button released, still in the corner ---"
  [M]::mouse_event([M]::MIDDLEUP, 0, 0, 0, [IntPtr]::Zero); $held = $false
  Hold ($L + 1) ($T + 1) 1400
  $fired2 = Test-Path $marker
  Write-Host ("  fired: {0} (want False - must leave the corner first)" -f $fired2)

  Write-Host "--- 3: left the corner and came back, no button ---"
  Hold $cx $cy 900
  Hold ($L + 2) ($T + 2) 1800
  $fired3 = Test-Path $marker
  Write-Host ("  fired: {0} (want True)" -f $fired3)
}
finally {
  if ($held) { [M]::mouse_event([M]::MIDDLEUP, 0, 0, 0, [IntPtr]::Zero) }
  [void][M]::SetCursorPos($orig.X, $orig.Y)
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
  Restore-HcConfig $backup
  Remove-Item $marker -ErrorAction SilentlyContinue
}

Write-Host ""
if (-not $fired1 -and -not $fired2 -and $fired3) { Write-Host "RESULT: PASS"; exit 0 }
Write-Host ("RESULT: FAIL  (held={0} released={1} returned={2})" -f $fired1, $fired2, $fired3)
exit 1
