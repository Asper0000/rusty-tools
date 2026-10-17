//! Steam per-app credential store (HKCU registry).
//! Source: `OSTPlatform/Windows/SteamCredentialStore.cpp`
//!
//! `RegGetValueW` returns `WIN32_ERROR` (a `u32` in `windows-sys`),
//! not the `i32` that the Win32 header names `LSTATUS`. All the
//! helper types and constants here use `u32` accordingly.

use super::windows::handles::{UniqueRegKey, HKEY};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    NotFound,
    Unsupported,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "Ok",
            Status::NotFound => "NotFound",
            Status::Unsupported => "Unsupported",
            Status::Failed => "Failed",
        }
    }
}

// ─── Windows implementation ─────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::c_void;
    use windows_sys::Win32::System::Registry::{
        RegCreateKeyExW, RegGetValueW, RegSetValueExW, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    // windows-sys models LSTATUS as `u32`.
    const ERROR_SUCCESS: u32 = 0;
    const ERROR_FILE_NOT_FOUND: u32 = 2;
    const ERROR_PATH_NOT_FOUND: u32 = 3;
    const ERROR_NOT_FOUND: u32 = 1168;

    fn from_lstatus(status: u32) -> Status {
        match status {
            ERROR_SUCCESS => Status::Ok,
            ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND | ERROR_NOT_FOUND => Status::NotFound,
            _ => Status::Failed,
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn steam_app_key_path(app_id: u32) -> Vec<u16> {
        to_wide(&format!("Software\\Valve\\Steam\\Apps\\{}", app_id))
    }

    const ACTIVE_PROCESS_KEY_PATH: &str = "Software\\Valve\\Steam\\ActiveProcess";

    fn value_name(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    unsafe fn read_binary_value(
        app_id: u32,
        value_name_str: &str,
        out: &mut Vec<u8>,
    ) -> u32 {
        let key_path = steam_app_key_path(app_id);
        let vname = value_name(value_name_str);

        let mut bytes: u32 = 0;
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            vname.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut bytes,
        );
        if status != ERROR_SUCCESS {
            let mapped = from_lstatus(status);
            if mapped == Status::NotFound {
                crate::ostp_debug!(
                    "SteamCredentialStore: {} missing for appid={} status={}",
                    value_name_str, app_id, status
                );
            } else {
                crate::ostp_warn!(
                    "SteamCredentialStore: failed to query {} size for appid={} status={}",
                    value_name_str, app_id, status
                );
            }
            return status;
        }
        if bytes == 0 {
            crate::ostp_debug!(
                "SteamCredentialStore: {} empty for appid={}",
                value_name_str, app_id
            );
            out.clear();
            return ERROR_SUCCESS;
        }

        let mut buf = vec![0u8; bytes as usize];
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            vname.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut c_void,
            &mut bytes,
        );
        if status != ERROR_SUCCESS {
            crate::ostp_warn!(
                "SteamCredentialStore: failed to read {} for appid={} status={}",
                value_name_str, app_id, status
            );
            return status;
        }

        buf.truncate(bytes as usize);
        *out = buf;
        crate::ostp_debug!(
            "SteamCredentialStore: read {} for appid={} bytes={}",
            value_name_str, app_id, out.len()
        );
        ERROR_SUCCESS
    }

    unsafe fn write_value(
        app_id: u32,
        value_name_str: &str,
        ty: u32,
        data: &[u8],
    ) -> u32 {
        let key_path = steam_app_key_path(app_id);
        let vname = value_name(value_name_str);

        let mut key = UniqueRegKey::new();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            std::ptr::null(),
            key.put(),
            std::ptr::null_mut(),
        );
        if status != ERROR_SUCCESS {
            crate::ostp_warn!(
                "SteamCredentialStore: failed to create/open app key appid={} status={}",
                app_id, status
            );
            return status;
        }

        let status = RegSetValueExW(
            key.get(),
            vname.as_ptr(),
            0,
            ty,
            data.as_ptr(),
            data.len() as u32,
        );
        if status != ERROR_SUCCESS {
            crate::ostp_warn!(
                "SteamCredentialStore: failed to write {} for appid={} bytes={} status={}",
                value_name_str, app_id, data.len(), status
            );
            return status;
        }

        crate::ostp_info!(
            "SteamCredentialStore: wrote {} for appid={} bytes={}",
            value_name_str, app_id, data.len()
        );
        ERROR_SUCCESS
    }

    pub(super) fn get_app_ticket(app_id: u32) -> Result<Vec<u8>, Status> {
        let mut out = Vec::new();
        let st = unsafe { read_binary_value(app_id, "AppTicket", &mut out) };
        match from_lstatus(st) {
            Status::Ok => Ok(out),
            other => Err(other),
        }
    }

    pub(super) fn write_app_ticket(app_id: u32, data: &[u8]) -> Status {
        let st = unsafe { write_value(app_id, "AppTicket", RRF_RT_REG_BINARY, data) };
        from_lstatus(st)
    }

    pub(super) fn get_e_ticket(app_id: u32) -> Result<Vec<u8>, Status> {
        let mut out = Vec::new();
        let st = unsafe { read_binary_value(app_id, "ETicket", &mut out) };
        match from_lstatus(st) {
            Status::Ok => Ok(out),
            other => Err(other),
        }
    }

    pub(super) fn write_e_ticket(app_id: u32, data: &[u8]) -> Status {
        let st = unsafe { write_value(app_id, "ETicket", RRF_RT_REG_BINARY, data) };
        from_lstatus(st)
    }

    pub(super) fn get_steam_id(app_id: u32) -> Result<u64, Status> {
        let key_path = steam_app_key_path(app_id);
        let vname = value_name("SteamID");

        let mut bytes: u32 = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                vname.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            let mapped = from_lstatus(status);
            if mapped == Status::NotFound {
                crate::ostp_debug!(
                    "SteamCredentialStore: SteamID missing for appid={} status={}",
                    app_id, status
                );
            } else {
                crate::ostp_warn!(
                    "SteamCredentialStore: failed to query SteamID size for appid={} status={}",
                    app_id, status
                );
            }
            return Err(mapped);
        }
        if bytes < 2 {
            crate::ostp_warn!(
                "SteamCredentialStore: SteamID value too short for appid={} bytes={}",
                app_id, bytes
            );
            return Err(Status::NotFound);
        }

        let mut raw = vec![0u16; (bytes / 2) as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                vname.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                raw.as_mut_ptr() as *mut c_void,
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            crate::ostp_warn!(
                "SteamCredentialStore: failed to read SteamID for appid={} status={}",
                app_id, status
            );
            return Err(from_lstatus(status));
        }

        let chars = (bytes / 2) as usize;
        let trimmed = if chars > 0 { &raw[..chars - 1] } else { &raw[..0] };
        let s = String::from_utf16_lossy(trimmed);

        let parsed = s.parse::<u64>().ok().filter(|&v| v != 0);
        let parsed = match parsed {
            Some(v) => v,
            None => {
                crate::ostp_warn!(
                    "SteamCredentialStore: SteamID value invalid for appid={}",
                    app_id
                );
                return Err(Status::NotFound);
            }
        };

        crate::ostp_debug!(
            "SteamCredentialStore: read SteamID for appid={} steamid={}",
            app_id, parsed
        );
        Ok(parsed)
    }

    pub(super) fn write_steam_id(app_id: u32, steam_id: u64) -> Status {
        let s = steam_id.to_string();
        let mut bytes: Vec<u8> = Vec::with_capacity((s.len() + 1) * 2);
        for cu in s.encode_utf16().chain(std::iter::once(0)) {
            bytes.extend_from_slice(&cu.to_le_bytes());
        }
        let st = unsafe { write_value(app_id, "SteamID", RRF_RT_REG_SZ, &bytes) };
        from_lstatus(st)
    }

    pub(super) fn get_active_user() -> Result<(u32, String), Status> {
        let key_path = to_wide(ACTIVE_PROCESS_KEY_PATH);
        let vname_user = value_name("ActiveUser");

        let mut active_user: u32 = 0;
        let mut bytes: u32 = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                vname_user.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut active_user as *mut u32 as *mut c_void,
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            let mapped = from_lstatus(status);
            if mapped == Status::NotFound {
                crate::ostp_debug!(
                    "SteamCredentialStore: ActiveUser missing status={}",
                    status
                );
            } else {
                crate::ostp_warn!(
                    "SteamCredentialStore: failed to read ActiveUser status={}",
                    status
                );
            }
            return Err(mapped);
        }
        if bytes != std::mem::size_of::<u32>() as u32 {
            crate::ostp_warn!(
                "SteamCredentialStore: ActiveUser has unexpected size bytes={}",
                bytes
            );
            return Err(Status::NotFound);
        }
        if active_user == 0 {
            crate::ostp_debug!("SteamCredentialStore: ActiveUser is zero");
            return Err(Status::NotFound);
        }

        let vname_universe = value_name("Universe");
        bytes = 0;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                vname_universe.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            let mapped = from_lstatus(status);
            if mapped == Status::NotFound {
                crate::ostp_debug!(
                    "SteamCredentialStore: ActiveProcess Universe missing status={}",
                    status
                );
            } else {
                crate::ostp_warn!(
                    "SteamCredentialStore: failed to query ActiveProcess Universe size status={}",
                    status
                );
            }
            return Err(mapped);
        }
        if bytes < 2 {
            crate::ostp_warn!(
                "SteamCredentialStore: ActiveProcess Universe value too short bytes={}",
                bytes
            );
            return Err(Status::NotFound);
        }

        let mut raw = vec![0u16; (bytes / 2) as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_path.as_ptr(),
                vname_universe.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                raw.as_mut_ptr() as *mut c_void,
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            crate::ostp_warn!(
                "SteamCredentialStore: failed to read ActiveProcess Universe status={}",
                status
            );
            return Err(from_lstatus(status));
        }

        let chars = (bytes / 2) as usize;
        let trimmed = if chars > 0 { &raw[..chars - 1] } else { &raw[..0] };
        let universe = String::from_utf16_lossy(trimmed);
        if universe.is_empty() {
            crate::ostp_warn!("SteamCredentialStore: ActiveProcess Universe is empty");
            return Err(Status::NotFound);
        }

        crate::ostp_debug!(
            "SteamCredentialStore: read ActiveUser accountid={} universe={}",
            active_user, universe
        );
        Ok((active_user, universe))
    }
}

