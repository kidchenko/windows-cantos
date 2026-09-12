//! The settings page protocol: what the page can ask for, and what we push
//! back into it.
//!
//! Two directions, both defined here so the contract is in one file:
//!
//!   * page -> Rust is [`Msg`], a JSON object tagged with `cmd`.
//!   * Rust -> page is the `push_*`/[`toast`] functions, which evaluate a
//!     `window.__hc.<name>(...)` call in the document.
//!
//! Every string that crosses into JavaScript goes through [`json`]. Paths and
//! OS error messages end up in these calls, and one stray quote would
//! otherwise turn a status message into a syntax error — or worse.

use super::Settings;
use crate::config::{Config, MonitorMode};
use crate::watcher::SharedConfig;
use crate::{autostart, geometry, log};
use serde::Deserialize;

/// Commands the settings page can send us.
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Msg {
    /// Page has loaded and wants the current state.
    Ready,
    Set {
        config: Box<Config>,
    },
    Test {
        corner: usize,
    },
    Autostart {
        value: bool,
    },
    /// Restore factory settings. Defaults live in Rust, so the page asks
    /// rather than hard-coding its own copy that could drift.
    Reset,
    OpenLog,
    OpenConfigDir,
}

impl Msg {
    pub fn parse(body: &str) -> Option<Self> {
        serde_json::from_str(body).ok()
    }
}

/// Everything the page needs to render itself from scratch.
pub fn push_state(s: &Settings, cfg: &SharedConfig) {
    // Take the geometry first: it locks the config, and std mutexes are not
    // reentrant, so doing it while holding `guard` would deadlock.
    let (displays, hitboxes) = geometry_json(cfg);
    let log_path = log::path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let config_dir = Config::dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let Ok(guard) = cfg.lock() else { return };
    let state = serde_json::json!({
        "config":    &*guard,
        "displays":  displays,
        "hitboxes":  hitboxes,
        "version":   env!("CARGO_PKG_VERSION"),
        "autostart": autostart::is_enabled(),
        "logPath":   log_path,
        "configDir": config_dir,
    });
    s.eval(&format!("window.__hc.state({state})"));
}

/// Just the diagram. Corner size and monitor mode change which rectangles are
/// live, so it is refreshed from Rust rather than the page guessing.
pub fn push_geometry(s: &Settings, cfg: &SharedConfig) {
    let (displays, hitboxes) = geometry_json(cfg);
    s.eval(&format!(
        "window.__hc.geometry({{displays:{displays},hitboxes:{hitboxes}}})"
    ));
}

/// A transient message in the page footer. `bad` colours it as an error.
pub fn toast(s: &Settings, msg: &str, bad: bool) {
    s.eval(&format!("window.__hc.toast({}, {bad})", json(msg)));
}

/// Reflect the real state of the run-at-login registry entry, with an optional
/// explanation of why it is not what was asked for.
pub fn push_autostart(s: &Settings, on: bool, err: Option<&str>) {
    match err {
        None => s.eval(&format!("window.__hc.autostart({on})")),
        Some(e) => s.eval(&format!("window.__hc.autostart({on}, {})", json(e))),
    }
}

/// The real display arrangement and the rectangles that are actually live.
///
/// Computed here, never in the page: a corner dropped for not sitting on a
/// physical display (L-shaped layouts) then shows as absent in the diagram
/// instead of being a corner that silently never fires.
fn geometry_json(cfg: &SharedConfig) -> (serde_json::Value, serde_json::Value) {
    let (mode, size) = cfg
        .lock()
        .map(|c| (c.monitor_mode, c.corner_size))
        .unwrap_or((MonitorMode::OuterCorners, 8));
    let monitors = geometry::monitors();
    let boxes = geometry::hitboxes(&monitors, mode, size);
    (
        serde_json::json!(geometry::display_views(&monitors)),
        serde_json::json!(geometry::hitbox_views(&boxes)),
    )
}

/// JSON-encode a string for embedding in a JavaScript expression.
fn json(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}
