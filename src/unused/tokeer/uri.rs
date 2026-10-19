//! `rundll32` entry point for `bst://` URI handling.
//!
//! Registered by [`super::bridge::register_uri_scheme`] as:
//! ```text
//!   rundll32.exe "<steam>\OpenSteamTool.dll",TokeerUri "%1"
//! ```
//! When the OS launches this, it runs in a short-lived rundll32 process,
//! **not** steam.exe. The DLL's injection init never runs here, so this
//! function must be self-contained.
//!
//! The C++ version initialises logging against this DLL's own directory
//! before dispatching. In Rust, log initialisation requires a
//! `ModuleHandle`, and computing the module containing an arbitrary
//! function requires `GetModuleHandleExW`. We skip that for now and let
//! logging init be a no-op (or fall through to the default sink, if any
//! has been installed).
//!
//! ## Type of the export
//!
//! Windows `CALLBACK` is `__stdcall`; on x64 that is the same as every
//! other convention, so `extern "system"` (which maps to `__stdcall` on
//! 32-bit Windows and the platform ABI on 64-bit) is correct here.

use super::bridge;

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn TokeerUri(
    _hwnd: *mut std::ffi::c_void,
    _hinst: *mut std::ffi::c_void,
    cmd_line: *const i8,
    _show_cmd: i32,
) {
    let raw = if cmd_line.is_null() {
        String::new()
    } else {
        unsafe {
            let mut len = 0;
            while *cmd_line.add(len) != 0 { len += 1; }
            let bytes = std::slice::from_raw_parts(cmd_line as *const u8, len);
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    bridge::handle_uri(&raw);
}

/// Non-Windows fallback so the symbol exists on every target.
#[cfg(not(windows))]
#[no_mangle]
pub extern "C" fn TokeerUri(_a: *mut std::ffi::c_void, _b: *mut std::ffi::c_void, _c: *const i8, _d: i32) {
}
