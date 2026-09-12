//! Minimal file logging to `%APPDATA%\Cantos\cantos.log`.
//!
//! Under `windows_subsystem = "windows"` there is no console, so `eprintln!`
//! goes nowhere. Without a log, "my corner didn't fire" is unanswerable — the
//! user cannot tell whether it was fullscreen suppression, the drag rule, a
//! hitbox that does not cover where they pointed, or the app not running at
//! all. Every one of those looks identical from the outside.
//!
//! Deliberately hand-rolled rather than pulling in `log` + a backend: this
//! needs about sixty lines and the binary is a stated constraint.
//!
//! Volume is kept low enough to leave on permanently — the watcher polls 33
//! times a second but only *outcomes* are recorded, never ticks. Set
//! `CANTOS_LOG=debug` for the chatty per-transition detail.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::System::SystemInformation::GetLocalTime;

static SINK: Mutex<Option<File>> = Mutex::new(None);
static DEBUG: AtomicBool = AtomicBool::new(false);

/// Rotate at 1 MB. A tray app can run for months, and an unbounded log on
/// someone else's disk is not our call to make.
const MAX_BYTES: u64 = 1_000_000;

pub fn path() -> Option<PathBuf> {
    crate::config::Config::dir().map(|d| d.join("cantos.log"))
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

pub fn init() {
    DEBUG.store(
        std::env::var("CANTOS_LOG")
            .map(|v| v.eq_ignore_ascii_case("debug"))
            .unwrap_or(false),
        Ordering::Relaxed,
    );

    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Keep one previous generation, so a crash loop cannot erase the run that
    // actually explains it.
    if std::fs::metadata(&p)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&p, p.with_extension("log.1"));
    }
    if let Ok(f) = OpenOptions::new().create(true).append(true).open(&p) {
        if let Ok(mut sink) = SINK.lock() {
            *sink = Some(f);
        }
    }
}

fn stamp() -> String {
    let t: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

pub fn write(level: &str, msg: &str) {
    // Formatted up front so the line reaches the file in a single write.
    // `writeln!` against a `File` issues one write per format fragment, and
    // every instance has this file open in append mode at once — so a second
    // launch signalling the first used to interleave mid-line, which is
    // precisely the moment the log is worth reading. Building the string
    // before taking the lock also keeps the critical section to the write.
    let line = format!("{} {:<5} {}\n", stamp(), level, msg);
    let Ok(mut sink) = SINK.lock() else { return };
    if let Some(f) = sink.as_mut() {
        // A failed write is ignored on purpose: logging must never be the
        // reason the app misbehaves.
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

macro_rules! linfo  { ($($a:tt)*) => { $crate::log::write("INFO",  &format!($($a)*)) } }
macro_rules! lwarn  { ($($a:tt)*) => { $crate::log::write("WARN",  &format!($($a)*)) } }
macro_rules! lerror { ($($a:tt)*) => { $crate::log::write("ERROR", &format!($($a)*)) } }
/// Only emitted when `CANTOS_LOG=debug`; the argument expression is not
/// even evaluated otherwise.
macro_rules! ldebug {
    ($($a:tt)*) => {
        if $crate::log::debug_enabled() {
            $crate::log::write("DEBUG", &format!($($a)*))
        }
    };
}

pub(crate) use {ldebug, lerror, linfo, lwarn};

/// Open the log in whatever handles .log (Notepad by default). Exposed on the
/// tray menu because when the settings window is the thing that will not
/// open, the tray is the only surface the user has left.
pub fn reveal() {
    if let Some(p) = path() {
        crate::util::shell_open(&p.to_string_lossy());
    }
}
