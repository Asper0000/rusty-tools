//! In-process memory probing. Source: `OSTPlatform/Windows/Memory.cpp`
//!
//! Three functions:
//!
//! * `get_module_image`  - base address and size of a loaded module.
//! * `write_executable_byte` - patch one byte in executable memory with
//!   the VirtualProtect dance and instruction-cache flush.
//! * `is_readable` - probe whether a byte range is committed and readable.
//!   Used before dereferencing a pointer of unknown provenance.
//!
//! The C++ `IsReadable` walks every region the range spans, because a
//! single VirtualQuery only describes the region containing the start
//! address. We do the same here.

use super::dynamic_library::ModuleHandle;
use std::ffi::c_void;

#[derive(Copy, Clone, Debug)]
pub struct ModuleImage {
    pub base: *mut u8,
    pub size: usize,
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows_sys::Win32::System::Memory::{
        VirtualProtect, VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOCACHE, PAGE_READONLY,
        PAGE_READWRITE, PAGE_WRITECOMBINE, PAGE_WRITECOPY,
    };
    use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    pub(super) fn get_module_image(module: ModuleHandle) -> Option<ModuleImage> {
        if module.0.is_null() {
            return None;
        }
        let mut info: MODULEINFO = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            GetModuleInformation(
                GetCurrentProcess(),
                module.0,
                &mut info,
                std::mem::size_of::<MODULEINFO>() as u32,
            )
        };
        if ok == 0 {
            crate::ostp_debug!(
                "GetModuleInformation(module={:p}) failed (error={})",
                module.0,
                super::super::dynamic_library::get_last_error_code()
            );
            return None;
        }
        Some(ModuleImage {
            base: info.lpBaseOfDll as *mut u8,
            size: info.SizeOfImage as usize,
        })
    }

    pub(super) fn write_executable_byte(target: *mut c_void, value: u8) -> bool {
        if target.is_null() {
            crate::ostp_warn!("write_executable_byte: target is null");
            return false;
        }

        let mut old_protect: u32 = 0;
        let ok = unsafe { VirtualProtect(target, 1, PAGE_EXECUTE_READWRITE, &mut old_protect) };
        if ok == 0 {
            crate::ostp_warn!(
                "write_executable_byte({:p}) VirtualProtect(RWX) failed (error={})",
                target,
                super::super::dynamic_library::get_last_error_code()
            );
            return false;
        }

        unsafe { *(target as *mut u8) = value };

        let mut ignored: u32 = 0;
        let ok2 = unsafe { VirtualProtect(target, 1, old_protect, &mut ignored) };
        if ok2 == 0 {
            crate::ostp_warn!(
                "write_executable_byte({:p}) VirtualProtect(restore=0x{:X}) failed (error={})",
                target,
                old_protect,
                super::super::dynamic_library::get_last_error_code()
            );
            return false;
        }

        let ok3 = unsafe { FlushInstructionCache(GetCurrentProcess(), target, 1) };
        if ok3 == 0 {
            crate::ostp_warn!(
                "write_executable_byte({:p}) FlushInstructionCache failed (error={})",
                target,
                super::super::dynamic_library::get_last_error_code()
            );
            return false;
        }

        true
    }

    /// Protections that permit a read. Bare `PAGE_EXECUTE` (execute-only)
    /// and `PAGE_NOACCESS` are deliberately excluded.
    fn is_readable_protect(protect: u32) -> bool {
        // Strip the modifier bits - they combine with the base protection.
        let base = protect & !(PAGE_GUARD | PAGE_NOCACHE | PAGE_WRITECOMBINE);
        matches!(
            base,
            PAGE_READONLY
                | PAGE_READWRITE
                | PAGE_WRITECOPY
                | PAGE_EXECUTE_READ
                | PAGE_EXECUTE_READWRITE
                | PAGE_EXECUTE_WRITECOPY
        )
    }

    pub(super) fn is_readable(addr: *const c_void, bytes: usize) -> bool {
        if addr.is_null() || bytes == 0 {
            return false;
        }

        let start = addr as usize;
        if start > usize::MAX - bytes {
            return false; // wraps
        }
        let end = start + bytes;

        let mut cursor = start;
        while cursor < end {
            let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
            let n = unsafe {
                VirtualQuery(
                    cursor as *const c_void,
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if n != std::mem::size_of::<MEMORY_BASIC_INFORMATION>() {
                return false;
            }

            if mbi.State != MEM_COMMIT
                || (mbi.Protect & PAGE_GUARD) != 0
                || !is_readable_protect(mbi.Protect)
            {
                return false;
            }

            let region_end = mbi.BaseAddress as usize + mbi.RegionSize;
            if region_end <= cursor {
                return false; // no forward progress
            }
            cursor = region_end;
        }
        true
    }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn get_module_image(_module: ModuleHandle) -> Option<ModuleImage> { None }
    pub(super) fn write_executable_byte(_target: *mut c_void, _value: u8) -> bool { false }
    pub(super) fn is_readable(_addr: *const c_void, _bytes: usize) -> bool { false }
}

// ─── Public API ─────────────────────────────────────────────────────

pub fn get_module_image(module: ModuleHandle) -> Option<ModuleImage> {
    imp::get_module_image(module)
}

/// # Safety
///
/// `target` must point to memory that can be made writable and is safe
/// to overwrite with a single byte.
pub unsafe fn write_executable_byte(target: *mut c_void, value: u8) -> bool {
    imp::write_executable_byte(target, value)
}

pub fn is_readable(addr: *const c_void, bytes: usize) -> bool {
    imp::is_readable(addr, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_addr_is_not_readable() {
        assert!(!is_readable(std::ptr::null(), 1));
        assert!(!is_readable(std::ptr::null(), 0));
    }

    #[test]
    fn zero_bytes_is_not_readable() {
        let x = 5u8;
        assert!(!is_readable(&x as *const u8 as *const c_void, 0));
    }

    #[cfg(windows)]
    #[test]
    fn stack_local_is_readable() {
        let x = [0u8; 16];
        assert!(is_readable(x.as_ptr() as *const c_void, x.len()));
    }

    #[cfg(windows)]
    #[test]
    fn current_module_image_resolves() {
        // GetModuleHandleW(NULL) returns the current executable's handle.
        let h = crate::platform::dynamic_library::get_loaded("");
        // "" has no module name; use a real one instead.
        let kernel = crate::platform::dynamic_library::get_loaded("kernel32.dll");
        assert!(!kernel.is_null());
        let img = get_module_image(kernel);
        assert!(img.is_some());
        let img = img.unwrap();
        assert!(!img.base.is_null());
        assert!(img.size > 0);
        let _ = h;
    }

    #[cfg(windows)]
    #[test]
    fn null_module_image_is_none() {
        let h = crate::platform::dynamic_library::ModuleHandle::null();
        assert!(get_module_image(h).is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_stubs() {
        assert!(get_module_image(
            crate::platform::dynamic_library::ModuleHandle::null()
        )
        .is_none());
        assert!(!is_readable(std::ptr::null(), 1));
    }
}
