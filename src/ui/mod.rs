//! The settings window: a tao window hosting a WebView2 control.
//!
//! The window is built on demand and dropped when closed. That teardown is
//! the whole reason WebView2 is affordable here — while the window is open we
//! pay the usual ~80 MB of Edge processes, and the moment it closes we are
//! back to a few megabytes of tray app. Keeping the environment warm would
//! make reopening instant but would hold that memory for the entire session,
//! which is exactly the trade this app exists to avoid.

pub mod page;

use crate::util::{pcwstr, wide};
use std::path::PathBuf;
use tao::dpi::LogicalSize;
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Icon, Window, WindowBuilder};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO,
};
use wry::{WebContext, WebView, WebViewBuilder};

const HTML: &str = include_str!("index.html");
const TRAY_RGBA: &[u8] = include_bytes!("../../assets/tray-64.rgba");
const ICON_EDGE: u32 = 64;
const WEBVIEW2_DOWNLOAD: &str = "https://developer.microsoft.com/microsoft-edge/webview2/";

/// Decoded window/tray icon pixels. Kept as raw RGBA rather than a PNG so we
/// do not need an image decoder in the dependency tree.
pub fn icon_rgba() -> Vec<u8> {
    TRAY_RGBA.to_vec()
}

pub fn window_icon() -> Option<Icon> {
    Icon::from_rgba(icon_rgba(), ICON_EDGE, ICON_EDGE).ok()
}

/// The installed WebView2 runtime version, or `None` if there is not one.
///
/// Asks the runtime itself (`GetAvailableCoreWebView2BrowserVersionString` via
/// wry) rather than reading `EdgeUpdate` registry keys, so it stays correct
/// however the runtime was deployed — per-user, per-machine, or fixed-version.
pub fn runtime_version() -> Option<String> {
    wry::webview_version().ok()
}

/// Tell the user, in a dialog, why the settings window did not appear.
///
/// The settings window is this app's only UI, so a failure recorded nowhere
/// but the log reads as "the app is broken": the user clicks Settings, nothing
/// happens at all, and the log they would need is behind the same tray menu
/// they have just stopped trusting.
///
/// By far the likeliest cause is a missing WebView2 runtime. Windows 11 ships
/// it, and on Windows 10 it arrives with Edge — but LTSC, N editions, and
/// locked-down enterprise images can be without it, and this app supports
/// Windows 10.
pub fn report_open_failure(error: &str) {
    match runtime_version() {
        // The runtime is there, so this is something else. Nothing to offer
        // beyond the error itself.
        Some(version) => {
            alert(
                "Cantos — settings unavailable",
                &format!(
                    "Cantos could not open its settings window.\n\n\
                     The WebView2 runtime is installed (version {version}), so \
                     that is not the cause:\n\n{error}\n\n\
                     Hot corners themselves are unaffected and keep working.",
                ),
                MB_ICONERROR | MB_OK,
            );
        }
        None => {
            let opened = alert(
                "Cantos — WebView2 required",
                "Cantos needs the Microsoft WebView2 runtime to show its \
                 settings window, and it is not installed on this PC.\n\n\
                 Your hot corners keep working — only the settings window is \
                 affected. You can also edit the settings file directly; it is \
                 in %APPDATA%\\Cantos\\config.json.\n\n\
                 Open the WebView2 download page now?",
                MB_ICONWARNING | MB_YESNO,
            );
            if opened {
                crate::util::shell_open(WEBVIEW2_DOWNLOAD);
            }
        }
    }
}

/// Returns whether the user chose Yes. Always false for an OK-only dialog.
fn alert(
    title: &str,
    text: &str,
    style: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> bool {
    let title = wide(title);
    let text = wide(text);
    // No owner window: the tray has none, and the event loop is blocked for
    // as long as this is up either way.
    unsafe { MessageBoxW(None, pcwstr(&text), pcwstr(&title), style) == IDYES }
}

pub struct Settings {
    // Field order is load-bearing: Rust drops fields in declaration order, and
    // `WebView`'s drop closes its WebView2 controller and un-subclasses the
    // parent HWND. With the window first, both of those ran against a window
    // that had already been destroyed — every time the settings window closed.
    pub webview: WebView,
    #[allow(dead_code)] // dropped with the struct; that teardown is the point
    pub window: Window,
}

impl Settings {
    /// Push a JS expression into the page, ignoring failures — the window may
    /// be closing underneath us, and a dropped status message is harmless.
    pub fn eval(&self, script: &str) {
        let _ = self.webview.evaluate_script(script);
    }
}

pub fn build<T, F>(target: &EventLoopWindowTarget<T>, on_ipc: F) -> Result<Settings, String>
where
    F: Fn(String) + 'static,
{
    let window = WindowBuilder::new()
        .with_title("Cantos")
        .with_inner_size(LogicalSize::new(680.0, 828.0))
        .with_min_inner_size(LogicalSize::new(560.0, 560.0))
        .with_window_icon(window_icon())
        .with_visible(true)
        .build(target)
        .map_err(|e| format!("could not create the settings window: {e}"))?;

    apply_titlebar_theme(&window);

    // WebView2 needs somewhere writable to keep its user data folder, and left
    // to itself it puts that folder *next to the executable*. That is fine
    // running out of `target\release`, and fails with `E_ACCESSDENIED
    // (0x80070005)` the moment the app is actually installed, because nobody
    // can write to `C:\Program Files\Cantos`. The settings window simply never
    // opened for anyone who used the installer.
    let mut context = WebContext::new(Some(webview_data_dir()));

    let webview = WebViewBuilder::new_with_web_context(&mut context)
        .with_html(HTML)
        // The page is a static local document with no navigation, so there is
        // nothing useful behind a context menu or devtools for an end user.
        .with_devtools(false)
        .with_ipc_handler(move |req| on_ipc(req.body().to_string()))
        .build(&window)
        .map_err(|e| format!("could not create the WebView2 control: {e}"))?;

    Ok(Settings { webview, window })
}

/// Where WebView2 keeps its cache and profile.
///
/// `LOCALAPPDATA` rather than `APPDATA`: this is a browser cache measured in
/// megabytes, and roaming it between machines would be nothing but a tax. It
/// deliberately sits beside the app's own data rather than inside it, so that
/// clearing a wedged webview profile cannot take `config.json` with it.
fn webview_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        // No LOCALAPPDATA is close to impossible, but a temp folder still
        // beats the install directory, which is the one place guaranteed to
        // fail.
        .unwrap_or_else(std::env::temp_dir);
    base.join("Cantos").join("WebView2")
}

/// Match the titlebar to the system theme.
///
/// The page itself follows `prefers-color-scheme`, but the non-client area is
/// Windows' to draw. Without this a dark page gets a bright white titlebar,
/// which looks broken.
fn apply_titlebar_theme(window: &Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};

    let hwnd = HWND(window.hwnd() as *mut _);
    let dark: i32 = if crate::theme::prefers_dark() { 1 } else { 0 };
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}
