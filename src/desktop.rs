//! "What is the desktop doing right now?" — the Win32 queries the trigger
//! needs in order to decide whether a corner may fire.
//!
//! This is the only place that asks the OS about live state. Keeping it apart
//! from [`crate::trigger`] is what lets the corner rules be tested: nothing is
//! ever held down or fullscreen during `cargo test`, so a state machine that
//! called these functions directly could only ever exercise the "nothing is in
//! the way" path. The [`Desktop`] trait lets the tests answer for it.
//!
//! Not to be confused with [`crate::shell`], which asks Explorer to *do*
//! things. Nothing here has side effects.

use crate::config::Modifier;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU,
    VK_RBUTTON, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsZoomed,
    GWL_STYLE, WS_CAPTION, WS_THICKFRAME,
};

/// The desktop state a corner's decision depends on.
///
/// Every method is a question with a "no" that means "go ahead", so a test
/// that wants the unobstructed path can answer `false` to everything.
pub trait Desktop {
    /// Is the key the user asked to hold actually down?
    fn modifier_held(&self, m: Modifier) -> bool;
    /// Is any mouse button down — a drag, a selection, a snap in progress?
    fn mouse_button_down(&self) -> bool;
    /// Is a game or a video player filling a screen?
    fn fullscreen_active(&self) -> bool;
}

/// The real thing: answers by asking Windows.
pub struct Windows;

impl Desktop for Windows {
    fn modifier_held(&self, m: Modifier) -> bool {
        match m {
            Modifier::None => true,
            Modifier::Ctrl => key_down(VK_CONTROL),
            Modifier::Alt => key_down(VK_MENU),
            Modifier::Shift => key_down(VK_SHIFT),
            Modifier::Win => key_down(VK_LWIN) || key_down(VK_RWIN),
        }
    }

    /// Covers more than window dragging: dragging a file, rubber-band
    /// selecting, and drawing all end up parked somewhere with a button down,
    /// and none of them should trip a corner. `VK_LBUTTON` tracks the
    /// *primary* button, so this stays correct when the buttons are swapped.
    fn mouse_button_down(&self) -> bool {
        key_down(VK_LBUTTON) || key_down(VK_RBUTTON) || key_down(VK_MBUTTON)
    }

    fn fullscreen_active(&self) -> bool {
        shell_says_fullscreen() || foreground_covers_monitor()
    }
}

pub fn cursor_pos() -> Option<POINT> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok().map(|_| p) }
}

fn key_down(vk: VIRTUAL_KEY) -> bool {
    // High bit of GetAsyncKeyState is the "currently down" flag.
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

// -- Fullscreen ------------------------------------------------------------
//
// Two independent checks, because neither alone is sufficient. The shell's
// notification state catches exclusive-mode D3D games and presentation mode
// but misses borderless-windowed apps entirely, so we also measure the
// foreground window against its monitor.

fn shell_says_fullscreen() -> bool {
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else {
        return false;
    };
    state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE || state == QUNS_BUSY
}

fn foreground_covers_monitor() -> bool {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return false;
    }
    // The desktop itself is permanently "fullscreen"; treating it as such
    // would disable the app whenever nothing else has focus.
    if is_shell_window(hwnd) {
        return false;
    }
    // An ordinary maximised window is exempt — see `is_ordinary_maximised`.
    if is_ordinary_maximised(hwnd) {
        return false;
    }
    let (Some(window), Some(monitor)) = (window_rect(hwnd), monitor_rect(hwnd)) else {
        return false;
    };
    rect_covers(&window, &monitor)
}

