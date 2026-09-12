//! Monitor enumeration and the corner hitboxes derived from it.
//!
//! Everything here works in physical (per-monitor-v2) pixels. The process is
//! manifested PerMonitorV2, so `GetCursorPos` and the monitor rects share one
//! coordinate space and no DPI scaling is needed anywhere.

use crate::config::{Corner, MonitorMode};
use serde::Serialize;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
// windows-rs namespaces this under WindowsAndMessaging rather than Gdi,
// which is where the C headers put it.
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

#[derive(Clone, Copy, Debug)]
pub struct MonitorInfo {
    /// Full bounds, including any taskbar. Right/bottom are exclusive,
    /// per the usual Win32 RECT convention.
    pub rect: RECT,
    pub is_primary: bool,
}

/// A live corner: which corner it represents and the square the cursor has
/// to be inside for it to count.
#[derive(Clone, Copy, Debug)]
pub struct Hitbox {
    pub corner: Corner,
    pub rect: RECT,
}

/// One physical hot spot, identified by where it actually is.
///
/// `Corner` alone is not an identity: in `EveryMonitor` mode every display
/// contributes its own `TopLeft`, and a state machine that keys on the enum
/// treats all of them as the same place — so one screen's top-left stays
/// disarmed after a different screen's top-left fired. The anchor pixel
/// separates them, and is stable across the watcher's periodic rebuilds
/// because it is derived from the monitor rect rather than from list order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spot {
    pub corner: Corner,
    pub anchor: (i32, i32),
}

impl Hitbox {
    pub fn spot(&self) -> Spot {
        let p = anchor(self.corner, &self.rect);
        Spot {
            corner: self.corner,
            anchor: (p.x, p.y),
        }
    }
}

pub fn contains(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

unsafe extern "system" fn enum_proc(
    hmon: HMONITOR,
    _hdc: HDC,
    _clip: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let out = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(hmon, &mut mi).as_bool() {
        out.push(MonitorInfo {
            rect: mi.rcMonitor,
            is_primary: mi.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    BOOL(1) // keep enumerating
}

pub fn monitors() -> Vec<MonitorInfo> {
    let mut out: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&mut out as *mut _ as isize),
        );
    }
    out
}

fn union(monitors: &[MonitorInfo]) -> Option<RECT> {
    monitors.iter().map(|m| m.rect).reduce(|a, b| RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    })
}

/// The four `size`-square boxes anchored at the corners of `r`.
fn boxes_for(r: RECT, size: i32) -> [(Corner, RECT); 4] {
    // Clamp so a silly `size` on a small display cannot produce inverted or
    // mutually overlapping rects.
    let w = size.min((r.right - r.left).max(1));
    let h = size.min((r.bottom - r.top).max(1));
    [
        (
            Corner::TopLeft,
            RECT {
                left: r.left,
                top: r.top,
                right: r.left + w,
                bottom: r.top + h,
            },
        ),
        (
            Corner::TopRight,
            RECT {
                left: r.right - w,
                top: r.top,
                right: r.right,
                bottom: r.top + h,
            },
        ),
        (
            Corner::BottomLeft,
            RECT {
                left: r.left,
                top: r.bottom - h,
                right: r.left + w,
                bottom: r.bottom,
            },
        ),
        (
            Corner::BottomRight,
            RECT {
                left: r.right - w,
                top: r.bottom - h,
                right: r.right,
                bottom: r.bottom,
            },
        ),
    ]
}

/// Is this point actually on a physical display?
///
/// Matters for `OuterCorners` on non-rectangular arrangements: with two
/// screens offset vertically, a corner of the bounding box can sit in dead
/// space the cursor can never reach. Publishing a hitbox there would mean a
/// corner that silently never fires, so we drop it instead.
fn on_a_display(monitors: &[MonitorInfo], p: POINT) -> bool {
    monitors.iter().any(|m| contains(&m.rect, p))
}

/// Anchor pixel of a hitbox — the outermost pixel it owns.
fn anchor(corner: Corner, r: &RECT) -> POINT {
    match corner {
        Corner::TopLeft => POINT {
            x: r.left,
            y: r.top,
        },
        Corner::TopRight => POINT {
            x: r.right - 1,
            y: r.top,
        },
        Corner::BottomLeft => POINT {
            x: r.left,
            y: r.bottom - 1,
        },
        Corner::BottomRight => POINT {
            x: r.right - 1,
            y: r.bottom - 1,
        },
    }
}

/// Build the live hitboxes for the current display layout.
pub fn hitboxes(monitors: &[MonitorInfo], mode: MonitorMode, size: i32) -> Vec<Hitbox> {
    if monitors.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(8);

    match mode {
        MonitorMode::OuterCorners => {
            if let Some(u) = union(monitors) {
                for (corner, rect) in boxes_for(u, size) {
                    if on_a_display(monitors, anchor(corner, &rect)) {
                        out.push(Hitbox { corner, rect });
                    }
                }
            }
        }
        MonitorMode::EveryMonitor => {
            for m in monitors {
                for (corner, rect) in boxes_for(m.rect, size) {
                    out.push(Hitbox { corner, rect });
                }
            }
        }
        MonitorMode::PrimaryOnly => {
            // Fall back to the first monitor if Windows reports no primary,
            // which can briefly happen while displays are being reconfigured.
            let primary = monitors
                .iter()
                .find(|m| m.is_primary)
                .or_else(|| monitors.first());
            if let Some(m) = primary {
                for (corner, rect) in boxes_for(m.rect, size) {
                    out.push(Hitbox { corner, rect });
                }
            }
        }
    }
    out
}

/// Serialisable views for the settings page.
///
/// The page draws the real display arrangement rather than a generic
/// rectangle, and highlights exactly which corners are live. Both come from
/// here so Rust stays the single source of truth -- a corner dropped for not
/// sitting on a physical display is then visibly absent in the UI instead of
/// silently never firing.
#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DisplayView {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub primary: bool,
}

#[derive(Serialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HitboxView {
    pub corner: Corner,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub fn display_views(monitors: &[MonitorInfo]) -> Vec<DisplayView> {
    monitors
        .iter()
        .map(|m| DisplayView {
            left: m.rect.left,
            top: m.rect.top,
            right: m.rect.right,
            bottom: m.rect.bottom,
            primary: m.is_primary,
        })
        .collect()
}

pub fn hitbox_views(boxes: &[Hitbox]) -> Vec<HitboxView> {
    boxes
        .iter()
        .map(|h| HitboxView {
            corner: h.corner,
            left: h.rect.left,
            top: h.rect.top,
            right: h.rect.right,
            bottom: h.rect.bottom,
        })
        .collect()
}
