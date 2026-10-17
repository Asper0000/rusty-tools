//! Dynamic library loading. Source: `OSTPlatform/Windows/DynamicLibrary.cpp`
//!
//! Real Win32 implementation via `windows-sys`.
//!
//! ## Argument-order divergence in Win32
//!
//! `GetCurrentDirectoryW` and `GetSystemDirectoryW` take the same two
//! arguments in the opposite order:
//!   GetCurrentDirectoryW(capacity, buffer)
//!   GetSystemDirectoryW(buffer, capacity)
//!
//! The C++ header hides this with two thunks. We do the same here with
//! `get_current_directory_w_thunk` and `get_system_directory_w_thunk`.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ModuleHandle(pub *mut c_void);

impl ModuleHandle {
    pub const fn null() -> Self { Self(std::ptr::null_mut()) }
    pub fn is_null(&self) -> bool { self.0.is_null() }
}

// SAFETY: an `HMODULE` is a process-global loader handle. Windows
// documentation permits it to be passed between threads; the loader
// serialises map/unmap operations internally, and the value itself
// carries no thread affinity. The default `!Send` / `!Sync` come only
// from the raw pointer field. CloudRedirect's host registry stores a
// `ModuleHandle` in a `Mutex<...>` static, which requires this.
unsafe impl Send for ModuleHandle {}
unsafe impl Sync for ModuleHandle {}

impl Default for ModuleHandle {
    fn default() -> Self { Self::null() }
}

// ─── Windows implementation ──────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::MAX_PATH;
    use windows_sys::Win32::System::Environment::GetCurrentDirectoryW;
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    const MAX_WIN32_PATH_BUFFER: u32 = 32768;

    fn path_to_wide_null(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    fn str_to_wide_null(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_to_string(buf: &[u16]) -> String {
        String::from_utf16_lossy(buf)
    }

    pub(super) fn load(path: &Path) -> ModuleHandle {
        let wide = path_to_wide_null(path);
        let h = unsafe { LoadLibraryW(wide.as_ptr()) };
        if h.is_null() {
            crate::ostp_warn!(
                "LoadLibraryW('{}') failed (error={})",
                path.display(),
                get_last_error_code()
            );
        }
        ModuleHandle(h as *mut c_void)
    }

    pub(super) fn get_loaded(module_name: &str) -> ModuleHandle {
        let wide = str_to_wide_null(module_name);
        let h = unsafe { GetModuleHandleW(wide.as_ptr()) };
        ModuleHandle(h as *mut c_void)
    }

    pub(super) unsafe fn get_symbol(module: ModuleHandle, symbol_name: &str) -> *mut c_void {
        if module.0.is_null() { return std::ptr::null_mut(); }
        let name = std::ffi::CString::new(symbol_name).unwrap_or_default();
        let f = GetProcAddress(module.0 as _, name.as_ptr() as *const u8);
        match f {
            Some(f) => f as *mut c_void,
            None => {
                crate::ostp_debug!(
                    "GetProcAddress('{}') failed (error={})",
                    symbol_name,
                    get_last_error_code()
                );
                std::ptr::null_mut()
            }
        }
    }

    pub(super) unsafe fn get_symbol_ordinal(module: ModuleHandle, ordinal: u16) -> *mut c_void {
        if module.0.is_null() { return std::ptr::null_mut(); }
        let ord_ptr = ordinal as usize as *const u8;
        let f = GetProcAddress(module.0 as _, ord_ptr);
        match f {
            Some(f) => f as *mut c_void,
            None => {
                crate::ostp_debug!(
                    "GetProcAddress(ordinal={}) failed (error={})",
                    ordinal,
                    get_last_error_code()
                );
                std::ptr::null_mut()
            }
        }
    }

    pub(super) fn get_last_error_code() -> u32 {
        unsafe { windows_sys::Win32::Foundation::GetLastError() }
    }

    fn module_file_path(module: *mut c_void, operation: &str) -> PathBuf {
        let mut capacity: u32 = MAX_PATH;
        while capacity <= MAX_WIN32_PATH_BUFFER {
            let mut buf = vec![0u16; capacity as usize];
            unsafe { windows_sys::Win32::Foundation::SetLastError(0); }
            let len = unsafe { GetModuleFileNameW(module as _, buf.as_mut_ptr(), capacity) };
            if len == 0 {
                crate::ostp_warn!(
                    "{}: GetModuleFileNameW failed (error={})",
                    operation,
                    get_last_error_code()
                );
                return PathBuf::new();
            }
            if len < capacity {
                return PathBuf::from(wide_to_string(&buf[..len as usize]));
            }
            capacity = capacity.saturating_mul(2);
        }
        crate::ostp_warn!(
            "{}: GetModuleFileNameW path exceeds {} wchars",
            operation,
            MAX_WIN32_PATH_BUFFER
        );
        PathBuf::new()
    }

    // ── Directory thunks ─────────────────────────────────────────
    // Normalise both APIs to the (capacity, buffer) shape so a single
    // loop handles them. Matches the C++ thunk pattern.

    unsafe extern "system" fn get_current_directory_w_thunk(
        capacity: u32,
        buffer: *mut u16,
    ) -> u32 {
        GetCurrentDirectoryW(capacity, buffer)
    }

    unsafe extern "system" fn get_system_directory_w_thunk(
        capacity: u32,
        buffer: *mut u16,
    ) -> u32 {
        // Note the reversed order - GetSystemDirectoryW takes (buffer, capacity).
        GetSystemDirectoryW(buffer, capacity)
    }

    fn directory_from_query(
        query: unsafe extern "system" fn(u32, *mut u16) -> u32,
        operation: &str,
    ) -> String {
        let mut capacity: u32 = MAX_PATH;
        while capacity <= MAX_WIN32_PATH_BUFFER {
            let mut buf = vec![0u16; capacity as usize];
            let len = unsafe { query(capacity, buf.as_mut_ptr()) };
            if len == 0 {
                crate::ostp_warn!("{} failed (error={})", operation, get_last_error_code());
                return String::new();
            }
            if len < capacity {
                return wide_to_string(&buf[..len as usize]);
            }
            capacity = if len > capacity { len } else { capacity.saturating_mul(2) };
        }
        crate::ostp_warn!(
            "{} path exceeds {} wchars",
            operation,
            MAX_WIN32_PATH_BUFFER
        );
        String::new()
    }

    pub(super) fn get_current_directory_path() -> String {
        directory_from_query(get_current_directory_w_thunk, "GetCurrentDirectoryW")
    }

    pub(super) fn get_system_directory_path() -> String {
        directory_from_query(get_system_directory_w_thunk, "GetSystemDirectoryW")
    }

    pub(super) fn get_module_directory(module: ModuleHandle) -> PathBuf {
        module_file_path(module.0, "GetModuleDirectory")
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default()
    }

    pub(super) fn get_main_executable_path() -> PathBuf {
        module_file_path(std::ptr::null_mut(), "GetMainExecutablePath")
    }
}

// ─── Non-Windows fallback (stubs) ────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn load(_path: &Path) -> ModuleHandle { ModuleHandle::null() }
    pub(super) fn get_loaded(_module_name: &str) -> ModuleHandle { ModuleHandle::null() }
    pub(super) unsafe fn get_symbol(_m: ModuleHandle, _s: &str) -> *mut c_void { std::ptr::null_mut() }
    pub(super) unsafe fn get_symbol_ordinal(_m: ModuleHandle, _o: u16) -> *mut c_void { std::ptr::null_mut() }
    pub(super) fn get_last_error_code() -> u32 { 0 }
    pub(super) fn get_current_directory_path() -> String { String::new() }
    pub(super) fn get_system_directory_path() -> String { String::new() }
    pub(super) fn get_module_directory(_m: ModuleHandle) -> PathBuf { PathBuf::new() }
    pub(super) fn get_main_executable_path() -> PathBuf { PathBuf::new() }
}

