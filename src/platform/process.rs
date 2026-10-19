//! Process inspection. Source: `OSTPlatform/Windows/Process.cpp`

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ModuleInfo {
    pub path: String,
    pub native_path: PathBuf,
    pub size: u32,
    pub executable: bool,
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, FILETIME, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W,
        TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    #[repr(C)]
    pub(super) struct ProcessBasicInformation {
        pub exit_status: i32,
        pub peb_base_address: *mut c_void,
        pub affinity_mask: usize,
        pub base_priority: i32,
        pub unique_process_id: usize,
        pub inherited_from_unique_process_id: usize,
    }

    type NtQueryInformationProcessFn = unsafe extern "system" fn(
        HANDLE, u32, *mut c_void, u32, *mut u32,
    ) -> i32;

    type NtQueryVirtualMemoryFn = unsafe extern "system" fn(
        HANDLE, *mut c_void, u32, *mut c_void, usize, *mut usize,
    ) -> i32;

    const PROCESS_BASIC_INFORMATION: u32 = 0;
    const WOW64_INFORMATION: u32 = 26;
    const MEMORY_BASIC_INFORMATION_CLASS: u32 = 0;

    fn resolve_ntdll(name: &str) -> *mut c_void {
        unsafe {
            let ntdll: Vec<u16> = "ntdll.dll".encode_utf16().chain(std::iter::once(0)).collect();
            let h = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(ntdll.as_ptr());
            if h.is_null() { return std::ptr::null_mut(); }
            let n = std::ffi::CString::new(name).unwrap_or_default();
            match windows_sys::Win32::System::LibraryLoader::GetProcAddress(h, n.as_ptr() as *const u8) {
                Some(f) => f as *mut c_void,
                None => std::ptr::null_mut(),
            }
        }
    }

    fn open_process(pid: u32, access: u32) -> HANDLE {
        if pid == 0 { return std::ptr::null_mut(); }
        let h = unsafe { OpenProcess(access, 0, pid) };
        if h.is_null() {
            crate::ostp_debug!(
                "OpenProcess(pid={}, access=0x{:X}) failed (error={})",
                pid, access, unsafe { GetLastError() }
            );
        }
        h
    }

    fn wide_to_string(buf: &[u16]) -> String {
        String::from_utf16_lossy(buf)
    }

    pub(super) fn get_creation_time(pid: u32) -> Option<u64> {
        let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION);
        if h.is_null() { return None; }
        let mut c: FILETIME = unsafe { std::mem::zeroed() };
        let mut e: FILETIME = unsafe { std::mem::zeroed() };
        let mut k: FILETIME = unsafe { std::mem::zeroed() };
        let mut u: FILETIME = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) };
        unsafe { CloseHandle(h); }
        if ok == 0 {
            crate::ostp_debug!("GetProcessTimes(pid={}) failed (error={})", pid, unsafe { GetLastError() });
            return None;
        }
        Some(((c.dwHighDateTime as u64) << 32) | (c.dwLowDateTime as u64))
    }

    pub(super) fn get_image_path(pid: u32) -> Option<String> {
        let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION);
        if h.is_null() { return None; }

        for cap in [260u32, 32768u32].iter() {
            let mut buf = vec![0u16; *cap as usize];
            let mut size: u32 = *cap;
            let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size) };
            if ok != 0 {
                buf.truncate(size as usize);
                unsafe { CloseHandle(h); }
                return Some(wide_to_string(&buf));
            }
            let err = unsafe { GetLastError() };
            if err != 122 {
                crate::ostp_debug!("QueryFullProcessImageNameW(pid={}) failed (error={})", pid, err);
                unsafe { CloseHandle(h); }
                return None;
            }
        }
        unsafe { CloseHandle(h); }
        None
    }

    pub(super) fn enumerate_modules(pid: u32) -> Vec<ModuleInfo> {
        let mut out = Vec::new();
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if snap == INVALID_HANDLE_VALUE {
            crate::ostp_debug!(
                "CreateToolhelp32Snapshot(pid={}) failed (error={})",
                pid, unsafe { GetLastError() }
            );
            return out;
        }

        let mut entry: MODULEENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<MODULEENTRY32W>() as u32;

        if unsafe { Module32FirstW(snap, &mut entry) } != 0 {
            loop {
                let nul = entry.szExePath.iter().position(|&c| c == 0).unwrap_or(260);
                let path = wide_to_string(&entry.szExePath[..nul]);
                if !path.is_empty() {
                    let executable =
                        path.len() >= 4 && path[path.len() - 4..].eq_ignore_ascii_case(".exe");
                    out.push(ModuleInfo {
                        native_path: PathBuf::from(&path),
                        path,
                        size: entry.modBaseSize,
                        executable,
                    });
                }
                if unsafe { Module32NextW(snap, &mut entry) } == 0 { break; }
            }
        }
        unsafe { CloseHandle(snap); }
        out
    }

    pub(super) fn launch_detached_hidden(command_line: &str) -> bool {
        use windows_sys::Win32::System::Threading::{
            CreateProcessW, CREATE_BREAKAWAY_FROM_JOB, CREATE_NO_WINDOW, PROCESS_INFORMATION,
            STARTUPINFOW, STARTF_USESHOWWINDOW,
        };

        let mut wide: Vec<u16> = command_line.encode_utf16().collect();
        wide.push(0);

        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW;
        si.wShowWindow = 0;

        for extra in [CREATE_BREAKAWAY_FROM_JOB, 0u32] {
            let mut cmd = wide.clone();
            let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
            let ok = unsafe {
                CreateProcessW(
                    std::ptr::null(),
                    cmd.as_mut_ptr(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    CREATE_NO_WINDOW | extra,
                    std::ptr::null(),
                    std::ptr::null(),
                    &si,
                    &mut pi,
                )
            };
            if ok != 0 {
                unsafe { CloseHandle(pi.hThread); CloseHandle(pi.hProcess); }
                return true;
            }
            crate::ostp_debug!(
                "launch_detached_hidden: CreateProcessW failed (extra=0x{:X}, error={})",
                extra, unsafe { GetLastError() }
            );
        }
        crate::ostp_warn!("launch_detached_hidden: could not launch '{}'", command_line);
        false
    }

    unsafe fn read_remote<T: Copy>(h: HANDLE, addr: *const c_void) -> Option<T> {
        let mut value: T = std::mem::zeroed();
        let mut read: usize = 0;
        let ok = windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            h,
            addr,
            &mut value as *mut T as *mut c_void,
            std::mem::size_of::<T>(),
            &mut read,
        );
        if ok == 0 || read != std::mem::size_of::<T>() { None } else { Some(value) }
    }

    const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
    const PARAMS_COMMAND_LINE_OFFSET: usize = 0x70;
    const PARAMS_ENVIRONMENT_OFFSET: usize = 0x80;

    struct ParamsLocation {
        command_line: *const c_void,
        env_addr: *const c_void,
        env_len: usize,
    }

    unsafe fn resolve_params(h: HANDLE) -> Option<ParamsLocation> {
        let ptr = resolve_ntdll("NtQueryInformationProcess");
        if ptr.is_null() { return None; }
        let nt_qip: NtQueryInformationProcessFn = std::mem::transmute(ptr);

        let mut info: ProcessBasicInformation = std::mem::zeroed();
        let st = nt_qip(
            h,
            PROCESS_BASIC_INFORMATION,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<ProcessBasicInformation>() as u32,
            std::ptr::null_mut(),
        );
        if st < 0 || info.peb_base_address.is_null() { return None; }

        let params_addr = info.peb_base_address as usize + PEB_PROCESS_PARAMETERS_OFFSET;
        let params: *const c_void = read_remote::<*const c_void>(h, params_addr as *const c_void)?;
        if params.is_null() { return None; }

        let cmd_ptr_addr = params as usize + PARAMS_COMMAND_LINE_OFFSET;
        let env_ptr_addr = params as usize + PARAMS_ENVIRONMENT_OFFSET;

        let cmd_line_buf_addr = cmd_ptr_addr + 8;
        let cmd_line_buffer: *const c_void =
            read_remote::<*const c_void>(h, cmd_line_buf_addr as *const c_void)?;

        let env_addr: *const c_void =
            read_remote::<*const c_void>(h, env_ptr_addr as *const c_void)?;
        if env_addr.is_null() { return None; }

        let env_len = query_readable_region(h, env_addr).unwrap_or(0);
        if env_len < 4 { return None; }

        Some(ParamsLocation {
            command_line: cmd_line_buffer,
            env_addr,
            env_len,
        })
    }

    unsafe fn query_readable_region(h: HANDLE, addr: *const c_void) -> Option<usize> {
        let ptr = resolve_ntdll("NtQueryVirtualMemory");
        if ptr.is_null() { return None; }
        let nt_qvm: NtQueryVirtualMemoryFn = std::mem::transmute(ptr);

        #[repr(C)]
        struct Mbi {
            base_address: *mut c_void,
            allocation_base: *mut c_void,
            allocation_protect: u32,
            partition_id: u16,
            _pad1: u16,
            region_size: usize,
            state: u32,
            protect: u32,
            ty: u32,
            _pad2: u32,
        }

        let mut info: Mbi = std::mem::zeroed();
        let st = nt_qvm(
            h,
            addr as *mut c_void,
            MEMORY_BASIC_INFORMATION_CLASS,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<Mbi>(),
            std::ptr::null_mut(),
        );
        if st < 0 || info.region_size == 0 { return None; }
        let base = info.base_address as usize;
        let cur = addr as usize;
        if cur < base { return None; }
        let off = cur - base;
        if off >= info.region_size { return None; }
        Some(info.region_size - off)
    }

    pub(super) fn get_environment_variable_value(pid: u32, name: &[u16]) -> Option<String> {
        let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ);
        if h.is_null() { return None; }
        let r = unsafe {
            resolve_params(h).and_then(|p| read_env_from(h, p.env_addr, p.env_len, name))
        };
        unsafe { CloseHandle(h); }
        r
    }

    /// ASCII-only uppercase for a UTF-16 code unit. Environment variable
    /// names are ASCII by convention; this matches Windows' case rules
    /// for those names without pulling in Unicode folding.
    #[inline]
    fn ascii_upper_u16(c: u16) -> u16 {
        if (0x61..=0x7A).contains(&c) { c - 0x20 } else { c }
    }

    unsafe fn read_env_from(
        h: HANDLE,
        env: *const c_void,
        len_bytes: usize,
        name: &[u16],
    ) -> Option<String> {
        let to_read = len_bytes.min(1024 * 1024);
        if to_read < 4 { return None; }
        let mut buf = vec![0u16; to_read / 2];
        let mut read: usize = 0;
        let ok = windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            h,
            env,
            buf.as_mut_ptr() as *mut c_void,
            to_read,
            &mut read,
        );
        if ok == 0 { return None; }
        buf.truncate(read / 2);

        let upper_name: Vec<u16> = name.iter().copied().map(ascii_upper_u16).collect();

        let mut offset = 0usize;
        while offset < buf.len() && buf[offset] != 0 {
            let end = buf[offset..].iter().position(|&c| c == 0)? + offset;
            let entry = &buf[offset..end];
            if entry.len() > upper_name.len() && entry[upper_name.len()] == b'=' as u16 {
                let matches = entry[..upper_name.len()]
                    .iter()
                    .zip(&upper_name)
                    .all(|(a, b)| ascii_upper_u16(*a) == *b);
                if matches {
                    let value = &entry[upper_name.len() + 1..];
                    return Some(String::from_utf16_lossy(value));
                }
            }
            offset = end + 1;
        }
        None
    }

    pub(super) fn get_process_command_line(pid: u32) -> Option<String> {
        let h = open_process(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ);
        if h.is_null() { return None; }

        let result = unsafe {
            read_command_line_wow64(h).or_else(|| {
                resolve_params(h).and_then(|p| {
                    if p.command_line.is_null() { return None; }
                    read_unicode_string(h, p.command_line)
                })
            })
        };
        unsafe { CloseHandle(h); }
        result
    }

    unsafe fn read_unicode_string(h: HANDLE, buf_addr: *const c_void) -> Option<String> {
        if buf_addr.is_null() { return None; }
        let mut buf = vec![0u16; 32768 / 2];
        let mut read: usize = 0;
        let ok = windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            h,
            buf_addr,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() * 2,
            &mut read,
        );
        if ok == 0 { return None; }
        buf.truncate(read / 2);
        if let Some(nul) = buf.iter().position(|&c| c == 0) {
            buf.truncate(nul);
        }
        if buf.is_empty() { return None; }
        Some(String::from_utf16_lossy(&buf))
    }

    unsafe fn read_command_line_wow64(h: HANDLE) -> Option<String> {
        let ptr = resolve_ntdll("NtQueryInformationProcess");
        if ptr.is_null() { return None; }
        let nt_qip: NtQueryInformationProcessFn = std::mem::transmute(ptr);

        let mut peb32: usize = 0;
        let st = nt_qip(
            h,
            WOW64_INFORMATION,
            &mut peb32 as *mut _ as *mut c_void,
            std::mem::size_of::<usize>() as u32,
            std::ptr::null_mut(),
        );
        if st < 0 || peb32 == 0 { return None; }

        let params32_ptr_addr = (peb32 as usize + 0x10) as *const c_void;
        let params32: u32 = read_remote::<u32>(h, params32_ptr_addr)?;
        if params32 == 0 { return None; }

        let cmd32_len_addr = (params32 as usize + 0x40) as *const c_void;
        let cmd32_len: u16 = read_remote::<u16>(h, cmd32_len_addr)?;
        let cmd32_buf_addr = (params32 as usize + 0x44) as *const c_void;
        let cmd32_buf: u32 = read_remote::<u32>(h, cmd32_buf_addr)?;
        if cmd32_buf == 0 || cmd32_len == 0 { return None; }

        let chars = (cmd32_len / 2) as usize;
        if chars == 0 || chars > 32768 / 2 { return None; }
        let mut buf = vec![0u16; chars];
        let mut read: usize = 0;
        let ok = windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory(
            h,
            cmd32_buf as *const c_void,
            buf.as_mut_ptr() as *mut c_void,
            chars * 2,
            &mut read,
        );
        if ok == 0 { return None; }
        buf.truncate(read / 2);
        Some(String::from_utf16_lossy(&buf))
    }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn get_creation_time(_pid: u32) -> Option<u64> { None }
    pub(super) fn get_image_path(_pid: u32) -> Option<String> { None }
    pub(super) fn enumerate_modules(_pid: u32) -> Vec<ModuleInfo> { Vec::new() }
    pub(super) fn launch_detached_hidden(_c: &str) -> bool { false }
    pub(super) fn get_environment_variable_value(_pid: u32, _name: &[u16]) -> Option<String> { None }
    pub(super) fn get_process_command_line(_pid: u32) -> Option<String> { None }
}

