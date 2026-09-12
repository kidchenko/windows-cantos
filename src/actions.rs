//! What a corner actually does when it fires.
//!
//! Every action here is dispatched on a short-lived detached thread (see
//! `fire`). Some of these calls — a broadcast `WM_SYSCOMMAND` in particular —
//! can block for seconds if any top-level window on the system is wedged, and
//! blocking the watcher thread would stall corner detection for everyone.

use crate::config::Action;
use crate::log::{ldebug, linfo, lwarn};
use crate::shell;
use crate::util::{pcwstr, wide};
use windows::Win32::Foundation::{GetLastError, LPARAM, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_OEM_PERIOD, VK_RCONTROL, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_TAB,
};
use windows::Win32::UI::Shell::ShellExecuteW;
// SC_SCREENSAVE and SC_MONITORPOWER are siblings in the Windows headers, but
// windows-rs files them under different namespaces.
use windows::Win32::Graphics::Gdi::SC_SCREENSAVE;
use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, HWND_BROADCAST, SC_MONITORPOWER, SW_SHOWNORMAL, WM_SYSCOMMAND,
};

/// windows-rs has no constants for letter keys; they are just their ASCII
/// codes in the virtual-key space.
const fn vk(c: u8) -> VIRTUAL_KEY {
    VIRTUAL_KEY(c as u16)
}
const VK_A: VIRTUAL_KEY = vk(b'A');
const VK_D: VIRTUAL_KEY = vk(b'D');
const VK_E: VIRTUAL_KEY = vk(b'E');
const VK_I: VIRTUAL_KEY = vk(b'I');
const VK_M: VIRTUAL_KEY = vk(b'M');
const VK_N: VIRTUAL_KEY = vk(b'N');
const VK_P: VIRTUAL_KEY = vk(b'P');
const VK_R: VIRTUAL_KEY = vk(b'R');
const VK_S: VIRTUAL_KEY = vk(b'S');
const VK_V: VIRTUAL_KEY = vk(b'V');
const VK_X: VIRTUAL_KEY = vk(b'X');

/// Modifiers we synthesise a release for before sending a combo. See
/// `release_held_modifiers`.
const MODIFIER_KEYS: [VIRTUAL_KEY; 8] = [
    VK_LCONTROL,
    VK_RCONTROL,
    VK_LSHIFT,
    VK_RSHIFT,
    VK_LMENU,
    VK_RMENU,
    VK_LWIN,
    VK_RWIN,
];

fn key_event(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        return;
    }

    // Note this does NOT catch a UIPI discard: when the focused window
    // outranks us Windows accepts the call and reports every event as
    // inserted, then throws the input away. That case is detected up front
    // in `combo` instead.
    let err = unsafe { GetLastError() };
    lwarn!(
        "SendInput inserted {sent} of {} events (error {})",
        inputs.len(),
        err.0
    );
}

