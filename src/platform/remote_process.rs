//! Remote-process DLL injection. Source: `OSTPlatform/Windows/RemoteProcess.cpp`
//!
//! Classic `LoadLibraryW` injection:
//!   1. Open the target with `PROCESS_CREATE_THREAD | QUERY_INFORMATION |
//!      VM_OPERATION | VM_WRITE | VM_READ`.
//!   2. `VirtualAllocEx` a buffer for the wide library path.
//!   3. `WriteProcessMemory` the path.
//!   4. `CreateRemoteThread(kernel32!LoadLibraryW, path_addr)`.
//!   5. `WaitForSingleObject` + `GetExitCodeThread` -> the module handle,
//!      or zero on failure.
//!   6. `VirtualFreeEx` the path buffer.
//!
//! The C++ resolves `LoadLibraryW` by parsing kernel32's export table
//! from disk (`PE::Image::FindExport`) and adding `modBaseAddr`. We do
//! the same via `crate::platform::pe::Image` and
//! `crate::platform::process::enumerate_modules`.

use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Architecture { Unknown, X86, X64 }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InjectStatus {
    Ok,
    OpenProcessFailed,
    UnknownArchitecture,
    AllocFailed,
    WriteFailed,
    ResolveLoadLibraryFailed,
    CreateThreadFailed,
    WaitFailed,
    RemoteLoadFailed,
}

impl Architecture {
    pub fn as_str(self) -> &'static str {
        match self {
            Architecture::Unknown => "Unknown",
            Architecture::X86 => "X86",
            Architecture::X64 => "X64",
        }
    }
}

