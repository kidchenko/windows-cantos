//! Small Win32 helpers shared across modules.

use windows::core::PCWSTR;

/// Null-terminated UTF-16, for the `W` half of the Win32 API.
/// The caller must keep the returned buffer alive for as long as the
/// `PCWSTR` derived from it is in use.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn pcwstr(buf: &[u16]) -> PCWSTR {
    PCWSTR(buf.as_ptr())
}

/// Full path to the running executable, quoted for use in a command line.
pub fn exe_path_quoted() -> Option<String> {
    let p = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", p.display()))
}

/// Hand a path (or URL) to the shell. Used for "Open log" and "Open config
/// folder", both of which are support affordances rather than features.
pub fn shell_open(target: &str) {
    let verb = wide("open");
    let file = wide(target);
    unsafe {
        let _ = windows::Win32::UI::Shell::ShellExecuteW(
            None,
            pcwstr(&verb),
            pcwstr(&file),
            windows::core::PCWSTR::null(),
            None,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
    }
}