/// Is this a normal application window someone has maximised?
///
/// Worth its own name because it is the single subtlest rule in the app.
/// Measuring cannot answer it: `GetWindowRect` reports the frame including the
/// invisible resize border, so a maximised window overhangs its monitor by
/// ~8px on every edge it is free to grow into. Wherever the work area equals
/// the monitor rect — a second display with no taskbar, which is the Windows
/// 11 default, or an auto-hide taskbar — that overhang alone is enough to
/// satisfy [`rect_covers`], and every corner on every display goes quiet while
/// a plainly ordinary maximised window has focus.
///
/// Both halves matter. `IsZoomed` alone would hand a free pass to anything
/// presenting itself maximised, so the window must also carry real furniture:
/// a borderless-fullscreen app has neither caption nor resize frame, and is
/// still caught by the geometry.
fn is_ordinary_maximised(hwnd: HWND) -> bool {
    unsafe { IsZoomed(hwnd).as_bool() && framed(window_style(hwnd)) }
}

fn window_style(hwnd: HWND) -> u32 {
    // x64-only target, so the `Ptr` variant is the right one.
    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 }
}

/// Does this style carry ordinary window furniture?
///
/// Every maximised window on a normal desktop has a caption and a resize
/// frame. A borderless-fullscreen app has neither — that is what "borderless"
/// means — so this separates the two without looking at any geometry.
fn framed(style: u32) -> bool {
    style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0
}

/// Does `window` reach or pass every edge of `monitor`?
///
/// `>=` rather than `==`: some players size themselves marginally larger than
/// the monitor to hide their borders.
fn rect_covers(window: &RECT, monitor: &RECT) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok().map(|_| r) }
}

fn monitor_rect(hwnd: HWND) -> Option<RECT> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(mon, &mut mi)
            .as_bool()
            .then_some(mi.rcMonitor)
    }
}

fn is_shell_window(hwnd: HWND) -> bool {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return false;
    }
    let class = String::from_utf16_lossy(&buf[..len as usize]);
    matches!(
        class.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Windows.UI.Core.CoreWindow"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    /// 1920x1200 display — the arrangement these numbers were measured on.
    const MONITOR: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1200,
    };

    #[test]
    fn a_borderless_fullscreen_window_covers_the_monitor() {
        assert!(rect_covers(&rect(0, 0, 1920, 1200), &MONITOR));
    }

    #[test]
    fn a_window_sized_past_the_monitor_still_counts() {
        // Some players overshoot deliberately to hide their borders.
        assert!(rect_covers(&rect(-2, -2, 1922, 1202), &MONITOR));
    }

    #[test]
    fn a_maximised_window_over_a_taskbar_does_not_cover_the_monitor() {
        // Measured: a maximised window reports its frame inflated by the
        // invisible resize border, here 9px, over a work area 60px short of
        // the monitor. Geometry alone rejects this one.
        assert!(!rect_covers(&rect(-9, -9, 1929, 1149), &MONITOR));
    }

    #[test]
    fn a_maximised_window_with_no_taskbar_looks_like_fullscreen() {
        // The case that broke it. With no taskbar on the display — the
        // Windows 11 default for a second screen — the work area *is* the
        // monitor, and the 9px frame inflation pushes a plainly ordinary
        // maximised window past every edge. Geometry cannot save us here,
        // which is why `is_ordinary_maximised` exists.
        assert!(rect_covers(&rect(-9, -9, 1929, 1209), &MONITOR));
    }

    #[test]
    fn an_ordinary_window_does_not_cover_the_monitor() {
        assert!(!rect_covers(&rect(50, 40, 1348, 1050), &MONITOR));
    }

    #[test]
    fn a_maximised_app_window_is_framed() {
        // Measured styles of every maximised window on the test desktop
        // (a browser, a chat app, a terminal): caption plus resize frame.
        assert!(framed(WS_CAPTION.0 | WS_THICKFRAME.0));
        assert!(framed(WS_CAPTION.0));
        assert!(framed(WS_THICKFRAME.0));
    }

    #[test]
    fn a_borderless_window_is_not_framed() {
        // What a fullscreen game presents. Even maximised, this must stay
        // eligible for suppression — hence checking the frame and not just
        // `IsZoomed`.
        use windows::Win32::UI::WindowsAndMessaging::{WS_POPUP, WS_VISIBLE};
        assert!(!framed(WS_POPUP.0 | WS_VISIBLE.0));
    }
}
