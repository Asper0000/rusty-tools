//! Modal message boxes. Source: `OSTPlatform/Windows/Dialog.cpp`
//!
//! Uses `MessageBoxA` with `MB_TOPMOST`, matching the C++ (Steam's
//! overlay can cover a non-topmost box).

/// Modal warning box (OK, warning icon).
pub fn show_warning(title: &str, message: &str) {
    #[cfg(windows)]
    message_box(title, message, MB_OK | MB_ICONWARNING);
    #[cfg(not(windows))]
    {
        let _ = (title, message);
    }
}

/// Modal information box (OK, info icon). Blocks until dismissed.
pub fn show_info(title: &str, message: &str) {
    #[cfg(windows)]
    message_box(title, message, MB_OK | MB_ICONINFORMATION);
    #[cfg(not(windows))]
    {
        let _ = (title, message);
    }
}

/// Modal Yes/No confirmation. Returns true when the user chose Yes.
pub fn show_confirm(title: &str, message: &str) -> bool {
    #[cfg(windows)]
    {
        let r = message_box(title, message, MB_YESNO | MB_ICONINFORMATION);
        r == IDYES
    }
    #[cfg(not(windows))]
    {
        let _ = (title, message);
        false
    }
}

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxA, IDYES, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MB_TOPMOST, MB_YESNO,
};

#[cfg(windows)]
fn message_box(title: &str, message: &str, flags: u32) -> i32 {
    use std::ffi::CString;
    let t = CString::new(title).unwrap_or_default();
    let m = CString::new(message).unwrap_or_default();
    unsafe {
        MessageBoxA(
            std::ptr::null_mut(),
            m.as_ptr() as *const u8,
            t.as_ptr() as *const u8,
            flags | MB_TOPMOST,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // We do not pop modal dialogs in tests - they would hang the test
    // runner. These tests only verify the functions are callable.

    #[cfg(not(windows))]
    #[test]
    fn stubs_are_callable() {
        show_warning("t", "m");
        show_info("t", "m");
        assert!(!show_confirm("t", "m"));
    }
}
