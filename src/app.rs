//! The tray application: the icon, its menu, and the settings window.
//!
//! Everything here runs on the main thread, driven by the event loop in
//! `main`. [`App`] owns the mutable state that outlives a single event — the
//! tray handle and the settings window — so each event handler is a short
//! method rather than another branch of one long closure.

use crate::config::Config;
use crate::log::{lerror, linfo};
use crate::ui::page::{self, Msg};
use crate::ui::{self, Settings};
use crate::watcher::SharedConfig;
use crate::{actions, autostart, log, util};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{EventLoopProxy, EventLoopWindowTarget};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayImage, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

const ICON_EDGE: u32 = 64;

/// Everything that can wake the event loop.
pub enum UserEvent {
    OpenSettings,
    OpenLog,
    ToggleEnabled,
    Quit,
    /// `config.json` was edited outside the app and has been picked up.
    ConfigReloaded,
    /// A JSON message from the settings page.
    Ipc(String),
}

/// Whether the event loop should keep going.
#[derive(PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

pub struct App {
    cfg: SharedConfig,
    proxy: EventLoopProxy<UserEvent>,
    tray: TrayIcon,
    /// The "Hot corners enabled" checkbox, kept in step with the config.
    enabled_item: CheckMenuItem,
    /// `None` whenever the window is closed — and that is the point. Dropping
    /// it tears down the WebView2 processes, which is what returns the app to
    /// a few megabytes resident.
    settings: Option<Settings>,
}

impl App {
    pub fn new(cfg: SharedConfig, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let enabled = cfg.lock().map(|c| c.enabled).unwrap_or(true);

        let enabled_item = CheckMenuItem::new("Hot corners enabled", true, enabled, None);
        let settings_item = MenuItem::new("Settings\u{2026}", true, None);
        let log_item = MenuItem::new("Open log", true, None);
        let quit_item = MenuItem::new("Quit Cantos", true, None);

        let menu = Menu::new();
        let _ = menu.append_items(&[
            &enabled_item,
            &settings_item,
            &PredefinedMenuItem::separator(),
            &log_item,
            &quit_item,
        ]);

        // Captured by id, because the menu items themselves are moved into the
        // menu and the handler runs on another callback entirely.
        forward_menu_events(
            &proxy,
            MenuIds {
                enabled: enabled_item.id().clone(),
                settings: settings_item.id().clone(),
                log: log_item.id().clone(),
                quit: quit_item.id().clone(),
            },
        );
        forward_tray_events(&proxy);

        let icon = TrayImage::from_rgba(ui::icon_rgba(), ICON_EDGE, ICON_EDGE)
            .map_err(|e| format!("embedded tray icon is malformed: {e}"))?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip(enabled))
            // Right-click opens the menu; left-click is the primary action,
            // which for this app means "show me the settings".
            .with_menu_on_left_click(false)
            .with_icon(icon)
            .build()
            .map_err(|e| format!("could not create the tray icon: {e}"))?;

