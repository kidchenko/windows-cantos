// No console window: this is a tray app, and a flashing conhost on login
// would be the first thing every user complained about.
#![windows_subsystem = "windows"]

//! Startup and wiring. The work happens elsewhere:
//!
//! | module      | responsibility                                      |
//! |-------------|-----------------------------------------------------|
//! | `watcher`   | polls the cursor on its own thread                  |
//! | `trigger`   | decides when a corner fires — the rules             |
//! | `desktop`   | asks Win32 what is happening right now              |
//! | `geometry`  | monitors, and the hitboxes derived from them        |
//! | `actions`   | what a corner does once it fires                    |
//! | `app`       | the tray icon, its menu, and the settings window    |
//! | `ui`        | the settings window and its page protocol           |

mod actions;
mod app;
mod autostart;
mod config;
mod desktop;
mod elevation;
mod geometry;
mod log;
mod shell;
mod single_instance;
mod theme;
mod trigger;
mod ui;
mod util;
mod watcher;

use app::{App, Flow, UserEvent};
use config::Config;
use log::{lerror, linfo, lwarn};
use std::sync::{Arc, Mutex};
use tao::event_loop::{ControlFlow, EventLoopBuilder};

fn main() {
    log::init();
    log_startup();

    // If we are the second launch, the first has been told to show its
    // settings window and there is nothing left for us to do.
    let Some(signal) = single_instance::acquire() else {
        linfo!("another instance owns the tray; asked it to show settings, exiting");
        return;
    };

    let cfg: watcher::SharedConfig = Arc::new(Mutex::new(Config::load()));
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Held for the life of the process; dropping it stops the polling thread.
    // It also owns config-file change detection, so an edit made in an editor
    // reaches the tray and an open settings window rather than sitting there
    // until the next restart — or being overwritten by our next save.
    let _watcher = {
        let proxy = proxy.clone();
        watcher::Watcher::start(cfg.clone(), move || {
            let _ = proxy.send_event(UserEvent::ConfigReloaded);
        })
    };

    // Launching the app again is how a user asks for the settings window.
    let signal_proxy = proxy.clone();
    signal.watch(move || {
        let _ = signal_proxy.send_event(UserEvent::OpenSettings);
    });

    let mut app = match App::new(cfg, proxy.clone()) {
        Ok(app) => app,
        Err(e) => {
            lerror!("{e}");
            return;
        }
    };
    linfo!("tray icon created; watcher running");

    // `--settings` opens the window straight away. The installer uses it for
    // its "launch now" checkbox, so a first run lands somewhere useful rather
    // than looking like nothing happened.
    if std::env::args().any(|a| a == "--settings") {
        let _ = proxy.send_event(UserEvent::OpenSettings);
    }

    event_loop.run(move |event, target, control_flow| {
        // Tray apps idle; nothing here needs a continuous redraw.
        *control_flow = ControlFlow::Wait;
        if app.handle(event, target) == Flow::Exit {
            *control_flow = ControlFlow::Exit;
        }
    });
}

/// The header every log file opens with. Worth the four lines: "which build,
/// running from where, at what privilege" answers most support questions
/// before the first corner is ever pressed.
fn log_startup() {
    linfo!(
        "--- Cantos {} starting (pid {}) ---",
        env!("CARGO_PKG_VERSION"),
        std::process::id()
    );
    linfo!(
        "exe: {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "<unknown>".into())
    );
    linfo!("args: {:?}", std::env::args().skip(1).collect::<Vec<_>>());
    linfo!(
        "integrity: {}",
        elevation::own_level()
            .map(elevation::describe)
            .unwrap_or("unknown")
    );
    // Recorded at every start, because "Settings does nothing" is otherwise
    // indistinguishable from a dozen other faults — and this is the answer
    // most of the time.
    match ui::runtime_version() {
        Some(v) => linfo!("webview2: {v}"),
        None => lwarn!("webview2: NOT INSTALLED — the settings window cannot open"),
    }
}