impl InjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            InjectStatus::Ok => "Ok",
            InjectStatus::OpenProcessFailed => "OpenProcessFailed",
            InjectStatus::UnknownArchitecture => "UnknownArchitecture",
            InjectStatus::AllocFailed => "AllocFailed",
            InjectStatus::WriteFailed => "WriteFailed",
            InjectStatus::ResolveLoadLibraryFailed => "ResolveLoadLibraryFailed",
            InjectStatus::CreateThreadFailed => "CreateThreadFailed",
            InjectStatus::WaitFailed => "WaitFailed",
            InjectStatus::RemoteLoadFailed => "RemoteLoadFailed",
        }
    }
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, IsWow64Process, OpenProcess,
        WaitForSingleObject, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    const INJECT_ACCESS: u32 = PROCESS_CREATE_THREAD
        | PROCESS_QUERY_INFORMATION
        | PROCESS_VM_OPERATION
        | PROCESS_VM_WRITE
        | PROCESS_VM_READ;

    pub(super) fn detect_arch(h: *mut c_void) -> Architecture {
        let mut wow64: i32 = 0;
        let ok = unsafe { IsWow64Process(h, &mut wow64) };
        if ok == 0 { return Architecture::Unknown; }
        if wow64 != 0 {
            return Architecture::X86;
        }
        // Native process.
        #[cfg(target_pointer_width = "64")]
        { Architecture::X64 }
        #[cfg(target_pointer_width = "32")]
        { Architecture::X86 }
    }

    pub(super) fn get_architecture(pid: u32) -> Architecture {
        if pid == 0 { return Architecture::Unknown; }
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION_SAFE, 0, pid) };
        if h.is_null() {
            crate::ostp_debug!(
                "OpenProcess(pid={}) failed (error={})",
                pid, unsafe { GetLastError() }
            );
            return Architecture::Unknown;
        }
        let a = detect_arch(h);
        unsafe { CloseHandle(h); }
        a
    }

    // PROCESS_QUERY_LIMITED_INFORMATION is 0x1000.
    const PROCESS_QUERY_LIMITED_INFORMATION_SAFE: u32 = 0x1000;

    fn wide_module_name(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn get_module_path(pid: u32, name: &str) -> Option<PathBuf> {
        let mods = crate::platform::process::enumerate_modules(pid);
        mods.into_iter()
            .find(|m| {
                m.native_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .map(|f| f.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            })
            .map(|m| m.native_path)
    }

    fn find_remote_module_base(pid: u32, name: &str) -> Option<u64> {
        let mods = crate::platform::process::enumerate_modules(pid);
        // We did not keep base address in ModuleInfo. For injection we
        // need it; re-snapshot with Toolhelp directly.
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W,
            TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
        };
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if snap == INVALID_HANDLE_VALUE { return None; }
        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut result = None;
        if unsafe { Module32FirstW(snap, &mut entry) } != 0 {
            loop {
                let nul = entry.szModule.iter().position(|&c| c == 0).unwrap_or(256);
                let mname = String::from_utf16_lossy(&entry.szModule[..nul]);
                if mname.eq_ignore_ascii_case(name) {
                    result = Some(entry.modBaseAddr as u64);
                    break;
                }
                if unsafe { Module32NextW(snap, &mut entry) } == 0 { break; }
            }
        }
        unsafe { CloseHandle(snap); }
        let _ = mods;
        result
    }

    /// Resolve `LoadLibraryW` in the target process.
    fn resolve_remote_load_library(pid: u32, arch: Architecture) -> Option<*mut c_void> {
        let _ = arch;
        // Get the on-disk path of kernel32 in the target.
        let k32_path = get_module_path(pid, "kernel32.dll")
            .or_else(|| get_module_path(pid, "kernelbase.dll"))?;
        let img = crate::platform::pe::Image::new(&k32_path);
        if !img.is_valid() { return None; }

        let export = img.find_export("LoadLibraryW")?;
        if export.is_fwd() {
            // Forwarder: parse "MODULE.Symbol" and try once more.
            let fwd = &export.forwarder;
            let dot = fwd.find('.')?;
            let mod_name = &fwd[..dot];
            let sym_name = &fwd[dot + 1..];
            if sym_name.starts_with('#') { return None; }
            let mod_file = if mod_name.contains('.') {
                mod_name.to_string()
            } else {
                format!("{}.dll", mod_name)
            };
            let p = get_module_path(pid, &mod_file)?;
            let img2 = crate::platform::pe::Image::new(&p);
            if !img2.is_valid() { return None; }
            let e2 = img2.find_export(sym_name)?;
            if e2.is_fwd() { return None; }
            let base = find_remote_module_base(pid, &mod_file)?;
            return Some((base + e2.rva as u64) as *mut c_void);
        }

        let base = find_remote_module_base(pid, "kernel32.dll")?;
        Some((base + export.rva as u64) as *mut c_void)
    }

    pub(super) fn inject_library(pid: u32, library_path: &Path) -> InjectStatus {
        if pid == 0 { return InjectStatus::OpenProcessFailed; }
        let h = unsafe { OpenProcess(INJECT_ACCESS, 0, pid) };
        if h.is_null() {
            crate::ostp_warn!(
                "OpenProcess(pid={}, access=0x{:X}) failed (error={})",
                pid, INJECT_ACCESS, unsafe { GetLastError() }
            );
            return InjectStatus::OpenProcessFailed;
        }

        let arch = detect_arch(h);
        if arch == Architecture::Unknown {
            unsafe { CloseHandle(h); }
            return InjectStatus::UnknownArchitecture;
        }

        let load_library = match resolve_remote_load_library(pid, arch) {
            Some(f) => f,
            None => {
                unsafe { CloseHandle(h); }
                return InjectStatus::ResolveLoadLibraryFailed;
            }
        };

        // Wide path with trailing NUL.
        let wide: Vec<u16> = library_path
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let bytes = wide.len() * 2;

        let remote = unsafe {
            VirtualAllocEx(h, std::ptr::null(), bytes, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE)
        };
        if remote.is_null() {
            crate::ostp_warn!("VirtualAllocEx(size={}) failed (error={})", bytes, unsafe { GetLastError() });
            unsafe { CloseHandle(h); }
            return InjectStatus::AllocFailed;
        }

        let mut written: usize = 0;
        let ok = unsafe {
            WriteProcessMemory(h, remote, wide.as_ptr() as *const c_void, bytes, &mut written)
        };
        if ok == 0 || written != bytes {
            crate::ostp_warn!(
                "WriteProcessMemory(size={}) failed (written={}, error={})",
                bytes, written, unsafe { GetLastError() }
            );
            unsafe {
                VirtualFreeEx(h, remote, 0, MEM_RELEASE);
                CloseHandle(h);
            }
            return InjectStatus::WriteFailed;
        }

        let thread = unsafe {
            CreateRemoteThread(
                h,
                std::ptr::null(),
                0,
                Some(std::mem::transmute::<*mut c_void, unsafe extern "system" fn(*mut c_void) -> u32>(load_library)),
                remote,
                0,
                std::ptr::null_mut(),
            )
        };
        if thread.is_null() {
            crate::ostp_warn!("CreateRemoteThread failed (error={})", unsafe { GetLastError() });
            unsafe {
                VirtualFreeEx(h, remote, 0, MEM_RELEASE);
                CloseHandle(h);
            }
            return InjectStatus::CreateThreadFailed;
        }

        let wait = unsafe { WaitForSingleObject(thread, 10_000) };
        if wait != WAIT_OBJECT_0 {
            crate::ostp_warn!("WaitForSingleObject(remote thread) returned {} (error={})", wait, unsafe { GetLastError() });
            unsafe {
                VirtualFreeEx(h, remote, 0, MEM_RELEASE);
                CloseHandle(thread);
                CloseHandle(h);
            }
            return InjectStatus::WaitFailed;
        }

        let mut exit_code: u32 = 0;
        let ok = unsafe { GetExitCodeThread(thread, &mut exit_code) };
        if ok == 0 {
            crate::ostp_warn!("GetExitCodeThread failed (error={})", unsafe { GetLastError() });
            unsafe {
                VirtualFreeEx(h, remote, 0, MEM_RELEASE);
                CloseHandle(thread);
                CloseHandle(h);
            }
            return InjectStatus::RemoteLoadFailed;
        }

        unsafe {
            VirtualFreeEx(h, remote, 0, MEM_RELEASE);
            CloseHandle(thread);
            CloseHandle(h);
        }

        if exit_code == 0 {
            crate::ostp_warn!("Remote LoadLibraryW returned 0 for '{}'", library_path.display());
            return InjectStatus::RemoteLoadFailed;
        }
        InjectStatus::Ok
    }
}

