//! extract_tickets - dump the AppTicket and ETicket hex strings for a
//! given Steam AppID.
//!
//! Standalone; runs in its own process, NOT injected. Reads the Steam
//! install path from `HKCU\Software\Valve\Steam\SteamPath` (32-bit
//! view), loads `steamclient64.dll` from that directory, opens a pipe
//! to the running Steam client, and asks it for the two tickets
//! associated with an AppID.
//!
//! Output goes into `<appid>/` relative to the current working
//! directory:
//!   * `appticket.bin` - raw ownership ticket bytes (if present)
//!   * `eticket.bin`   - raw encrypted app ticket bytes (if present)
//!   * `tickets.txt`   - plain-text hex strings for setAppTicket /
//!                       setETicket
//!
//! ## C++ ABI
//!
//! Steamworks is a C++ API: each interface is a vtable of function
//! pointers, laid out in declaration order. Rust models this as a
//! `#[repr(C)]` struct whose only field is `*const VTable`, plus a
//! `#[repr(C)]` vtable struct of `extern "system"` fn pointers. On x64
//! Windows every calling convention collapses to the platform ABI, so
//! `extern "system"` is correct for the C++ `__cdecl` Steamworks uses
//! on x86.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

#[cfg(not(windows))]
fn main() -> std::process::ExitCode {
    eprintln!("extract_tickets must be built for Windows x64.");
    std::process::ExitCode::from(1)
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    win::run()
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io::{IsTerminal, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::System::Environment::SetEnvironmentVariableA;
    use windows_sys::Win32::Foundation::FreeLibrary;
    use windows_sys::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH,
    };
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    // `KEY_WOW64_32KEY` is not exported by windows-sys 0.59. Value from
    // `winnt.h`. Steam is a 32-bit client whose registry entries live in
    // the WoW64 view on 64-bit Windows.
    const KEY_WOW64_32KEY: u32 = 0x0200;

    const STEAM_CLIENT_VERSION:     &[u8] = b"SteamClient023\0";
    const STEAM_USER_VERSION:       &[u8] = b"SteamUser023\0";
    const STEAM_UTILS_VERSION:      &[u8] = b"SteamUtils010\0";
    const STEAM_APP_TICKET_VERSION: &[u8] = b"STEAMAPPTICKET_INTERFACE_VERSION001\0";

    type HSteamPipe = i32;
    type HSteamUser = i32;
    type SteamAPICall_t = u64;
    type AppId_t = u32;
    type Bool = u8;

    const K_ERESULT_OK: i32 = 1;
    const K_ENCRYPTED_APP_TICKET_CALLBACK: i32 = 100 + 54;

    const POLL_MAX_MS: u64 = 15_000;
    const POLL_STEP_MS: u64 = 50;

    // ── Vtables ─────────────────────────────────────────────────────

    #[repr(C)]
    pub struct ISteamClient { vtable: *const ISteamClientVtable }

    #[repr(C)]
    struct ISteamClientVtable {
        create_steam_pipe: unsafe extern "system" fn(*mut ISteamClient) -> HSteamPipe,
        b_release_steam_pipe: unsafe extern "system" fn(*mut ISteamClient, HSteamPipe) -> Bool,
        connect_to_global_user: unsafe extern "system" fn(*mut ISteamClient, HSteamPipe) -> HSteamUser,
        create_local_user: unsafe extern "system" fn(*mut ISteamClient, *mut HSteamPipe, i32) -> HSteamUser,
        release_user: unsafe extern "system" fn(*mut ISteamClient, HSteamPipe, HSteamUser),
        get_isteam_user: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut ISteamUser,
        get_isteam_game_server: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut c_void,
        set_local_ip_binding: unsafe extern "system" fn(*mut ISteamClient, *const c_void, u16),
        get_isteam_friends: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut c_void,
        get_isteam_utils: unsafe extern "system" fn(*mut ISteamClient, HSteamPipe, *const i8) -> *mut ISteamUtils,
        get_isteam_matchmaking: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut c_void,
        get_isteam_matchmaking_servers: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut c_void,
        get_isteam_generic_interface: unsafe extern "system" fn(*mut ISteamClient, HSteamUser, HSteamPipe, *const i8) -> *mut c_void,
    }

    #[repr(C)]
    pub struct ISteamUtils { vtable: *const ISteamUtilsVtable }

    #[repr(C)]
    struct ISteamUtilsVtable {
        get_seconds_since_app_active: unsafe extern "system" fn(*mut ISteamUtils) -> u32,
        get_seconds_since_computer_active: unsafe extern "system" fn(*mut ISteamUtils) -> u32,
        get_connected_universe: unsafe extern "system" fn(*mut ISteamUtils) -> i32,
        get_server_real_time: unsafe extern "system" fn(*mut ISteamUtils) -> u32,
        get_ip_country: unsafe extern "system" fn(*mut ISteamUtils) -> *const i8,
        get_image_size: unsafe extern "system" fn(*mut ISteamUtils, i32, *mut u32, *mut u32) -> Bool,
        get_image_rgba: unsafe extern "system" fn(*mut ISteamUtils, i32, *mut u8, i32) -> Bool,
        get_cser_ip_port: unsafe extern "system" fn(*mut ISteamUtils, *mut u32, *mut u16) -> Bool,
        get_current_battery_power: unsafe extern "system" fn(*mut ISteamUtils) -> u8,
        get_app_id: unsafe extern "system" fn(*mut ISteamUtils) -> AppId_t,
        set_overlay_notification_position: unsafe extern "system" fn(*mut ISteamUtils, i32),
        is_api_call_completed: unsafe extern "system" fn(*mut ISteamUtils, SteamAPICall_t, *mut Bool) -> Bool,
        get_api_call_failure_reason: unsafe extern "system" fn(*mut ISteamUtils, SteamAPICall_t) -> i32,
        get_api_call_result: unsafe extern "system" fn(*mut ISteamUtils, SteamAPICall_t, *mut c_void, i32, i32, *mut Bool) -> Bool,
    }

    #[repr(C)]
    pub struct ISteamUser { vtable: *const ISteamUserVtable }

    #[repr(C)]
    struct ISteamUserVtable {
        get_h_steam_user: unsafe extern "system" fn(*mut ISteamUser) -> HSteamUser,
        b_logged_on: unsafe extern "system" fn(*mut ISteamUser) -> Bool,
        get_steam_id: unsafe extern "system" fn(*mut ISteamUser) -> u64,
        initiate_game_connection: unsafe extern "system" fn(*mut ISteamUser, *mut c_void, i32, u64, u32, u16, Bool) -> i32,
        terminate_game_connection: unsafe extern "system" fn(*mut ISteamUser, u32, u16),
        track_app_usage_event: unsafe extern "system" fn(*mut ISteamUser, u64, i32, *const i8),
        get_user_data_folder: unsafe extern "system" fn(*mut ISteamUser, *mut i8, i32) -> Bool,
        start_voice_recording: unsafe extern "system" fn(*mut ISteamUser),
        stop_voice_recording: unsafe extern "system" fn(*mut ISteamUser),
        get_available_voice: unsafe extern "system" fn(*mut ISteamUser, *mut u32, *mut u32, u32) -> i32,
        get_voice: unsafe extern "system" fn(*mut ISteamUser, Bool, *mut c_void, u32, *mut u32, Bool, *mut c_void, u32, *mut u32, u32) -> i32,
        decompress_voice: unsafe extern "system" fn(*mut ISteamUser, *const c_void, u32, *mut c_void, u32, *mut u32, u32) -> i32,
        get_voice_optimal_sample_rate: unsafe extern "system" fn(*mut ISteamUser) -> u32,
        get_auth_session_ticket: unsafe extern "system" fn(*mut ISteamUser, *mut c_void, i32, *mut u32, *const c_void) -> u32,
        get_auth_ticket_for_web_api: unsafe extern "system" fn(*mut ISteamUser, *const i8) -> u32,
        begin_auth_session: unsafe extern "system" fn(*mut ISteamUser, *const c_void, i32, u64) -> i32,
        end_auth_session: unsafe extern "system" fn(*mut ISteamUser, u64),
        cancel_auth_ticket: unsafe extern "system" fn(*mut ISteamUser, u32),
        user_has_license_for_app: unsafe extern "system" fn(*mut ISteamUser, u64, AppId_t) -> i32,
        b_is_behind_nat: unsafe extern "system" fn(*mut ISteamUser) -> Bool,
        advertise_game: unsafe extern "system" fn(*mut ISteamUser, u64, u32, u16),
        request_encrypted_app_ticket: unsafe extern "system" fn(*mut ISteamUser, *mut c_void, i32) -> SteamAPICall_t,
        get_encrypted_app_ticket: unsafe extern "system" fn(*mut ISteamUser, *mut c_void, i32, *mut u32) -> Bool,
    }

    #[repr(C)]
    pub struct ISteamAppTicket { vtable: *const ISteamAppTicketVtable }

    #[repr(C)]
    struct ISteamAppTicketVtable {
        get_app_ownership_ticket_data: unsafe extern "system" fn(
            *mut ISteamAppTicket, AppId_t, *mut c_void, u32,
            *mut u32, *mut u32, *mut u32, *mut u32,
        ) -> u32,
    }

    // ── Vtable call helpers ─────────────────────────────────────────

    unsafe fn client_create_pipe(c: *mut ISteamClient) -> HSteamPipe {
        ((*(*c).vtable).create_steam_pipe)(c)
    }
    unsafe fn client_release_pipe(c: *mut ISteamClient, p: HSteamPipe) {
        ((*(*c).vtable).b_release_steam_pipe)(c, p);
    }
    unsafe fn client_connect_global(c: *mut ISteamClient, p: HSteamPipe) -> HSteamUser {
        ((*(*c).vtable).connect_to_global_user)(c, p)
    }
    unsafe fn client_get_user(c: *mut ISteamClient, u: HSteamUser, p: HSteamPipe, v: *const i8) -> *mut ISteamUser {
        ((*(*c).vtable).get_isteam_user)(c, u, p, v)
    }
    unsafe fn client_get_utils(c: *mut ISteamClient, p: HSteamPipe, v: *const i8) -> *mut ISteamUtils {
        ((*(*c).vtable).get_isteam_utils)(c, p, v)
    }
    unsafe fn client_get_generic(c: *mut ISteamClient, u: HSteamUser, p: HSteamPipe, v: *const i8) -> *mut c_void {
        ((*(*c).vtable).get_isteam_generic_interface)(c, u, p, v)
    }
    unsafe fn utils_universe(u: *mut ISteamUtils) -> i32 {
        ((*(*u).vtable).get_connected_universe)(u)
    }
    unsafe fn utils_app_id(u: *mut ISteamUtils) -> AppId_t {
        ((*(*u).vtable).get_app_id)(u)
    }
    unsafe fn utils_is_api_call_completed(u: *mut ISteamUtils, call: SteamAPICall_t, failed: *mut Bool) -> Bool {
        ((*(*u).vtable).is_api_call_completed)(u, call, failed)
    }
    unsafe fn utils_get_api_call_result(
        u: *mut ISteamUtils, call: SteamAPICall_t, cb: *mut c_void, cb_len: i32, cb_id: i32, failed: *mut Bool,
    ) -> Bool {
        ((*(*u).vtable).get_api_call_result)(u, call, cb, cb_len, cb_id, failed)
    }
    unsafe fn user_request_encrypted_app_ticket(u: *mut ISteamUser, data: *mut c_void, cb: i32) -> SteamAPICall_t {
        ((*(*u).vtable).request_encrypted_app_ticket)(u, data, cb)
    }
    unsafe fn user_get_encrypted_app_ticket(u: *mut ISteamUser, buf: *mut c_void, cap: i32, out: *mut u32) -> Bool {
        ((*(*u).vtable).get_encrypted_app_ticket)(u, buf, cap, out)
    }
    unsafe fn appticket_get_data(
        a: *mut ISteamAppTicket, app_id: AppId_t, buf: *mut c_void, cap: u32,
        app_id_off: *mut u32, steam_id_off: *mut u32, sig_off: *mut u32, sig_size: *mut u32,
    ) -> u32 {
        ((*(*a).vtable).get_app_ownership_ticket_data)(
            a, app_id, buf, cap, app_id_off, steam_id_off, sig_off, sig_size,
        )
    }

    // ── Registry helper ─────────────────────────────────────────────

    fn read_hkcu_sz_32(sub_key: &str, value_name: &str) -> Option<String> {
        let sub: Vec<u16> = sub_key.encode_utf16().chain(std::iter::once(0)).collect();
        let vn: Vec<u16> = value_name.encode_utf16().chain(std::iter::once(0)).collect();

        let mut hkey: HKEY = std::ptr::null_mut();
        let sam: u32 = KEY_READ | KEY_WOW64_32KEY;
        let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, sam, &mut hkey) };
        if status != 0 { return None; }

        unsafe {
            let mut value_type: u32 = 0;
            let mut value_size: u32 = 0;
            let s1 = RegQueryValueExW(
                hkey, vn.as_ptr(), std::ptr::null(),
                &mut value_type, std::ptr::null_mut(), &mut value_size,
            );
            if s1 != 0 || value_type != REG_SZ || value_size < 2 {
                RegCloseKey(hkey);
                return None;
            }
            let mut buf = vec![0u8; value_size as usize];
            let mut fetched = value_size;
            let s2 = RegQueryValueExW(
                hkey, vn.as_ptr(), std::ptr::null(),
                std::ptr::null_mut(), buf.as_mut_ptr(), &mut fetched,
            );
            RegCloseKey(hkey);
            if s2 != 0 { return None; }
            buf.truncate(fetched as usize);
            if buf.len() >= 2 && buf[buf.len() - 2] == 0 && buf[buf.len() - 1] == 0 {
                buf.truncate(buf.len() - 2);
            }
            let units: Vec<u16> = buf.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units).ok()
        }
    }

    // ── Load + setup ────────────────────────────────────────────────

    fn load_steam_client() -> Option<(*mut c_void, PathBuf)> {
        let steam_path = read_hkcu_sz_32("Software\\Valve\\Steam", "SteamPath");
        let Some(steam_path) = steam_path else {
            eprintln!("Failed to find Steam install path in registry.");
            return None;
        };
        println!("Found SteamPath in HKEY_CURRENT_USER: {steam_path}");

        let dll_path = Path::new(&steam_path).join("steamclient64.dll");
        let wide: Vec<u16> = dll_path.as_os_str().encode_wide()
            .chain(std::iter::once(0)).collect();

        let module = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH)
        };
        if module.is_null() {
            let e = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            eprintln!("Failed to load {} (GetLastError={e}).", dll_path.display());
            return None;
        }
        Some((module as *mut c_void, dll_path))
    }

    unsafe fn create_steam_client(module: *mut c_void) -> Option<*mut ISteamClient> {
        let name = b"CreateInterface\0";
        let raw = GetProcAddress(module as _, name.as_ptr());
        let Some(f) = raw else {
            eprintln!("steamclient64.dll has no CreateInterface export.");
            return None;
        };
        let create_interface: unsafe extern "system" fn(*const i8, *mut i32) -> *mut c_void =
            std::mem::transmute(f);

        let mut return_code: i32 = 0;
        let client = create_interface(STEAM_CLIENT_VERSION.as_ptr() as *const i8, &mut return_code);
        if client.is_null() {
            eprintln!("CreateInterface(SteamClient023) failed (returnCode={return_code}).");
            return None;
        }
        Some(client as *mut ISteamClient)
    }

    // ── Ticket extraction ───────────────────────────────────────────

    unsafe fn extract_app_ownership_ticket(
        client: *mut ISteamClient, pipe: HSteamPipe, user: HSteamUser, app_id: u32,
    ) -> Option<Vec<u8>> {
        let app_ticket = client_get_generic(
            client, user, pipe, STEAM_APP_TICKET_VERSION.as_ptr() as *const i8,
        ) as *mut ISteamAppTicket;
        if app_ticket.is_null() {
            eprintln!("GetISteamGenericInterface(STEAMAPPTICKET_INTERFACE_VERSION001) returned null.");
            return None;
        }

        let mut buf = vec![0u8; 2048];
        let mut app_id_off: u32 = 0;
        let mut steam_id_off: u32 = 0;
        let mut sig_off: u32 = 0;
        let mut sig_size: u32 = 0;
        let written = appticket_get_data(
            app_ticket, app_id,
            buf.as_mut_ptr() as *mut c_void, buf.len() as u32,
            &mut app_id_off, &mut steam_id_off, &mut sig_off, &mut sig_size,
        );
        if written == 0 || written as usize > buf.len() {
            eprintln!(
                "GetAppOwnershipTicketData returned no ticket for AppID {app_id} \
                 (own the app and have it cached locally?)."
            );
            return None;
        }
        buf.truncate(written as usize);
        println!(
            "Ownership ticket {written} bytes (appIdOffset={app_id_off} \
             steamIdOffset={steam_id_off} signatureOffset={sig_off} signatureSize={sig_size})"
        );
        Some(buf)
    }

    unsafe fn extract_encrypted_app_ticket(
        client: *mut ISteamClient, pipe: HSteamPipe, user: HSteamUser, app_id: u32,
    ) -> Option<Vec<u8>> {
        let utils = client_get_utils(client, pipe, STEAM_UTILS_VERSION.as_ptr() as *const i8);
        let steam_user = client_get_user(client, user, pipe, STEAM_USER_VERSION.as_ptr() as *const i8);
        if utils.is_null() || steam_user.is_null() {
            eprintln!("GetISteamUtils/GetISteamUser returned null.");
            return None;
        }

        let call = user_request_encrypted_app_ticket(steam_user, std::ptr::null_mut(), 0);
        if call == 0 {
            eprintln!("RequestEncryptedAppTicket failed to start for AppID {app_id}.");
            return None;
        }

        let start = Instant::now();
        let mut failed: Bool = 0;
        loop {
            if utils_is_api_call_completed(utils, call, &mut failed) != 0 { break; }
            if start.elapsed() >= Duration::from_millis(POLL_MAX_MS) {
                eprintln!("Timed out waiting for EncryptedAppTicketResponse_t.");
                return None;
            }
            std::thread::sleep(Duration::from_millis(POLL_STEP_MS));
        }

        let mut eresult: i32 = 0;
        failed = 0;
        let got = utils_get_api_call_result(
            utils, call,
            &mut eresult as *mut i32 as *mut c_void,
            std::mem::size_of::<i32>() as i32,
            K_ENCRYPTED_APP_TICKET_CALLBACK,
            &mut failed,
        );
        if got == 0 || failed != 0 {
            eprintln!("GetAPICallResult failed for EncryptedAppTicketResponse_t.");
            return None;
        }
        if eresult != K_ERESULT_OK {
            eprintln!("RequestEncryptedAppTicket returned EResult {eresult}.");
            return None;
        }

        let mut cb_ticket: u32 = 0;
        user_get_encrypted_app_ticket(steam_user, std::ptr::null_mut(), 0, &mut cb_ticket);
        if cb_ticket == 0 {
            eprintln!("Encrypted app ticket is empty.");
            return None;
        }
        let mut buf = vec![0u8; cb_ticket as usize];
        if user_get_encrypted_app_ticket(
            steam_user, buf.as_mut_ptr() as *mut c_void,
            buf.len() as i32, &mut cb_ticket,
        ) == 0 {
            eprintln!("GetEncryptedAppTicket failed.");
            return None;
        }
        buf.truncate(cb_ticket as usize);
        println!("Encrypted ticket {cb_ticket} bytes");
        Some(buf)
    }

    // ── Output ──────────────────────────────────────────────────────

    fn to_hex_lower(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
        out
    }

    fn print_hex(label: &str, data: &[u8]) {
        println!("{label} ({} bytes):", data.len());
        const BYTES_PER_ROW: usize = 16;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut row = 0;
        while row < data.len() {
            let mut line = String::new();
            for shift in (0..=12).step_by(4) {
                let nibble = ((row as u32) >> (12 - shift)) & 0xF;
                line.push(HEX[nibble as usize] as char);
            }
            line.push_str("  ");
            let mut ascii = String::new();
            for col in 0..BYTES_PER_ROW {
                if row + col < data.len() {
                    let byte = data[row + col];
                    line.push(HEX[(byte >> 4) as usize] as char);
                    line.push(HEX[(byte & 0x0F) as usize] as char);
                    line.push(' ');
                    ascii.push(if (0x20..0x7F).contains(&byte) { byte as char } else { '.' });
                } else {
                    line.push_str("   ");
                }
                if col == 7 { line.push(' '); }
            }
            println!("{line} {ascii}");
            row += BYTES_PER_ROW;
        }
    }

    fn ticket_line(name: &str, ticket: Option<&[u8]>) -> String {
        match ticket {
            None => format!("{name}:null\n"),
            Some(bytes) => format!("{name}({}bytes):{}\n", bytes.len(), to_hex_lower(bytes)),
        }
    }

    fn write_outputs(app_id: u32, ownership: Option<&[u8]>, encrypted: Option<&[u8]>) -> bool {
        let dir = PathBuf::from(app_id.to_string());
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("Failed to create directory {} ({e}).", dir.display());
            return false;
        }
        let mut ok = true;
        if let Some(b) = ownership {
            let p = dir.join("appticket.bin");
            if let Err(e) = std::fs::write(&p, b) {
                eprintln!("Failed to write {} ({e}).", p.display());
                ok = false;
            }
        }
        if let Some(b) = encrypted {
            let p = dir.join("eticket.bin");
            if let Err(e) = std::fs::write(&p, b) {
                eprintln!("Failed to write {} ({e}).", p.display());
                ok = false;
            }
        }
        let text = format!(
            "appid:{app_id}\n{}{}",
            ticket_line("appticket", ownership),
            ticket_line("eticket", encrypted),
        );
        let summary = dir.join("tickets.txt");
        match std::fs::write(&summary, text) {
            Ok(()) => println!("Wrote {}\\", dir.display()),
            Err(e) => {
                eprintln!("Failed to write {} ({e}).", summary.display());
                ok = false;
            }
        }
        ok
    }

    // ── AppID parsing ───────────────────────────────────────────────

    fn parse_app_id(s: &str) -> Option<u32> {
        if s.is_empty() { return None; }
        if !s.bytes().all(|b| b.is_ascii_digit()) { return None; }
        s.parse::<u32>().ok()
    }

    // ── Entry point ─────────────────────────────────────────────────

    pub fn run() -> ExitCode {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let app_id = if let Some(a) = args.first() {
            match parse_app_id(a) {
                Some(id) => id,
                None => { eprintln!("Invalid AppID: {a}"); return ExitCode::from(1); }
            }
        } else {
            print!("AppID: ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            if std::io::stdin().read_line(&mut line).is_err() {
                eprintln!("Invalid AppID.");
                return ExitCode::from(1);
            }
            match parse_app_id(line.trim()) {
                Some(id) => id,
                None => { eprintln!("Invalid AppID."); return ExitCode::from(1); }
            }
        };

        let id_c = format!("{app_id}\0");
        unsafe {
            SetEnvironmentVariableA(b"SteamAppId\0".as_ptr(), id_c.as_ptr());
            SetEnvironmentVariableA(b"SteamGameId\0".as_ptr(), id_c.as_ptr());
        }

        let Some((module, dll_path)) = load_steam_client() else {
            return pause_and(ExitCode::from(1));
        };
        println!("Loaded {}", dll_path.display());

        let Some(client) = (unsafe { create_steam_client(module) }) else {
            unsafe { FreeLibrary(module as _); }
            return pause_and(ExitCode::from(1));
        };

        let pipe = unsafe { client_create_pipe(client) };
        if pipe == 0 {
            eprintln!("CreateSteamPipe failed. Is Steam running?");
            unsafe { FreeLibrary(module as _); }
            return pause_and(ExitCode::from(1));
        }
        let user = unsafe { client_connect_global(client, pipe) };
        if user == 0 {
            eprintln!("ConnectToGlobalUser failed. Is a user logged in?");
            unsafe {
                client_release_pipe(client, pipe);
                FreeLibrary(module as _);
            }
            return pause_and(ExitCode::from(1));
        }

        let utils = unsafe { client_get_utils(client, pipe, STEAM_UTILS_VERSION.as_ptr() as *const i8) };
        if !utils.is_null() {
            let universe = unsafe { utils_universe(utils) };
            let client_app_id = unsafe { utils_app_id(utils) };
            println!("ConnectedUniverse={universe} ClientAppID={client_app_id}");
        }

        let ownership = unsafe { extract_app_ownership_ticket(client, pipe, user, app_id) };
        if let Some(b) = &ownership { print_hex("Ownership ticket", b); }

        let encrypted = unsafe { extract_encrypted_app_ticket(client, pipe, user, app_id) };
        if let Some(b) = &encrypted { print_hex("Encrypted ticket", b); }

        let ok = write_outputs(app_id, ownership.as_deref(), encrypted.as_deref());

        unsafe {
            client_release_pipe(client, pipe);
            FreeLibrary(module as _);
        }

        pause_and(if ok { ExitCode::SUCCESS } else { ExitCode::from(1) })
    }

    fn pause_and(code: ExitCode) -> ExitCode {
        if std::io::stdin().is_terminal() {
            print!("\nPress Enter to exit...");
            let _ = std::io::stdout().flush();
            let mut s = String::new();
            let _ = std::io::stdin().read_line(&mut s);
        }
        code
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test] fn parse_app_id_accepts_decimal() {
            assert_eq!(parse_app_id("730"), Some(730));
            assert_eq!(parse_app_id("0"), Some(0));
            assert_eq!(parse_app_id("4294967295"), Some(u32::MAX));
        }

        #[test] fn parse_app_id_rejects_invalid() {
            assert_eq!(parse_app_id(""), None);
            assert_eq!(parse_app_id("-1"), None);
            assert_eq!(parse_app_id("+1"), None);
            assert_eq!(parse_app_id("abc"), None);
            assert_eq!(parse_app_id("1.5"), None);
            assert_eq!(parse_app_id("4294967296"), None);
        }

        #[test] fn to_hex_lower_round_trips() {
            assert_eq!(to_hex_lower(&[0x00, 0xFF, 0x10]), "00ff10");
            assert_eq!(to_hex_lower(&[]), "");
        }

        #[test] fn ticket_line_null_when_absent() {
            assert_eq!(ticket_line("appticket", None), "appticket:null\n");
        }

        #[test] fn ticket_line_hex_when_present() {
            assert_eq!(
                ticket_line("eticket", Some(&[0xDE, 0xAD])),
                "eticket(2bytes):dead\n"
            );
        }
    }
}
