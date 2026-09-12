# Verifies what fullscreen suppression does and does not cover.
#
#   1. A borderless window covering the whole monitor  -> must NOT fire
#   2. An ordinary maximised window                    -> must STILL fire
#   3. A borderless AND maximised window               -> must NOT fire
#
# (2) is the regression this was written for. The old check was pure geometry:
# does the foreground window's rect cover its monitor? But GetWindowRect
# reports the frame including the invisible resize border, so a maximised
# window overhangs its monitor by ~8px on every edge it is free to grow into.
# Wherever the work area equals the monitor rect -- a second display with no
# taskbar, which is the Windows 11 default, or an auto-hide taskbar -- that
# overhang alone satisfied the test, and every corner on every display went
# quiet while an ordinary maximised window had focus.
#
# WHAT THIS SCRIPT DOES NOT PROVE. Measured on a normal desktop,
# SHQueryUserNotificationState returns QUNS_BUSY for any borderless window
# covering the monitor -- cases (1) and (3) both. fullscreen_active() consults
# that state first, so it short-circuits and the geometry path, IsZoomed and
# the frame check included, never runs for those two. They are genuine
# end-to-end assertions about the app's behaviour, but they do NOT isolate
# `has_frame`; the Rust unit tests do that.
#
# Case (2) is the only one that reaches the geometry, and on a primary display
# with a visible taskbar it passes either way, because there the work area is
# genuinely shorter than the monitor. The geometry that actually broke is
# pinned in the Rust unit tests
# (`a_maximised_window_with_no_taskbar_looks_like_fullscreen`). To see the real
# thing, maximise a window on a second display that has no taskbar.

$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"

$proj   = Split-Path $PSScriptRoot -Parent
$marker = Join-Path $PSScriptRoot 'fs-fired.txt'
$backup = Join-Path $PSScriptRoot 'config.fsbackup.json'
$exe    = Join-Path $proj 'target\release\cantos.exe'

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class F {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr h, IntPtr c, MonEnum f, IntPtr d);
  [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr SetActiveWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

  public static string FgClass() {
    var c = new StringBuilder(256);
    GetClassName(GetForegroundWindow(), c, 256);
    return c.ToString();
  }

  // Taking the foreground from a background script is not something Windows
  // grants for the asking: SetForegroundWindow is refused unless the calling
  // thread already owns the foreground. Borrowing the current foreground
  // thread's input queue lifts that restriction for the duration.
  //
  // Without this the probe window never gains focus, the app correctly sees
  // whatever was focused before, and BOTH cases below report the wrong thing
  // -- the fullscreen case fails and the maximised case passes vacuously.
  public static bool Focus(IntPtr h) {
    uint us = GetCurrentThreadId();
    uint them = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
    if (them != 0 && them != us) AttachThreadInput(them, us, true);
    BringWindowToTop(h);
    SetForegroundWindow(h);
    SetActiveWindow(h);
    if (them != 0 && them != us) AttachThreadInput(them, us, false);
    return GetForegroundWindow() == h;
  }

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
        o.Add(string.Format("{0},{1},{2},{3}", mi.rcMonitor.L, mi.rcMonitor.T, mi.rcMonitor.R, mi.rcMonitor.B));
      return true;
    }, IntPtr.Zero);
    return o;
  }
}
"@

[void][F]::SetProcessDpiAwarenessContext([IntPtr](-4))

$u = Get-HcUnionRect ([F]::Rects())
if (-not $u) { Write-Host 'RESULT: FAIL - bogus union rect'; exit 1 }
$L = $u.L; $T = $u.T; $R = $u.R; $B = $u.B
Write-Host ("union rect: {0},{1} .. {2},{3}" -f $L, $T, $R, $B)

Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 600

if (-not (Backup-HcConfig $backup)) { Write-Host 'RESULT: SKIP - stale backup present'; exit 1 }
Remove-Item $marker -ErrorAction SilentlyContinue

# suppressFullscreen ON -- that is the whole point of this script.
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
  suppressFullscreen    = $true
  suppressWhileDragging = $true
  modifier              = 'none'
}

# Pumps the probe window's message queue as it goes. A form that never pumps
# is a window Windows treats as hung, which is not what we are trying to test.
function Hold($x, $y, $ms) {
  $n = [int]($ms / 100)
  for ($i = 0; $i -lt $n; $i++) {
    [void][F]::SetCursorPos($x, $y)
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 100
  }
}