fn is_down(vk: VIRTUAL_KEY) -> bool {
    // The high bit of GetAsyncKeyState is the "currently down" flag.
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

/// Synthesise a key-up for every modifier the user is physically holding.
///
/// This exists because of the "require a modifier" setting: if the user has to
/// hold Ctrl to arm a corner, then a naive `Win+Tab` actually arrives as
/// `Ctrl+Win+Tab`, which is a different shortcut entirely. We release the held
/// modifiers first so the combo lands clean. We deliberately do not re-press
/// them — the user's physical release will do that naturally.
fn release_held_modifiers() {
    let ups: Vec<INPUT> = MODIFIER_KEYS
        .iter()
        .filter(|&&vk| is_down(vk))
        .map(|&vk| key_event(vk, KEYEVENTF_KEYUP))
        .collect();
    send(&ups);
}

/// Press keys in order, then release them in reverse — the order a human
/// would produce, which is what shell hooks expect.
fn combo(keys: &[VIRTUAL_KEY]) {
    release_held_modifiers();
    let mut inputs = Vec::with_capacity(keys.len() * 2);
    for &k in keys {
        inputs.push(key_event(k, KEYBD_EVENT_FLAGS(0)));
    }
    for &k in keys.iter().rev() {
        inputs.push(key_event(k, KEYEVENTF_KEYUP));
    }
    send(&inputs);
}

fn broadcast_syscommand(command: u32, param: i32) {
    // PostMessage rather than SendMessage: this is fire-and-forget and cannot
    // block on an unresponsive window, which SendMessage very much can.
    unsafe {
        let _ = PostMessageW(
            Some(HWND_BROADCAST),
            WM_SYSCOMMAND,
            WPARAM(command as usize),
            LPARAM(param as isize),
        );
    }
}

fn run_custom(command: &str, args: &str) {
    if command.trim().is_empty() {
        lwarn!("custom action has no command set; nothing to run");
        return;
    }
    let verb = wide("open");
    let file = wide(command.trim());
    let params = wide(args);
    unsafe {
        // ShellExecuteW handles executables, scripts, documents and URLs
        // uniformly, which is exactly the range we advertise for this action.
        // ShellExecuteW reports failure as a value <= 32, not as an error.
        let rc = ShellExecuteW(
            None,
            pcwstr(&verb),
            pcwstr(&file),
            if args.is_empty() {
                PCWSTR_NULL
            } else {
                pcwstr(&params)
            },
            None,
            SW_SHOWNORMAL,
        );
        if rc.0 as isize <= 32 {
            lwarn!(
                "could not run {command:?}: ShellExecute returned {}",
                rc.0 as isize
            );
        } else {
            ldebug!("ran {command:?} {args:?}");
        }
    }
}

const PCWSTR_NULL: windows::core::PCWSTR = windows::core::PCWSTR::null();

fn execute(action: &Action) {
    match action {
        Action::None => {}
        // Shell automation first, keystrokes only as a fallback. See
        // `shell` for why: synthesised input is silently discarded whenever
        // an elevated window holds focus, and these routes are not.
        Action::TaskView => dispatch("Task View", shell::window_switcher, &[VK_LWIN, VK_TAB]),
        Action::ShowDesktop => dispatch("Show desktop", shell::toggle_desktop, &[VK_LWIN, VK_D]),
        Action::MinimizeAll => dispatch("Minimize all", shell::minimize_all, &[VK_LWIN, VK_M]),
        Action::StartMenu => dispatch("Start menu", shell::start_menu, &[VK_LCONTROL, VK_ESCAPE]),
        Action::RunDialog => dispatch("Run", shell::run_dialog, &[VK_LWIN, VK_R]),
        Action::Settings => dispatch(
            "Settings",
            || shell::launch("ms-settings:"),
            &[VK_LWIN, VK_I],
        ),
        Action::FileExplorer => dispatch(
            "File Explorer",
            || shell::launch("explorer.exe"),
            &[VK_LWIN, VK_E],
        ),
        Action::Screenshot => dispatch(
            "Screenshot",
            || shell::launch("ms-screenclip:"),
            &[VK_LWIN, VK_LSHIFT, VK_S],
        ),
        Action::ProjectDisplay => dispatch(
            "Project",
            || shell::launch("ms-settings-displays-topology-projection:"),
            &[VK_LWIN, VK_P],
        ),
        Action::Notifications => dispatch(
            "Notifications",
            || shell::launch("ms-actioncenter:"),
            &[VK_LWIN, VK_N],
        ),

        // No shell equivalent exists for these, so they stay on synthesised
        // input and inherit its limitation against elevated windows.
        Action::DesktopLeft => combo(&[VK_LCONTROL, VK_LWIN, VK_LEFT]),
        Action::DesktopRight => combo(&[VK_LCONTROL, VK_LWIN, VK_RIGHT]),
        Action::Search => combo(&[VK_LWIN, VK_S]),
        Action::QuickSettings => combo(&[VK_LWIN, VK_A]),
        Action::QuickLink => combo(&[VK_LWIN, VK_X]),
        Action::Clipboard => combo(&[VK_LWIN, VK_V]),
        Action::Emoji => combo(&[VK_LWIN, VK_OEM_PERIOD]),
        Action::LockWorkstation => unsafe {
            let _ = LockWorkStation();
        },
        Action::Screensaver => broadcast_syscommand(SC_SCREENSAVE, 0),
        // 2 == power off. Note this is a request, not a guarantee: systems
        // using Modern Standby may ignore it.
        Action::DisplayOff => broadcast_syscommand(SC_MONITORPOWER, 2),
        Action::Custom { command, args } => run_custom(command, args),
    }
}

/// Synthesised keys by default; the shell route only when keys would be lost.
///
/// The ordering matters and was originally the other way round, which was a
/// mistake: `IShellDispatch` reports success from the COM call itself, not
/// from the UI actually appearing, so a shell route that quietly does nothing
/// still looked like it worked and the keystroke fallback never ran. Keys are
/// the better default because their behaviour is observable and correct
/// wherever they are permitted at all — everywhere except an elevated
/// foreground window.
fn dispatch<F: FnOnce() -> bool>(what: &str, shell_route: F, keys: &[VIRTUAL_KEY]) {
    if let Some(reason) = crate::elevation::keystroke_will_be_discarded() {
        lwarn!("{what}: keys would be discarded ({reason}); trying shell automation");
        if shell_route() {
            linfo!("{what}: sent via shell automation");
            return;
        }
        lwarn!("{what}: shell automation unavailable too; sending keys anyway");
    }
    combo(keys);
}

/// Run an action without blocking the caller.
pub fn fire(action: Action) {
    if action.is_none() {
        return;
    }
    std::thread::spawn(move || {
        // Shell automation is COM, so this thread needs an apartment. It is
        // a fresh thread every time, hence initialise and uninitialise here
        // rather than once at startup.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        execute(&action);
        unsafe { CoUninitialize() };
    });
}
