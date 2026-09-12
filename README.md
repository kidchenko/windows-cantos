<div align="center">

<img src="assets/icon-256.png" width="112" height="112" alt="Cantos">

# Cantos

macOS-style hot corners for Windows.<br>
Throw the cursor into a corner of the screen and something happens.

<sub><i>canto</i> — Portuguese for <i>corner</i></sub>

</div>

A single 0.72 MB executable, no runtime dependencies. Sits in the tray using
about 2 MB and no measurable CPU.

---

## Install

```powershell
winget install kidchenko.Cantos
# or
choco install cantos
```

Or download from [Releases](https://github.com/kidchenko/windows-cantos/releases):
`Cantos-Setup-0.0.2.exe` to install, or `cantos-0.0.2-x64.exe` to run without
installing.

The binary is not code-signed, so SmartScreen warns on first run. Click
**More info**, then **Run anyway**.

## Actions

Twenty-one, plus anything you can launch yourself.

| | |
|---|---|
| **Windows & desktops** | Task View `Win+Tab` · Show desktop `Win+D` · Minimize all `Win+M` · Previous desktop `Ctrl+Win+Left` · Next desktop `Ctrl+Win+Right` |
| **Shell** | Start menu · Search `Win+S` · Settings `Win+I` · Quick Settings `Win+A` · Notifications `Win+N` · Quick Link menu `Win+X` · Run `Win+R` |
| **Tools** | File Explorer `Win+E` · Screenshot `Win+Shift+S` · Clipboard history `Win+V` · Emoji picker `Win+.` · Project / second screen `Win+P` |
| **Power & screen** | Lock · Screensaver · Turn display off |
| **Custom** | Any program, script, document, or URL, with arguments |

## Settings

Left-click the tray icon. The window shows your actual display layout and
marks which corners are live.

| Setting | Default | Notes |
|---|---|---|
| Dwell | 250 ms | How long the cursor rests before a corner fires. Stops overshooting toward a Close button from triggering it. |
| Cooldown | 700 ms | After firing, the corner needs both this delay **and** the cursor to leave before it can fire again. |
| Corner size | 8 px | Edge length of the hot square. |
| Hold a key | off | Require Ctrl, Alt, Shift, or Win. |
| Pause in fullscreen | on | No corner fires over a game or video. A maximised window is not fullscreen and still works. |
| Pause while dragging | on | Dragging a window into a corner stays a Windows Snap. |
| Start with Windows | off | Per-user `Run` key. No scheduled task, no admin. |

**Multiple monitors.** Three modes: *Outer* (the four corners of the whole
desktop, default), *Every screen*, or *Primary only*. On L-shaped layouts an
outer corner that isn't on any physical display is dropped rather than
published as a corner that can never fire.

Settings live in `%APPDATA%\Cantos\config.json`. Edit it by hand if you like;
changes apply within a second, no restart.

## Troubleshooting

Cantos logs to `%APPDATA%\Cantos\cantos.log`, reachable from the tray menu via
**Open log** even when the settings window won't open. It records every
decision that can make a corner do nothing.

**A corner does nothing.** Look for the `live corners` line. It lists the pixel
rectangles that are hot right now:

```
INFO  live corners: OuterCorners across 1 monitor(s): TopLeft[0,0..8,8] ...
INFO  TopLeft fired: TaskView
INFO  TopRight suppressed: a mouse button is held (window drag / snap)
```

For per-transition detail, set `CANTOS_LOG=debug` before launching.

**Nothing happens while an admin app is focused.** Windows blocks synthetic
keystrokes aimed at a higher-privilege window. That is UIPI, a security
boundary, not a bug. Lock, Screensaver, Turn display off, and Custom command
still work, since none of them synthesises input. Running Cantos elevated also
fixes it, at the cost of admin rights it otherwise doesn't need.

**The settings window won't open.** It needs the Microsoft WebView2 runtime.
Windows 11 ships it; on Windows 10 it arrives with Edge. If it's missing,
Cantos says so and offers the download page. Hot corners keep working, and you
can still edit `config.json` by hand.

**Launching Cantos again does nothing visible.** A second launch tells the
first to show its settings, then exits. A brief flicker is expected.

## Build

```powershell
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-msvc
choco install innosetup -y     # only for the installer
```

Open a new terminal afterwards, or `cargo` won't be on PATH.

```powershell
.\build.ps1                 # binary + installer
.\build.ps1 -Run            # ...then launch it
.\build.ps1 -SkipInstaller  # binary only
.\build.ps1 -Test           # ...then run the full test suite (needs admin)
```

Outputs `target\release\cantos.exe` and `dist\Cantos-Setup-0.0.2.exe`.

If `cargo build` fails on `link.exe`, install the MSVC C++ workload:

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

## Tests

```powershell
cargo test                                              # corner rules
powershell -ExecutionPolicy Bypass -File tests\e2e.ps1  # corner detection
```

The integration scripts drive the real cursor, so run them on an idle machine.
See [`tests/README.md`](tests/README.md).

## How it works

Polls `GetCursorPos` at ~33 Hz instead of installing a `WH_MOUSE_LL` hook. A
low-level hook runs inline on the input thread for every mouse move
system-wide, and Windows silently unhooks it if the callback overruns. Polling
costs a few hundred nanoseconds a tick and can't be disabled behind your back.

The process is manifested `PerMonitorV2`, so cursor and monitor coordinates
share one physical-pixel space.

Start with `src/trigger.rs`. It holds the corner rules and nothing else: no
Win32 handles, no mutex, no actions. It takes a hot spot plus the current
settings and answers "fire this corner" or "not yet", which is what makes the
rules testable.

## Licence

MIT
