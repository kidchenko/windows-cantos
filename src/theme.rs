//! System light/dark preference.

use crate::util::{pcwstr, wide};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    REG_VALUE_TYPE,
};

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// True when the user has apps set to dark mode.
///
/// Defaults to dark if the value is missing: this app's own palette is
/// dark-first, so that is the safer guess on the rare system where the key is
/// absent.
pub fn prefers_dark() -> bool {
    read_dword(PERSONALIZE, "AppsUseLightTheme")
        .map(|v| v == 0)
        .unwrap_or(true)
}

fn read_dword(subkey: &str, value: &str) -> Option<u32> {
    let sub = wide(subkey);
    let name = wide(value);
    let mut hkey = HKEY::default();

    unsafe {
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            pcwstr(&sub),
            None,
            KEY_QUERY_VALUE,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return None;
        }

        let mut data: u32 = 0;
        let mut size: u32 = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let rc = RegQueryValueExW(
            hkey,
            pcwstr(&name),
            None,
            Some(&mut kind),
            Some(&mut data as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(hkey);

        (rc == ERROR_SUCCESS).then_some(data)
    }
}
