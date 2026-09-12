//! Persisted user settings, stored as JSON in %APPDATA%\Cantos\config.json.

use crate::log::{lerror, linfo};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub fn index(self) -> usize {
        match self {
            Corner::TopLeft => 0,
            Corner::TopRight => 1,
            Corner::BottomLeft => 2,
            Corner::BottomRight => 3,
        }
    }
}

/// What a corner does when it fires.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Action {
    /// Corner is inert.
    None,
    // -- Windows and desktops ------------------------------------------
    /// Win+Tab — the closest Windows analogue to Mission Control.
    TaskView,
    /// Win+D.
    ShowDesktop,
    /// Win+M. Unlike Show desktop this does not toggle back.
    MinimizeAll,
    /// Ctrl+Win+Left.
    DesktopLeft,
    /// Ctrl+Win+Right.
    DesktopRight,

    // -- Shell ----------------------------------------------------------
    /// Ctrl+Esc. Used in preference to tapping the Windows key on its own,
    /// which the shell ignores if anything else was pressed recently.
    StartMenu,
    /// Win+S.
    Search,
    /// Win+I.
    Settings,
    /// Win+A — Quick Settings on Windows 11, Action Center on 10.
    QuickSettings,
    /// Win+N — Notification Centre.
    Notifications,
    /// Win+X — the power-user menu on the Start button.
    QuickLink,
    /// Win+R.
    RunDialog,

    // -- Tools ----------------------------------------------------------
    /// Win+E.
    FileExplorer,
    /// Win+Shift+S — the screenshot region overlay.
    Screenshot,
    /// Win+V — clipboard history.
    Clipboard,
    /// Win+. — emoji and symbol picker.
    Emoji,
    /// Win+P — second-screen / projection options.
    ProjectDisplay,

    // -- Power and screen -----------------------------------------------
    /// Immediately locks the workstation.
    LockWorkstation,
    /// Kicks off the configured screensaver.
    Screensaver,
    /// Powers the display down; any input wakes it.
    DisplayOff,
    /// Runs an arbitrary executable, script, or URL.
    #[serde(rename_all = "camelCase")]
    Custom {
        command: String,
        #[serde(default)]
        args: String,
    },
}

impl Action {
    pub fn is_none(&self) -> bool {
        matches!(self, Action::None)
    }
}

/// Which physical corners are live.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum MonitorMode {
    /// Only the four corners of the whole virtual desktop. Matches how macOS
    /// feels and, crucially, means moving between monitors never clips a
    /// hot corner at the shared edge.
    OuterCorners,
    /// All four corners of every display.
    EveryMonitor,
    /// Only the primary display; every other screen is inert.
    PrimaryOnly,
}

/// An optional key that must be held for a corner to fire, mirroring the
/// modifier support in macOS hot corners.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Modifier {
    None,
    Ctrl,
    Alt,
    Shift,
    Win,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Master switch, toggled from the tray.
    pub enabled: bool,
    /// Indexed by `Corner::index()`.
    pub corners: [Action; 4],
    /// How long the cursor must rest in a corner before it fires. The whole
    /// point is to not trigger when you overshoot toward the Close button.
    pub dwell_ms: u32,
    /// After firing, the corner stays disarmed for at least this long *and*
    /// until the cursor leaves it. Both conditions, not either.
    pub cooldown_ms: u32,
    /// Edge length of the square hitbox at each corner, in physical pixels.
    pub corner_size: i32,
    pub monitor_mode: MonitorMode,
    /// Suppress while a game or video is fullscreen. Firing Task View mid-game
    /// is the single worst thing this app could do, so this defaults on.
    pub suppress_fullscreen: bool,
    /// Suppress while a mouse button is held. Dragging a window into a corner
    /// is how Windows Snap works, and it would otherwise trigger the corner
    /// too; the same applies to dragging files and selecting text.
    pub suppress_while_dragging: bool,
    pub modifier: Modifier,
    // Run-at-login is deliberately absent. It lives in the HKCU Run key and
    // nowhere else: mirroring it here gave us a field that was written but
    // never read, so hand-editing it in this file appeared to do something
    // and did not. `autostart::is_enabled()` is the only source of truth.
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            // Default to the two top corners. The bottom corners are left
            // inert on purpose: bottom-left is the Start button and
            // bottom-right is Aero Peek, so users already have muscle memory
            // for throwing the cursor there.
            corners: [
                Action::TaskView,    // top-left
                Action::ShowDesktop, // top-right
                Action::None,        // bottom-left
                Action::None,        // bottom-right
            ],
            dwell_ms: 250,
            cooldown_ms: 700,
            corner_size: 8,
            monitor_mode: MonitorMode::OuterCorners,
            suppress_fullscreen: true,
            suppress_while_dragging: true,
            modifier: Modifier::None,
        }
    }
}

impl Config {
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Cantos"))
    }

    pub fn path() -> Option<PathBuf> {
        Self::dir().map(|d| d.join("config.json"))
    }

    /// Last-modified time of the config file, or `None` if it is not there.
    ///
    /// Used by the watcher to notice a hand edit. Cheap enough to call on a
    /// one-second timer; the file is parsed only when this value moves.
    pub fn modified() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()?).ok()?.modified().ok()
    }

    /// Read and parse the file, without logging or falling back to defaults.
    ///
    /// `None` covers both "not there" and "could not be parsed". Callers that
    /// are reacting to a change on disk want to leave the running config
    /// alone in either case — an editor caught mid-save must not reset
    /// anyone's settings.
    pub fn reload() -> Option<Self> {
        let text = std::fs::read_to_string(Self::path()?).ok()?;
        Self::parse(&text).ok()
    }

    fn parse(text: &str) -> Result<Self, serde_json::Error> {
        // Strip a UTF-8 BOM. serde_json treats one as a syntax error, and
        // anyone who edits this file in Notepad or writes it from Windows
        // PowerShell will have one — silently resetting their settings would
        // be a baffling way to greet them.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        serde_json::from_str::<Config>(text).map(Config::sanitised)
    }

    /// Never fails: a missing, unreadable, or corrupt file just yields
    /// defaults. A tray app that refuses to start because its config got
    /// truncated by a hard power-off would be worse than one that resets.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            linfo!("no config at {}; using defaults", path.display());
            return Self::default();
        };
        match Self::parse(&text) {
            Ok(c) => {
                linfo!(
                    "config loaded: enabled={} corners={:?} dwell={}ms size={}px mode={:?} fullscreen={} dragging={} modifier={:?}",
                    c.enabled, c.corners, c.dwell_ms, c.corner_size,
                    c.monitor_mode, c.suppress_fullscreen, c.suppress_while_dragging, c.modifier
                );
                c
            }
            Err(e) => {
                lerror!(
                    "config at {} is unreadable ({e}); falling back to defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Write via a temp file + rename so an interrupted save can never leave
    /// a half-written config behind.
    pub fn save(&self) -> std::io::Result<()> {
        let (Some(dir), Some(path)) = (Self::dir(), Self::path()) else {
            return Err(std::io::Error::other("no APPDATA in environment"));
        };
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    /// Clamp anything a hand-edited config could set to a hostile value.
    /// `corner_size` of 0 would make corners unreachable; a huge one would
    /// make a third of the screen hot.
    pub fn sanitised(mut self) -> Self {
        self.corner_size = self.corner_size.clamp(1, 200);
        self.dwell_ms = self.dwell_ms.min(5_000);
        self.cooldown_ms = self.cooldown_ms.clamp(100, 10_000);
        self
    }
}
