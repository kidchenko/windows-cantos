<div align="center">

<img src="assets/icon-256.png" width="112" height="112" alt="Cantos">

# Cantos

macOS-style hot corners for Windows.<br>
Throw the cursor into a corner of the screen and something happens.

<sub><i>canto</i> — Portuguese for <i>corner</i></sub>

</div>

A single 0.72 MB executable with no runtime dependencies. It sits in the tray
using about 2 MB and effectively no CPU.

---

## Install

```powershell
choco install cantos
# or
winget install kidchenko.Cantos
```

Or grab `Cantos-Setup.exe` (or the portable `cantos.exe`) from
[Releases](https://github.com/kidchenko/windows-cantos/releases).

## Actions

Twenty-one, grouped in the picker so a list this long stays scannable.

| | |
|---|---|
| **Windows & desktops** | Task View `Win+Tab` · Show desktop `Win+D` · Minimize all `Win+M` · Previous desktop `Ctrl+Win+Left` · Next desktop `Ctrl+Win+Right` |
| **Shell** | Start menu · Search `Win+S` · Settings `Win+I` · Quick Settings `Win+A` · Notifications `Win+N` · Quick Link menu `Win+X` · Run `Win+R` |
| **Tools** | File Explorer `Win+E` · Screenshot `Win+Shift+S` · Clipboard history `Win+V` · Emoji picker `Win+.` · Project / second screen `Win+P` |
| **Power & screen** | Lock · Screensaver · Turn display off |
| **Custom** | Any program, script, document, or URL, with arguments |

Start menu sends `Ctrl+Esc` rather than tapping the Windows key alone: the
shell ignores a bare `Win` tap if another key was pressed recently, and this
app has just synthesised modifier releases (see "Hold a key" below).

## Behaviour worth knowing about

**Dwell.** A corner does not fire the instant you touch it. The cursor has to
rest there for a configurable delay (250 ms by default). This is the
difference between a hot corner and a booby trap — without it, overshooting
toward a window's Close button triggers the corner every time.

**Cooldown, and leaving.** After a corner fires it stays disarmed until *both*
the cooldown elapses **and** the cursor leaves the corner. Both conditions,
not either — otherwise parking the mouse in the corner would fire the action
on repeat.

**Fullscreen suppression.** On by default. Firing Task View in the middle of a
game is the worst thing this app could do, so it checks two independent
signals: the shell notification state (which catches exclusive-mode D3D and
presentation mode) and whether the foreground window covers its monitor (which
catches borderless-windowed games and every video player).

An ordinary *maximised* window is explicitly not fullscreen, and measuring it
cannot establish that. `GetWindowRect` reports the frame including the
invisible resize border, so a maximised window overhangs its monitor by ~8px
on every edge it can grow into — and wherever the work area equals the monitor
rect (a second display with no taskbar, which is the Windows 11 default, or an
auto-hide taskbar) that overhang alone used to read as fullscreen, silencing
every corner on every display.

So a window is exempted only when it is **both** maximised and framed — it has
a caption or a resize border. A borderless-fullscreen app has neither, so it is
still caught by the geometry even if it presents itself maximised.

**Dragging.** On by default, and the reason it exists is Windows Snap:
dragging a window into a corner is how you snap it to a quarter of the screen,
and the corner must not also fire. So no corner fires while any mouse button
is held — which also covers dragging files and rubber-band selection.

Crucially it *disarms* rather than waits. If it merely waited for the button,
the corner would fire at the exact moment you released to complete the snap,
and you would get the snap **and** the action. Instead you have to leave the
corner and come back.

**Multiple monitors.** Three modes:

- **Outer** *(default)* — only the four corners of the whole virtual desktop.
  Matches how macOS feels, and moving between screens never clips a corner at
  the shared edge. On L-shaped layouts, a corner of the bounding box that
  isn't on any physical display is dropped rather than published as a corner
  that can never fire.
- **Every screen** — all four corners of every display.
- **Primary** — only the primary display; the rest stay inert.

**Hold a key.** Optionally require Ctrl, Alt, Shift, or Win. The app releases
any modifier you are physically holding before synthesising its own shortcut,
so `Win`+`Tab` arrives as `Win`+`Tab` and not `Ctrl`+`Win`+`Tab`.

## Footprint

Measured on Windows 11, release build:

| | Processes | Working set | Private |
|---|---|---|---|
| Tray, idle | 1 | ~11 MB | **~2 MB** |
| Settings window open | 7 | ~400 MB | ~208 MB |
| Settings closed again | 1 | ~22 MB | ~3.6 MB |

The settings window is a WebView2 control, which means a real Chromium
instance for as long as it is on screen. It is torn down completely when you
close the window — that teardown is what keeps the idle number honest, and it
is why the environment is deliberately *not* kept warm between openings.
Reopening costs a few hundred milliseconds; holding 200 MB all day to avoid
that would defeat the point of the app.

## Code layout

Roughly in the order a corner press flows through it:

| module | responsibility |
|---|---|
| `watcher` | the polling thread, and its three timers |
| `geometry` | monitors, and the corner hitboxes derived from them |
| `trigger` | **the rules** — dwell, cooldown, and the vetoes |
| `desktop` | asks Win32 what is happening right now (keys, buttons, fullscreen) |
| `actions` | what a corner does once it fires |
| `shell` | asks Explorer to do things, when synthesised keys would be discarded |
| `app` | the tray icon, its menu, and the settings window |
| `ui` | the settings window; `ui::page` is its protocol |
| `config` | the settings file, and watching it for external edits |

`trigger` is the one to read first — it is where "does this feel solid or
twitchy" is decided. It deliberately holds no Win32 handles, touches no mutex,
and runs no actions: it is handed a hot spot and the current settings, and
answers "fire this corner" or "not yet". That is what makes the rules testable,
since nothing is ever held down or fullscreen during `cargo test`; `desktop` is
a trait so the tests can answer for it.

## How it works

Corner detection polls `GetCursorPos` at ~33 Hz rather than installing a
`WH_MOUSE_LL` hook. A low-level hook runs inline on the input thread for every
mouse move system-wide, so any latency it adds is latency the whole desktop
feels — and if the callback overruns `LowLevelHooksTimeout`, Windows silently
unhooks it and the app just appears to stop working. Polling costs a few
hundred nanoseconds per tick, cannot be silently disabled, and is plenty when
the trigger is gated behind a 250 ms dwell anyway.

The process is manifested `PerMonitorV2`, so cursor coordinates and monitor
rectangles share one physical-pixel space and no DPI scaling is needed
anywhere.

Settings live in `%APPDATA%\Cantos\config.json`, written via a temp file
and rename so an interrupted save cannot corrupt them. The file is also
watched: edit it by hand and the change is picked up within a second, so an
external edit takes effect immediately rather than being ignored until the
next restart and then overwritten by the next save. A file that is missing or
mid-save is left alone rather than resetting anything.

Run-at-login is the one setting that is *not* in there. It lives in
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and nowhere else, so the
Startup tab in Task Manager and the settings window can never disagree.

## Troubleshooting

Cantos writes a log to `%APPDATA%\Cantos\cantos.log`. Open it from
the tray menu -> **Open log**, which is deliberately reachable even when the
settings window is the thing refusing to appear.

It records every decision that can make a corner do nothing, because from the
outside they all look identical:

```
INFO  --- Cantos 0.1.0 starting (pid 40332) ---
INFO  exe: C:\Program Files\Cantos\cantos.exe
INFO  config loaded: enabled=true corners=[TaskView, ShowDesktop, None, None] dwell=250ms ...
INFO  live corners: OuterCorners across 1 monitor(s): TopLeft[0,0..8,8] TopRight[1912,0..1920,8] ...
INFO  TopLeft fired: TaskView
INFO  TopRight suppressed: a mouse button is held (window drag / snap)
INFO  BottomLeft suppressed: something is fullscreen
```

`live corners` is usually the answer: it lists the exact pixel rectangles that
are hot right now. If a corner you expect is missing, it was dropped for not
being on a physical display (see the multi-monitor notes above).

For per-transition detail, start it with `CANTOS_LOG=debug`:

```powershell
$env:CANTOS_LOG = 'debug'
& "C:\Program Files\Cantos\cantos.exe"
```

The log rotates at 1 MB, keeping one previous generation as `cantos.log.1`.

**The settings window will not open.**

Cantos's settings window is a WebView2 control, so it needs the Microsoft
WebView2 runtime. Windows 11 ships it. On Windows 10 it arrives with Edge,
which covers almost every machine — but LTSC and N editions, and locked-down
enterprise images, can be without it.

If it is missing, the app tells you so in a dialog and offers the download
page. Hot corners keep working either way; only the settings window is
affected, and you can still edit `%APPDATA%\Cantos\config.json` by hand —
changes there are picked up within a second, no restart needed.

Every log starts with the version it found, so this is answerable without
reproducing anything:

```
INFO  webview2: 152.0.4191.66
WARN  webview2: NOT INSTALLED — the settings window cannot open
```

**A corner does nothing while a particular app is focused.**

Windows refuses synthetic keystrokes sent from a normal-privilege process to a
window owned by a process running **as administrator**. That is User Interface
Privilege Isolation, and it is a security boundary, not a bug. If you keep an
elevated terminal or editor focused, every keyboard-driven action (Task View,
Show desktop, Search, and so on) silently does nothing while it has focus.

The log names it explicitly:

```
WARN  Windows blocked the keystroke (UIPI). The focused window belongs to a
      process running as administrator, and Cantos does not.
```

It is easy to misread as "hot corners only work when the settings window is
open", because opening Settings makes Cantos's own window the foreground
window — same process, same privilege level — so the keystroke lands.

Three ways around it:

- **Use the actions that do not synthesise keystrokes.** Lock, Screensaver,
  Turn display off, and Custom command all work regardless, because none of
  them goes through `SendInput`.
- **Run Cantos elevated too**, via a Task Scheduler task at logon with
  "run with highest privileges". Everything then works, at the cost of the
  app holding admin rights it does not otherwise need.
- **Do nothing.** If you do not routinely run elevated apps — which is most
  people — you will never encounter this.

The app is manifested `asInvoker` on purpose: a hot-corner utility should not
demand admin rights, and an elevated process cannot send input to
unelevated windows either, so elevating by default would just invert the
problem.

**Launching the app again does not open a window.** That is a second instance
telling the first to show its settings, then exiting -- a brief flicker is
expected. If no window appears, the log will say either
`relaunch signal received` (the first instance heard it) or
`could not open the show-settings event` (it did not).

## Build

**Prerequisites** (one time):

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-msvc
choco install innosetup -y          # only needed for the installer
```

Rust also needs the MSVC linker. If `cargo build` fails with a `link.exe`
error, install the C++ workload:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

**Build:**

```powershell
.\build.ps1                 # binary + installer
.\build.ps1 -Run            # ... then launch it
.\build.ps1 -SkipInstaller  # binary only
.\build.ps1 -Test           # ... then run the whole test suite (needs admin)
```

Outputs:

| | |
|---|---|
| `target\release\cantos.exe` | 0.72 MB, portable, no dependencies |
| `dist\Cantos-Setup-0.1.0.exe` | 2.25 MB installer |

Open a **new** terminal after installing Rust — `cargo` will not be on the
PATH of a shell that was already running. (`build.ps1` falls back to
`%USERPROFILE%\.cargo\bin` if it has to, and finds Inno Setup the same way,
since Inno never adds itself to the PATH.)

To test the installer specifically, run an **elevated** PowerShell:

```powershell
.\build.ps1
powershell -ExecutionPolicy Bypass -File tests\installer.ps1
```

That installs silently, checks the files, uninstall entry, Start Menu
shortcut and running process, then uninstalls and verifies the cleanup. It
refuses to run if Cantos is already installed, so uninstall any existing
copy first.

Or just double-click `dist\Cantos-Setup-0.1.0.exe`.

The release profile optimises hard for size (`opt-level = "z"`, LTO,
`codegen-units = 1`, `panic = "abort"`, stripped), which is what keeps a
Chromium-hosting app under a megabyte.

## Tests

```powershell
cargo test                                              # state machine
powershell -ExecutionPolicy Bypass -File tests\e2e.ps1  # corner detection
```

See [`tests/README.md`](tests/README.md). The integration scripts drive the
real cursor, so run them on an idle machine.

## Licence

MIT