// ─── Public API ─────────────────────────────────────────────────────

pub fn get_creation_time(pid: u32) -> Option<u64> { imp::get_creation_time(pid) }

pub fn format_creation_time(file_time: u64) -> String {
    if file_time == 0 { return "-".to_string(); }
    const EPOCH_DIFF: u64 = 116_444_736_000_000_000;
    if file_time < EPOCH_DIFF {
        return format!("filetime={}", file_time);
    }
    let since_unix_100ns = file_time - EPOCH_DIFF;
    let secs = since_unix_100ns / 10_000_000;
    let micros = (since_unix_100ns % 10_000_000) / 10;
    match chrono::DateTime::from_timestamp(secs as i64, (micros * 1000) as u32) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
        None => format!("filetime={}", file_time),
    }
}

pub fn get_image_path(pid: u32) -> Option<String> { imp::get_image_path(pid) }
pub fn get_environment_variable_value(pid: u32, name: &[u16]) -> Option<String> {
    imp::get_environment_variable_value(pid, name)
}
pub fn get_process_command_line(pid: u32) -> Option<String> { imp::get_process_command_line(pid) }
pub fn enumerate_modules(pid: u32) -> Vec<ModuleInfo> { imp::enumerate_modules(pid) }

pub fn is_system_module_path(path: &str) -> bool {
    let windir = match std::env::var("WINDIR") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut norm_dir = windir.to_lowercase().replace('/', "\\");
    if !norm_dir.ends_with('\\') { norm_dir.push('\\'); }
    let norm_path = path.to_lowercase().replace('/', "\\");
    norm_path.starts_with(&norm_dir)
}

