//! xinput1_4.dll loader-hijack proxy.
//!
//! Placed next to `steam.exe`, this DLL is loaded in preference to the
//! system `%WINDIR%\System32\xinput1_4.dll`. Every export forwards to the
//! real DLL, which is loaded lazily on first call so a missing XInput
//! runtime on the host does not fail the proxy load.
//!
//! Wrapper functions exist (unlike the dwmapi proxy) because the real DLL
//! exposes undocumented ordinals — most notably 100, `XInputGetStateEx`,
//! which includes the guide-button state. Steam Big Picture reads the
//! guide button through that ordinal, so the proxy must forward it
//! explicitly. The ordinals are assigned by `xinput1_4.def`.
//!
//! `DllMain` additionally loads `RustyTools.dll` when the host is
//! `steam.exe`, exactly as the dwmapi proxy does.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

#[cfg(not(windows))]
mod stub {}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::ERROR_DEVICE_NOT_CONNECTED;
    use windows_sys::Win32::System::LibraryLoader::{
        DisableThreadLibraryCalls, GetModuleFileNameA, GetProcAddress, LoadLibraryA,
        LoadLibraryW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    // ── Resolved real-DLL function pointer types ──────────────────────
    // Signatures copy from the C++ source verbatim. `WINAPI` / `CALLBACK`
    // is `extern "system"`; DWORD is `u32`, BYTE is `u8`, BOOL is `i32`,
    // LPWSTR is `*mut u16`, UINT is `u32`, UINT* is `*mut u32`.

    type GetState_t           = unsafe extern "system" fn(u32, *mut c_void) -> u32;
    type SetState_t           = unsafe extern "system" fn(u32, *mut c_void) -> u32;
    type GetCapabilities_t    = unsafe extern "system" fn(u32, u32, *mut c_void) -> u32;
    type Enable_t             = unsafe extern "system" fn(i32);
    type GetAudioDeviceIds_t  = unsafe extern "system" fn(u32, *mut u16, *mut u32, *mut u16, *mut u32) -> u32;
    type GetBatteryInfo_t     = unsafe extern "system" fn(u32, u8, *mut c_void) -> u32;
    type GetKeystroke_t       = unsafe extern "system" fn(u32, u32, *mut c_void) -> u32;

    struct RealXInput {
        _module: *mut c_void,
        get_state: Option<GetState_t>,
        set_state: Option<SetState_t>,
        get_capabilities: Option<GetCapabilities_t>,
        enable: Option<Enable_t>,
        get_audio_device_ids: Option<GetAudioDeviceIds_t>,
        get_battery_information: Option<GetBatteryInfo_t>,
        get_keystroke: Option<GetKeystroke_t>,
        // Ordinal-only slots. The proxy calls through these but never
        // exports them under a name.
        ord_100: *mut c_void, // XInputGetStateEx - guide button
        ord_101: *mut c_void, // XInputWaitForGuideButton
        ord_102: *mut c_void, // XInputCancelGuideButtonWait
        ord_103: *mut c_void, // XInputPowerOffController
        ord_104: *mut c_void, // XInputGetBaseBusInformation
        ord_108: *mut c_void, // XInputGetAudioDeviceIdsEx
    }

    // The real module handle is process-global; the pointers are opaque and
    // never freed. `OnceLock` serialises the one-time load and hands every
    // later caller a shared reference.
    unsafe impl Send for RealXInput {}
    unsafe impl Sync for RealXInput {}

    static REAL: OnceLock<Option<RealXInput>> = OnceLock::new();

    /// Return the resolved real XInput, loading it on first use. `None` when
    /// the system library is missing or none of the expected exports resolve
    /// — every wrapper falls back to `ERROR_DEVICE_NOT_CONNECTED` in that case
    /// without panicking.
    fn real() -> Option<&'static RealXInput> {
        REAL.get_or_init(load_real_xinput).as_ref()
    }

    fn to_wide(path: &str) -> Vec<u16> {
        path.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn system_directory() -> Option<String> {
        let mut buf = [0u16; 260];
        let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
        if n == 0 || (n as usize) >= buf.len() {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }

    /// Resolve an export by name. Returns `None` when the symbol is absent
    /// — treated as "this build does not support it", not an error.
    unsafe fn resolve_named<T: Copy>(module: *mut c_void, name: &str) -> Option<T> {
        let cname = std::ffi::CString::new(name).ok()?;
        let p = GetProcAddress(module, cname.as_ptr() as *const u8);
        match p {
            Some(f) => Some(std::mem::transmute_copy(&(f as *mut c_void))),
            None => None,
        }
    }

    /// Resolve an export by ordinal. `GetProcAddress` treats an integer
    /// cast into the name pointer as an ordinal lookup; anything below
    /// 0x10000 is unambiguously an ordinal because no real module exports
    /// a string that would fit in that range.
    unsafe fn resolve_ordinal(module: *mut c_void, ordinal: u16) -> *mut c_void {
        let p = ordinal as usize as *const u8;
        match GetProcAddress(module, p) {
            Some(f) => f as *mut c_void,
            None => std::ptr::null_mut(),
        }
    }

    fn load_real_xinput() -> Option<RealXInput> {
        let sysdir = system_directory()?;
        let path = format!("{}\\xinput1_4.dll", sysdir);
        let wide = to_wide(&path);

        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            return None;
        }

        unsafe {
            Some(RealXInput {
                _module: module,
                get_state: resolve_named(module, "XInputGetState"),
                set_state: resolve_named(module, "XInputSetState"),
                get_capabilities: resolve_named(module, "XInputGetCapabilities"),
                enable: resolve_named(module, "XInputEnable"),
                get_audio_device_ids: resolve_named(module, "XInputGetAudioDeviceIds"),
                get_battery_information: resolve_named(module, "XInputGetBatteryInformation"),
                get_keystroke: resolve_named(module, "XInputGetKeystroke"),
                ord_100: resolve_ordinal(module, 100),
                ord_101: resolve_ordinal(module, 101),
                ord_102: resolve_ordinal(module, 102),
                ord_103: resolve_ordinal(module, 103),
                ord_104: resolve_ordinal(module, 104),
                ord_108: resolve_ordinal(module, 108),
            })
        }
    }

    // ── Named exports ─────────────────────────────────────────────────
    // Each wrapper forwards to the resolved real function, or returns
    // ERROR_DEVICE_NOT_CONNECTED (1167) if the real library or the export
    // is unavailable. Never panics.

    #[no_mangle]
    pub unsafe extern "system" fn XInputGetState(dwUserIndex: u32, pState: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.get_state {
            Some(f) => f(dwUserIndex, pState),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputSetState(dwUserIndex: u32, pVibration: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.set_state {
            Some(f) => f(dwUserIndex, pVibration),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputGetCapabilities(dwUserIndex: u32, dwFlags: u32, pCapabilities: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.get_capabilities {
            Some(f) => f(dwUserIndex, dwFlags, pCapabilities),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputEnable(enable: i32) {
        let Some(r) = real() else { return };
        if let Some(f) = r.enable {
            f(enable);
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputGetAudioDeviceIds(
        dwUserIndex: u32,
        pRenderDeviceId: *mut u16,
        pRenderCount: *mut u32,
        pCaptureDeviceId: *mut u16,
        pCaptureCount: *mut u32,
    ) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.get_audio_device_ids {
            Some(f) => f(dwUserIndex, pRenderDeviceId, pRenderCount, pCaptureDeviceId, pCaptureCount),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputGetBatteryInformation(dwUserIndex: u32, devType: u8, pBatteryInformation: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.get_battery_information {
            Some(f) => f(dwUserIndex, devType, pBatteryInformation),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputGetKeystroke(dwUserIndex: u32, dwReserved: u32, pKeystroke: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        match r.get_keystroke {
            Some(f) => f(dwUserIndex, dwReserved, pKeystroke),
            None => ERROR_DEVICE_NOT_CONNECTED,
        }
    }

    // ── Ordinal-only exports ──────────────────────────────────────────
    // Named XInputOrdinalNNN in Rust; the .def exports them at their true
    // ordinals with NONAME so no symbol name leaks.

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal100(a1: u32, a2: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_100.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: GetState_t = std::mem::transmute(r.ord_100);
        f(a1, a2)
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal101(a1: u32, a2: u32, a3: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_101.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: unsafe extern "system" fn(u32, u32, *mut c_void) -> u32 = std::mem::transmute(r.ord_101);
        f(a1, a2, a3)
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal102(a1: u32) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_102.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: unsafe extern "system" fn(u32) -> u32 = std::mem::transmute(r.ord_102);
        f(a1)
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal103(a1: u32) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_103.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: unsafe extern "system" fn(u32) -> u32 = std::mem::transmute(r.ord_103);
        f(a1)
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal104(a1: u32, a2: *mut c_void) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_104.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: unsafe extern "system" fn(u32, *mut c_void) -> u32 = std::mem::transmute(r.ord_104);
        f(a1, a2)
    }

    #[no_mangle]
    pub unsafe extern "system" fn XInputOrdinal108(
        a1: u32, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void, a5: *mut c_void,
    ) -> u32 {
        let Some(r) = real() else { return ERROR_DEVICE_NOT_CONNECTED };
        if r.ord_108.is_null() { return ERROR_DEVICE_NOT_CONNECTED }
        let f: unsafe extern "system" fn(u32, *mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32 =
            std::mem::transmute(r.ord_108);
        f(a1, a2, a3, a4, a5)
    }

    // ── Injection ─────────────────────────────────────────────────────
    // Only steam.exe gets RustyTools.dll loaded. Same rule as the dwmapi
    // proxy; identical body on purpose so the two proxies read the same.

    fn is_steam_host() -> bool {
        let mut buf = [0u8; 260];
        let n = unsafe {
            GetModuleFileNameA(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32)
        };
        if n == 0 { return false; }
        let path = &buf[..n as usize];
        let name_start = path.iter().rposition(|&b| b == b'\\').map(|i| i + 1).unwrap_or(0);
        let name = &path[name_start..];
        name.eq_ignore_ascii_case(b"steam.exe")
    }

    fn rustytools_load() -> bool {
        if !is_steam_host() { return true; }
        let name = b"RustyTools.dll\0";
        !unsafe { LoadLibraryA(name.as_ptr()) }.is_null()
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllMain(
        hmodule: *mut c_void,
        reason: u32,
        _reserved: *mut c_void,
    ) -> i32 {
        match reason {
            DLL_PROCESS_ATTACH => {
                DisableThreadLibraryCalls(hmodule);
                // EAGER LOAD IN DllMain: matches the C++ behaviour. The
                // real XInput DLL is loaded here rather than lazily on
                // first wrapper call. Lazy load was a Rust-port
                // divergence intended to avoid loader-lock re-entrancy;
                // it turned out to cause a different, worse re-entrancy
                // when steamwebhelper's Chromium UI calls XInputGetState
                // from a context close to the loader.
                let _ = real();
                if !rustytools_load() {
                    return 0; // FALSE
                }
            }
            DLL_PROCESS_DETACH => {}
            _ => {}
        }
        1 // TRUE
    }
}
