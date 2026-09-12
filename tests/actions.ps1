# Verifies the built-in actions actually fire, end to end: corner -> dwell ->
# SendInput -> the shell reacting.
#
# Every other test in this directory uses the Custom action because it is
# observable via a marker file. That deliberately avoids the SendInput key
# synthesis path, which is what Task View and Show desktop actually use --
# so this covers the app's headline feature.
#
# Only the two reversible actions are exercised. Lock, Screensaver and
# Display off are all trivially the same dispatch path but would leave the
# machine locked or dark, so they are checked by inspection, not by firing.
#
# The app is deliberately started at MEDIUM integrity (see StartWith). That is
# the only configuration in which this test means anything: at HIGH integrity
# synthesised input reaches windows it normally cannot, so every action
# appears to work whether or not it really would for a user.

$ErrorActionPreference = 'Continue'
. "$PSScriptRoot\common.ps1"

$proj   = Split-Path $PSScriptRoot -Parent
$cfg    = Get-HcConfigPath
$backup = Join-Path $PSScriptRoot 'config.actbackup.json'
$exe    = Join-Path $proj 'target\release\cantos.exe'

Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class A {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out P p);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int m);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, IntPtr e);
  [DllImport("user32.dll")] public static extern bool EnumDisplayMonitors(IntPtr h, IntPtr c, MonEnum f, IntPtr d);
  [DllImport("user32.dll")] public static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint a, bool inh, uint pid);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
  [DllImport("advapi32.dll")] public static extern bool OpenProcessToken(IntPtr p, uint acc, out IntPtr tok);
  [DllImport("advapi32.dll")] public static extern bool GetTokenInformation(IntPtr tok, int cls, out uint info, uint len, out uint ret);

  // Is the window that currently has focus owned by an elevated process?
  //
  // This decides which dispatch path the app takes, and therefore whether
  // this test means anything -- see the SKIP below.
  public static bool ForegroundIsElevated() {
    uint pid;
    GetWindowThreadProcessId(GetForegroundWindow(), out pid);
    if (pid == 0) return false;
    IntPtr p = OpenProcess(0x1000 /* QUERY_LIMITED_INFORMATION */, false, pid);
    if (p == IntPtr.Zero) return false;       // cannot open it => almost certainly outranks us
    IntPtr tok;
    bool elevated = false;
    if (OpenProcessToken(p, 0x0008 /* TOKEN_QUERY */, out tok)) {
      uint val, ret;
      if (GetTokenInformation(tok, 20 /* TokenElevation */, out val, 4, out ret)) elevated = val != 0;
      CloseHandle(tok);
    }
    CloseHandle(p);
    return elevated;
  }

  [StructLayout(LayoutKind.Sequential)] public struct P { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [StructLayout(LayoutKind.Sequential)] public struct MONITORINFO {
    public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags;
  }
  public delegate bool MonEnum(IntPtr m, IntPtr hdc, IntPtr r, IntPtr d);

  public static string Fg() {
    IntPtr h = GetForegroundWindow();
    var c = new StringBuilder(256); GetClassName(h, c, 256);
    var t = new StringBuilder(256); GetWindowText(h, t, 256);
    return c.ToString() + "|" + t.ToString();
  }
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

[void][A]::SetProcessDpiAwarenessContext([IntPtr](-4))

$L = [int]::MaxValue; $T = [int]::MaxValue; $R = [int]::MinValue; $B = [int]::MinValue
foreach ($line in [A]::Rects()) {
  $n = $line -split ','
  $L = [Math]::Min($L, [int]$n[0]); $T = [Math]::Min($T, [int]$n[1])
  $R = [Math]::Max($R, [int]$n[2]); $B = [Math]::Max($B, [int]$n[3])
}
if ($R -le $L -or $B -le $T) { Write-Host "RESULT: FAIL - bogus union rect"; exit 1 }
Write-Host ("union rect: {0},{1} .. {2},{3}" -f $L, $T, $R, $B)

# This test only means something when synthesised input can actually reach the
# foreground window. Run it from an ELEVATED shell and that shell's own window
# is the foreground one, Cantos (deliberately launched at medium integrity
# below) cannot send it keystrokes, and every action falls back to shell
# automation. Task View survives that -- IShellDispatch5::WindowSwitcher really
# works -- but ToggleDesktop reports success and does nothing, so Show desktop
# reads as a failure when the app behaved exactly as designed.
#
# That is a real, documented limitation, not something this test can assert on.
# Skip rather than cry wolf.
if ([A]::ForegroundIsElevated()) {
  Write-Host ''
  Write-Host 'RESULT: SKIP - an elevated window has focus.' -ForegroundColor Yellow
  Write-Host '  Windows discards synthesised input aimed at a higher-integrity window,'
  Write-Host '  so the actions under test would take the shell-automation fallback and'
  Write-Host '  this run could not tell a real failure from that fallback.'
  Write-Host '  Re-run from a NON-elevated PowerShell, with no elevated window focused.'
  exit 0
}

if (-not (Backup-HcConfig $backup)) { Write-Host 'RESULT: SKIP - stale backup present'; exit 1 }

# Set if an elevated window ever holds the foreground while a case is running.
#
# The check above runs once, at startup, and that is not enough: focus drifts.
# Launch this from an elevated shell and the *first* case can pass with a
# non-elevated window focused, while by the second the foreground has fallen
# back to the elevated terminal that started the run -- at which point the app
# correctly takes the shell-automation fallback, ToggleDesktop silently does
# nothing, and a correct build reports RESULT: FAIL. Sampling throughout is
# what makes the verdict trustworthy.
$script:sawElevated = $false

function Hold($x, $y, $ms) {
  $n = [int]($ms / 100)
  for ($i = 0; $i -lt $n; $i++) {
    [void][A]::SetCursorPos($x, $y)
    if ([A]::ForegroundIsElevated()) { $script:sawElevated = $true }
    Start-Sleep -Milliseconds 100
  }
}

function StartWith($kind) {
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 600
  # A fresh launch per action. The app does now pick config changes up from
  # disk within a second, but restarting keeps each case independent of
  # whatever the previous one left in memory.
  Write-HcConfig @{
    enabled = $true
    corners = @(@{ kind = $kind }, @{ kind = 'none' }, @{ kind = 'none' }, @{ kind = 'none' })
    dwellMs = 150; cooldownMs = 700; cornerSize = 12
    monitorMode = 'outerCorners'
    suppressFullscreen = $false; suppressWhileDragging = $true
    modifier = 'none'
  }
  # Via Explorer, so the app runs at MEDIUM integrity like it does for a real
  # user. Launching it directly from an elevated shell gives it HIGH
  # integrity, which lets synthesised input reach elevated windows it could
  # not otherwise touch -- a false pass that hid a real limitation for an
  # entire development session.
  Start-Process explorer.exe -ArgumentList "`"$exe`""
  Start-Sleep -Seconds 3
}

$orig = New-Object 'A+P'; [void][A]::GetCursorPos([ref]$orig)
$VK_ESC = 0x1B; $VK_LWIN = 0x5B; $VK_D = 0x44; $KEYUP = 2

function WinD {
  [A]::keybd_event($VK_LWIN, 0, 0, [IntPtr]::Zero)
  [A]::keybd_event($VK_D, 0, 0, [IntPtr]::Zero)
  [A]::keybd_event($VK_D, 0, $KEYUP, [IntPtr]::Zero)
  [A]::keybd_event($VK_LWIN, 0, $KEYUP, [IntPtr]::Zero)
}

function OnDesktop { return ([A]::Fg() -match '^(WorkerW|Progman)\|') }

# Win+D is a toggle, so the test has to start from a known state. Without
# this, firing the corner while the desktop is already showing *restores*
# windows and the assertion reads as a failure when the action worked fine.
function EnsureWindowsShown {
  for ($i = 0; $i -lt 3; $i++) {
    if (-not (OnDesktop)) { return $true }
    WinD; Start-Sleep -Milliseconds 900
  }
  return (-not (OnDesktop))
}

$aborted = $false
try {
  # ---------------------------------------------------------------- Task View
  Write-Host "--- Task View (Win+Tab) ---"
  StartWith 'taskView'
  Hold $([int](($L+$R)/2)) $([int](($T+$B)/2)) 600
  $before = [A]::Fg()
  Write-Host ("  foreground before: {0}" -f $before)
  Hold ($L + 1) ($T + 1) 2000
  $after = [A]::Fg()
  Write-Host ("  foreground after:  {0}" -f $after)
  $taskView = ($after -ne $before) -and
              ($after -match 'MultitaskingViewFrame|XamlExplorerHostIslandWindow|Windows\.UI\.Core\.CoreWindow|Task View')
  Write-Host ("  Task View opened: {0}" -f $taskView)
  # Close it again.
  [A]::keybd_event($VK_ESC, 0, 0, [IntPtr]::Zero)
  [A]::keybd_event($VK_ESC, 0, $KEYUP, [IntPtr]::Zero)
  Start-Sleep -Milliseconds 1200

  # ------------------------------------------------------------- Show desktop
  Write-Host "--- Show desktop (Win+D) ---"
  StartWith 'showDesktop'
  Hold $([int](($L+$R)/2)) $([int](($T+$B)/2)) 600
  if (-not (EnsureWindowsShown)) { Write-Host "  (could not get off the desktop; skipping)" }
  $before2 = [A]::Fg()
  Write-Host ("  foreground before: {0}" -f $before2)
  Hold ($L + 1) ($T + 1) 2000
  $after2 = [A]::Fg()
  Write-Host ("  foreground after:  {0}" -f $after2)
  $showDesktop = $after2 -match '^(WorkerW|Progman)\|'
  Write-Host ("  desktop shown: {0}" -f $showDesktop)
}
catch {
  $aborted = $true
  Write-Host ("  aborted: {0}" -f $_.Exception.Message) -ForegroundColor Red
}
finally {
  # Order matters. The cursor is still parked in a hot corner here, and
  # EnsureWindowsShown sleeps for up to ~2.7s -- long enough for the corner to
  # fire again mid-cleanup and toggle the desktop straight back. Leave the
  # corner and stop the app first, then tidy the desktop.
  [void][A]::SetCursorPos($orig.X, $orig.Y)
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 400
  # Never leave the user staring at a bare desktop.
  [void](EnsureWindowsShown)
  Write-Host ("  windows restored, foreground now: {0}" -f [A]::Fg())
  Restore-HcConfig $backup
}

Write-Host ""
if ($aborted) { Write-Host 'RESULT: FAIL - the run aborted before every case completed'; exit 1 }
if ($script:sawElevated) {
  Write-Host 'RESULT: SKIP - an elevated window took the foreground mid-run.' -ForegroundColor Yellow
  Write-Host '  Windows discards synthesised input aimed at a higher-integrity window,'
  Write-Host '  so from that point the actions took the shell-automation fallback and'
  Write-Host '  this run cannot tell a real failure from that fallback.'
  Write-Host ("  (taskView={0} showDesktop={1} - not trustworthy)" -f $taskView, $showDesktop)
  Write-Host '  Re-run from a NON-elevated PowerShell, with no elevated window focused.'
  exit 0
}
if ($taskView -and $showDesktop) { Write-Host "RESULT: PASS - both SendInput actions fired"; exit 0 }
Write-Host ("RESULT: FAIL  (taskView={0} showDesktop={1})" -f $taskView, $showDesktop)
exit 1