pub fn launch_detached_hidden(command_line: &str) -> bool {
    imp::launch_detached_hidden(command_line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_zero_returns_dash() {
        assert_eq!(format_creation_time(0), "-");
    }

    #[test]
    fn format_unix_epoch() {
        let f = 116_444_736_000_000_000u64;
        let s = format_creation_time(f);
        assert!(s.starts_with("1970-01-01"));
    }

    #[test]
    fn is_system_module_path_unrelated() {
        if std::env::var("WINDIR").is_ok() {
            assert!(!is_system_module_path("C:\\Users\\foo\\module.dll"));
        }
    }

    #[test]
    fn is_system_module_path_case_separator_insensitive() {
        let windir = match std::env::var("WINDIR") {
            Ok(v) => v,
            Err(_) => return,
        };
        let probe = format!("{}/system32/kernel32.dll", windir.to_lowercase());
        assert!(is_system_module_path(&probe));
        let probe2 = format!("{}\\SYSTEM32\\KERNEL32.DLL", windir.to_uppercase());
        assert!(is_system_module_path(&probe2));
    }

    #[cfg(windows)]
    #[test]
    fn current_process_creation_time() {
        let pid = std::process::id();
        let t = get_creation_time(pid);
        assert!(t.is_some());
        assert!(t.unwrap() > 0);
    }

    #[cfg(windows)]
    #[test]
    fn current_process_image_path() {
        let pid = std::process::id();
        let p = get_image_path(pid);
        assert!(p.is_some());
        assert!(!p.unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn current_process_modules_nonempty() {
        let pid = std::process::id();
        let mods = enumerate_modules(pid);
        assert!(!mods.is_empty());
        assert!(mods.iter().any(|m| m.path.to_lowercase().contains("kernel32")));
    }

    #[cfg(windows)]
    #[test]
    fn nonexistent_pid_returns_none() {
        let t = get_creation_time(0xFFFF_FFF0u32);
        assert!(t.is_none());
    }
}