# A window on the monitor the top-left outer corner belongs to, so the
# foreground window and the corner under test are on the same screen.
function New-TestWindow($mode) {
  $f = New-Object System.Windows.Forms.Form
  $f.StartPosition = 'Manual'
  $f.Text = 'Cantos fullscreen probe'
  $f.BackColor = [System.Drawing.Color]::FromArgb(20, 22, 30)
  $f.Location = New-Object System.Drawing.Point($L, $T)
  switch ($mode) {
    # How a borderless-fullscreen game presents: no frame, sized to the
    # monitor, not maximised.
    'fullscreen' {
      $f.FormBorderStyle = 'None'
      $f.WindowState = 'Normal'
      $f.Size = New-Object System.Drawing.Size(($R - $L), ($B - $T))
    }
    # An everyday application window.
    'maximised' {
      $f.FormBorderStyle = 'Sizable'
      $f.WindowState = 'Maximized'
    }
    # Borderless *and* maximised: covers the monitor exactly, reports
    # IsZoomed, and carries no frame -- so it must still be suppressed.
    'borderlessMaximised' {
      $f.FormBorderStyle = 'None'
      $f.WindowState = 'Maximized'
    }
    default { throw "unknown window mode '$mode'" }
  }
  # Topmost only where it is needed, to sit above the taskbar. The maximised
  # case deliberately goes without: the shell's own full-screen heuristic
  # keys partly on topmost-and-covering-the-monitor, and on a display whose
  # work area equals the monitor rect that could suppress the corner through
  # SHQueryUserNotificationState before this app's geometry check ever runs --
  # failing the test on exactly the hardware the fix targets. (Measured here,
  # a framed maximised window reports QUNS_ACCEPTS_NOTIFICATIONS either way,
  # so this is insurance rather than an observed problem.)
  $f.TopMost = ($mode -ne 'maximised')
  $f.Show()
  [System.Windows.Forms.Application]::DoEvents()

  # Retry: the foreground can be contested for a moment after Show().
  $got = $false
  for ($i = 0; $i -lt 10 -and -not $got; $i++) {
    $got = [F]::Focus($f.Handle)
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 150
  }
  if (-not $got) {
    Write-Host ("  could not take the foreground (it is '{0}')" -f [F]::FgClass()) -ForegroundColor Yellow
  }
  return @{ Form = $f; Focused = $got }
}

$orig = New-Object 'F+P'; [void][F]::GetCursorPos([ref]$orig)
$win = $null
$started = $false
$aborted = $false

try {
  Start-Process -FilePath $exe
  Start-Sleep -Seconds 2
  $started = $null -ne (Get-Process cantos -ErrorAction SilentlyContinue)
  if (-not $started) { throw 'process did not start' }

  # Runs one case: park away from the corner so the previous case's cooldown
  # lapses and the corner re-arms, then raise the window and park in it.
  function Invoke-Case($label, $mode, $want) {
    Write-Host ("--- {0} ---" -f $label)
    Remove-Item $marker -ErrorAction SilentlyContinue
    Hold ([int](($L + $R) / 2)) ([int](($T + $B) / 2)) 900
    $w = New-TestWindow $mode
    $script:win = $w.Form
    Hold ($L + 1) ($T + 1) 2000
    $fired = Test-Path $marker
    Write-Host ("  fired: {0} (want {1})" -f $fired, $want)
    $script:win.Close(); $script:win.Dispose(); $script:win = $null
    Start-Sleep -Milliseconds 800
    return @{ Fired = $fired; Focused = $w.Focused }
  }

  $r1 = Invoke-Case 'borderless window covering the monitor' 'fullscreen' 'False - suppressed'
  $r2 = Invoke-Case 'ordinary maximised window' 'maximised' 'True - maximised is not fullscreen'
  $r3 = Invoke-Case 'borderless AND maximised' 'borderlessMaximised' 'False - still suppressed'

  $firedFull = $r1.Fired; $firedMax = $r2.Fired; $firedBoth = $r3.Fired
  $focused   = $r1.Focused -and $r2.Focused -and $r3.Focused
}
catch {
  $aborted = $true
  Write-Host ("  aborted: {0}" -f $_.Exception.Message) -ForegroundColor Red
}
finally {
  if ($win) { $win.Close(); $win.Dispose() }
  [void][F]::SetCursorPos($orig.X, $orig.Y)
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
  Restore-HcConfig $backup
  Remove-Item $marker -ErrorAction SilentlyContinue
}

if (-not $started) { Write-Host 'RESULT: FAIL - process did not start'; exit 1 }
if ($aborted)      { Write-Host 'RESULT: FAIL - the run aborted before every case completed'; exit 1 }

Write-Host ''
# Every case hinges on the probe window actually being the foreground window.
# If it never got focus, none of the results mean anything -- and the maximised
# case would "pass" vacuously, which is worse than an honest skip.
if (-not $focused) {
  Write-Host 'RESULT: SKIP - the probe window could not take the foreground.'
  Write-Host '  Something is holding a foreground lock (a screen recorder, remote'
  Write-Host '  session, or an elevated window). Run again on an idle desktop.'
  exit 0
}
if (-not $firedFull -and $firedMax -and -not $firedBoth) { Write-Host 'RESULT: PASS'; exit 0 }
Write-Host ("RESULT: FAIL  (fullscreen={0} maximised={1} borderlessMaximised={2})" `
  -f $firedFull, $firedMax, $firedBoth)
exit 1