// Small helper on the PE Export to detect forwarders without importing
// the private field.
#[cfg(windows)]
trait ExportFwd {
    fn is_fwd(&self) -> bool;
}
#[cfg(windows)]
impl ExportFwd for crate::platform::pe::Export {
    fn is_fwd(&self) -> bool { self.is_forwarder() }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn get_architecture(_pid: u32) -> Architecture { Architecture::Unknown }
    pub(super) fn inject_library(_pid: u32, _path: &Path) -> InjectStatus {
        InjectStatus::OpenProcessFailed
    }
}

pub fn get_architecture(pid: u32) -> Architecture { imp::get_architecture(pid) }
pub fn inject_library(pid: u32, library_path: &Path) -> InjectStatus {
    imp::inject_library(pid, library_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inject_status_strings() {
        assert_eq!(InjectStatus::Ok.as_str(), "Ok");
        assert_eq!(InjectStatus::OpenProcessFailed.as_str(), "OpenProcessFailed");
        assert_eq!(InjectStatus::RemoteLoadFailed.as_str(), "RemoteLoadFailed");
    }

    #[test]
    fn architecture_strings() {
        assert_eq!(Architecture::Unknown.as_str(), "Unknown");
        assert_eq!(Architecture::X86.as_str(), "X86");
        assert_eq!(Architecture::X64.as_str(), "X64");
    }

    #[cfg(windows)]
    #[test]
    fn current_process_architecture_is_known() {
        let pid = std::process::id();
        let a = get_architecture(pid);
        assert_ne!(a, Architecture::Unknown);
    }

    #[cfg(windows)]
    #[test]
    fn nonexistent_pid_returns_unknown() {
        let a = get_architecture(0xFFFF_FFF0u32);
        assert_eq!(a, Architecture::Unknown);
    }
}