        Ok(Self {
            cfg,
            proxy,
            tray,
            enabled_item,
            settings: None,
        })
    }

    pub fn handle(
        &mut self,
        event: Event<UserEvent>,
        target: &EventLoopWindowTarget<UserEvent>,
    ) -> Flow {
        match event {
            Event::UserEvent(UserEvent::Quit) => {
                linfo!("quit requested from the tray");
                self.settings = None;
                return Flow::Exit;
            }
            Event::UserEvent(UserEvent::OpenLog) => log::reveal(),
            Event::UserEvent(UserEvent::ToggleEnabled) => self.toggle_enabled(),
            Event::UserEvent(UserEvent::OpenSettings) => self.open_settings(target),
            Event::UserEvent(UserEvent::ConfigReloaded) => self.sync_to_config(),
            Event::UserEvent(UserEvent::Ipc(body)) => self.handle_ipc(&body),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                linfo!("settings window closed; tearing down WebView2");
                self.settings = None;
            }
            _ => {}
        }
        Flow::Continue
    }

    fn toggle_enabled(&mut self) {
        let now = {
            let Ok(mut c) = self.cfg.lock() else { return };
            c.enabled = !c.enabled;
            let _ = c.save();
            c.enabled
        };
        self.show_enabled(now);
        self.refresh_page();
    }

    fn open_settings(&mut self, target: &EventLoopWindowTarget<UserEvent>) {
        // Already open: bring it forward rather than spawning a second
        // WebView2 instance.
        if let Some(s) = &self.settings {
            linfo!("settings already open; raising it");
            s.window.set_visible(true);
            s.window.set_minimized(false);
            s.window.set_focus();
            return;
        }

        let proxy = self.proxy.clone();
        match ui::build(target, move |body| {
            let _ = proxy.send_event(UserEvent::Ipc(body));
        }) {
            Ok(s) => {
                linfo!("settings window opened");
                self.settings = Some(s);
            }
            // The likeliest cause by far is a missing WebView2 runtime.
            // Say so in a dialog as well as the log: this is the app's only
            // window, so a log-only failure just looks like being ignored.
            Err(e) => {
                lerror!("could not open settings window: {e}");
                ui::report_open_failure(&e);
            }
        }
    }

    fn handle_ipc(&mut self, body: &str) {
        let Some(msg) = Msg::parse(body) else {
            lerror!("unparseable IPC message: {body}");
            return;
        };
        // Every message originates from the page, so the window is open --
        // but it can close between the post and the event being drained.
        //
        // Deliberately a presence check and not `let Some(s) = &self.settings`:
        // holding that borrow across the match would collide with the arms
        // that need `&mut self`. Arms that talk to the page take the borrow
        // themselves, right where they use it.
        if self.settings.is_none() {
            return;
        }

        match msg {
            Msg::Ready => self.refresh_page(),
            Msg::Set { config } => self.save_from_page(*config),
            Msg::Test { corner } => self.test_corner(corner),
            Msg::Reset => self.reset_to_defaults(),
            Msg::OpenLog => log::reveal(),
            Msg::OpenConfigDir => {
                if let Some(d) = Config::dir() {
                    util::shell_open(&d.to_string_lossy());
                }
            }
            Msg::Autostart { value } => self.set_autostart(value),
        }
    }

    /// The HKCU Run key is the only record of this; see the note on [`Config`].
    /// On failure we report back what the registry actually says rather than
    /// what was asked for, so the toggle cannot lie.
    fn set_autostart(&self, value: bool) {
        let Some(s) = &self.settings else { return };
        match autostart::set(value) {
            Ok(()) => page::push_autostart(s, value, None),
            Err(e) => page::push_autostart(s, autostart::is_enabled(), Some(&e)),
        }
    }

    fn save_from_page(&mut self, config: Config) {
        let sanitised = config.sanitised();
        let enabled = sanitised.enabled;
        let saved = {
            let Ok(mut guard) = self.cfg.lock() else {
                return;
            };
            *guard = sanitised;
            guard.save()
        };
        // Tray state can drift from the page, so mirror it on every save.
        self.show_enabled(enabled);

        let Some(s) = &self.settings else { return };
        page::push_geometry(s, &self.cfg);
        match saved {
            Ok(()) => page::toast(s, "Saved", false),
            Err(e) => page::toast(s, &format!("Could not save: {e}"), true),
        }
    }

    fn test_corner(&self, corner: usize) {
        let Some(action) = self
            .cfg
            .lock()
            .ok()
            .and_then(|c| c.corners.get(corner).cloned())
        else {
            return;
        };
        actions::fire(action);
    }

    fn reset_to_defaults(&mut self) {
        linfo!("settings reset to defaults");
        {
            let Ok(mut guard) = self.cfg.lock() else {
                return;
            };
            *guard = Config::default();
            let _ = guard.save();
        }
        self.show_enabled(Config::default().enabled);
        if let Some(s) = &self.settings {
            page::push_state(s, &self.cfg);
            page::toast(s, "Restored defaults", false);
        }
    }

    /// Bring the tray and any open page back in line with the config, after
    /// something outside this app changed it.
    fn sync_to_config(&mut self) {
        let enabled = self.cfg.lock().map(|c| c.enabled).unwrap_or(true);
        self.show_enabled(enabled);
        self.refresh_page();
    }

    /// The tray's two representations of the on/off state, updated together
    /// so they cannot disagree.
    fn show_enabled(&self, on: bool) {
        self.enabled_item.set_checked(on);
        let _ = self.tray.set_tooltip(Some(tooltip(on)));
    }

    fn refresh_page(&self) {
        if let Some(s) = &self.settings {
            page::push_state(s, &self.cfg);
        }
    }
}

fn tooltip(enabled: bool) -> &'static str {
    if enabled {
        "Cantos — active"
    } else {
        "Cantos — paused"
    }
}

struct MenuIds {
    enabled: tray_icon::menu::MenuId,
    settings: tray_icon::menu::MenuId,
    log: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

fn forward_menu_events(proxy: &EventLoopProxy<UserEvent>, ids: MenuIds) {
    let proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let event = if e.id == ids.enabled {
            UserEvent::ToggleEnabled
        } else if e.id == ids.settings {
            UserEvent::OpenSettings
        } else if e.id == ids.log {
            UserEvent::OpenLog
        } else if e.id == ids.quit {
            UserEvent::Quit
        } else {
            return;
        };
        let _ = proxy.send_event(event);
    }));
}

fn forward_tray_events(proxy: &EventLoopProxy<UserEvent>) {
    let proxy = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            ..
        } = e
        {
            let _ = proxy.send_event(UserEvent::OpenSettings);
        }
    }));
}