// ─── Non-Windows fallback ───────────────────────────────────────────

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn get_app_ticket(_app_id: u32) -> Result<Vec<u8>, Status> {
        Err(Status::Unsupported)
    }
    pub(super) fn write_app_ticket(_app_id: u32, _data: &[u8]) -> Status {
        Status::Unsupported
    }
    pub(super) fn get_e_ticket(_app_id: u32) -> Result<Vec<u8>, Status> {
        Err(Status::Unsupported)
    }
    pub(super) fn write_e_ticket(_app_id: u32, _data: &[u8]) -> Status {
        Status::Unsupported
    }
    pub(super) fn get_steam_id(_app_id: u32) -> Result<u64, Status> {
        Err(Status::Unsupported)
    }
    pub(super) fn write_steam_id(_app_id: u32, _steam_id: u64) -> Status {
        Status::Unsupported
    }
    pub(super) fn get_active_user() -> Result<(u32, String), Status> {
        Err(Status::Unsupported)
    }
}

// ─── Public API ─────────────────────────────────────────────────────

pub fn get_app_ticket(app_id: u32) -> Result<Vec<u8>, Status> {
    imp::get_app_ticket(app_id)
}
pub fn write_app_ticket(app_id: u32, data: &[u8]) -> Status {
    imp::write_app_ticket(app_id, data)
}
pub fn get_e_ticket(app_id: u32) -> Result<Vec<u8>, Status> {
    imp::get_e_ticket(app_id)
}
pub fn write_e_ticket(app_id: u32, data: &[u8]) -> Status {
    imp::write_e_ticket(app_id, data)
}
pub fn get_steam_id(app_id: u32) -> Result<u64, Status> {
    imp::get_steam_id(app_id)
}
pub fn write_steam_id(app_id: u32, steam_id: u64) -> Status {
    imp::write_steam_id(app_id, steam_id)
}
pub fn get_active_user() -> Result<(u32, String), Status> {
    imp::get_active_user()
}

#[cfg(not(windows))]
#[allow(dead_code)]
fn _touch(_: HKEY) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings() {
        assert_eq!(Status::Ok.as_str(), "Ok");
        assert_eq!(Status::NotFound.as_str(), "NotFound");
        assert_eq!(Status::Unsupported.as_str(), "Unsupported");
        assert_eq!(Status::Failed.as_str(), "Failed");
    }

    #[cfg(not(windows))]
    #[test]
    fn stubs_return_unsupported() {
        assert_eq!(get_app_ticket(730), Err(Status::Unsupported));
        assert_eq!(get_e_ticket(730), Err(Status::Unsupported));
        assert_eq!(get_steam_id(730), Err(Status::Unsupported));
        assert_eq!(get_active_user(), Err(Status::Unsupported));
        assert_eq!(write_app_ticket(730, &[0u8; 4]), Status::Unsupported);
        assert_eq!(write_e_ticket(730, &[0u8; 4]), Status::Unsupported);
        assert_eq!(write_steam_id(730, 0), Status::Unsupported);
    }
}
