//! Single-instance enforcement, plus a way for the second launch to be useful.
//!
//! Exiting silently when the app is already in the tray is correct but
//! baffling: double-clicking the shortcut appears to do nothing at all. So the
//! second instance signals the first to surface its settings window, then
//! steps aside.
//!
//! A named auto-reset event carries the signal rather than a broadcast window
//! message. `HWND_BROADCAST` only reaches top-level windows, and this app
//! spends nearly all its life with no window at all — it would have to hold a
//! hidden top-level window open purely to receive the broadcast. An event
//! needs no window and no message pump.

use crate::log::{linfo, lwarn};
use crate::util::{pcwstr, wide};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};

// "Local\" scopes both objects to the logon session, so two users signed in at
// once each get their own instance rather than fighting over one.
const MUTEX_NAME: &str = r"Local\Cantos.SingleInstance";
const EVENT_NAME: &str = r"Local\Cantos.ShowSettings";

/// Held by the sole running instance. Owns the event other launches signal.
pub struct Signal(isize);

/// Returns `None` if another instance is already running, in which case it has
/// been asked to show its settings window and this process should exit.
pub fn acquire() -> Option<Signal> {
    let mutex_name = wide(MUTEX_NAME);

    let existing = unsafe {
        match CreateMutexW(None, true, pcwstr(&mutex_name)) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = CloseHandle(handle);
                    true
                } else {
                    // Deliberately never closed: the mutex must outlive
                    // `main`, and Windows releases it on process exit anyway.
                    // `HANDLE` is a plain Copy wrapper with no Drop, so simply
                    // dropping the binding keeps the handle open.
                    let _ = handle;
                    false
                }
            }
            // If the mutex cannot be created at all, assume we are alone
            // rather than refusing to start.
            Err(_) => false,
        }
    };

    if existing {
        linfo!("mutex already held: another instance is running");
        raise();
        return None;
    }

    // Auto-reset, initially unsignalled. Created here rather than in `watch`
    // so the window in which a second launch could find no event to open is as
    // small as possible.
    let event_name = wide(EVENT_NAME);
    match unsafe { CreateEventW(None, false, false, pcwstr(&event_name)) } {
        Ok(handle) => Some(Signal(handle.0 as isize)),
        Err(e) => {
            // Emphatically NOT None. Returning None here would make the app
            // silently refuse to start, which from the outside is
            // indistinguishable from a crash -- the window "blinks and
            // closes". Losing relaunch-to-surface is the lesser failure.
            lwarn!("could not create the show-settings event ({e}); relaunching will not surface the window");
            Some(Signal(0))
        }
    }
}

/// Ask the instance that already owns the event to show its settings.
fn raise() {
    let event_name = wide(EVENT_NAME);
    unsafe {
        match OpenEventW(EVENT_MODIFY_STATE, false, pcwstr(&event_name)) {
            Ok(handle) => {
                match SetEvent(handle) {
                    Ok(()) => linfo!("signalled the running instance to show settings"),
                    Err(e) => lwarn!("SetEvent failed: {e}"),
                }
                let _ = CloseHandle(handle);
            }
            Err(e) => lwarn!("could not open the show-settings event: {e}"),
        }
    }
}

impl Signal {
    /// Run `on_signal` every time another launch asks us to surface.
    ///
    /// The thread blocks forever. That is fine: the event loop never returns,
    /// so the process tears this thread down on exit.
    pub fn watch<F>(self, on_signal: F)
    where
        F: Fn() + Send + 'static,
    {
        let raw = self.0;
        if raw == 0 {
            lwarn!("no show-settings event; relaunch-to-surface is disabled");
            return;
        }
        let _ = std::thread::Builder::new()
            .name("cantos-instance".into())
            .spawn(move || {
                let handle = HANDLE(raw as *mut std::ffi::c_void);
                linfo!("listening for relaunch signals");
                loop {
                    // WAIT_OBJECT_0 is the only value we want to act on; a
                    // failure here means the handle is gone and looping would
                    // spin at full tilt.
                    let rc = unsafe { WaitForSingleObject(handle, INFINITE) };
                    if rc.0 != 0 {
                        lwarn!(
                            "relaunch listener stopping (WaitForSingleObject returned {})",
                            rc.0
                        );
                        break;
                    }
                    linfo!("relaunch signal received; opening settings");
                    on_signal();
                }
            });
    }
}