// ─── Public API ──────────────────────────────────────────────────────

pub fn load(path: &Path) -> ModuleHandle { imp::load(path) }
pub fn get_loaded(module_name: &str) -> ModuleHandle { imp::get_loaded(module_name) }
pub unsafe fn get_symbol(module: ModuleHandle, symbol_name: &str) -> *mut c_void {
    imp::get_symbol(module, symbol_name)
}
pub unsafe fn get_symbol_ordinal(module: ModuleHandle, ordinal: u16) -> *mut c_void {
    imp::get_symbol_ordinal(module, ordinal)
}
pub fn get_last_error_code() -> u32 { imp::get_last_error_code() }
pub fn get_current_directory_path() -> String { imp::get_current_directory_path() }
pub fn get_system_directory_path() -> String { imp::get_system_directory_path() }
pub fn get_module_directory(module: ModuleHandle) -> PathBuf { imp::get_module_directory(module) }
pub fn get_main_executable_path() -> PathBuf { imp::get_main_executable_path() }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_handle_is_null() {
        assert!(ModuleHandle::null().is_null());
        assert!(ModuleHandle::default().is_null());
    }

    #[test]
    fn module_handle_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ModuleHandle>();
    }

    #[cfg(windows)]
    #[test]
    fn load_kernel32_succeeds() {
        let h = load(Path::new("kernel32.dll"));
        assert!(!h.is_null(), "LoadLibraryW(kernel32.dll) returned null");
    }

    #[cfg(windows)]
    #[test]
    fn get_loaded_kernel32_succeeds() {
        let h = get_loaded("kernel32.dll");
        assert!(!h.is_null());
    }

    #[cfg(windows)]
    #[test]
    fn get_symbol_load_library_w_resolves() {
        let h = get_loaded("kernel32.dll");
        assert!(!h.is_null());
        let sym = unsafe { get_symbol(h, "LoadLibraryW") };
        assert!(!sym.is_null());
    }

    #[cfg(windows)]
    #[test]
    fn get_symbol_missing_returns_null() {
        let h = get_loaded("kernel32.dll");
        let sym = unsafe { get_symbol(h, "ThisExportDoesNotExist_xyz") };
        assert!(sym.is_null());
    }

    #[cfg(windows)]
    #[test]
    fn current_directory_is_nonempty() {
        assert!(!get_current_directory_path().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn system_directory_is_nonempty() {
        assert!(!get_system_directory_path().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn main_executable_path_is_nonempty() {
        let p = get_main_executable_path();
        assert!(!p.as_os_str().is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn stubs_return_null() {
        assert!(load(Path::new("foo")).is_null());
        assert!(get_loaded("foo").is_null());
        assert!(get_current_directory_path().is_empty());
    }
}
