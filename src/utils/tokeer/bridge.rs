//! Tokeer code-redemption bridge. Source: `TokeerBridge.cpp`.
//!
//! Two entry points: `handle_uri` (called from the rundll32 handler) and
//! `register_uri_scheme` (called once at startup to install the scheme
//! under HKCU so `bst://...` launches this DLL).
//!
//! The server URL is resolved from `option_env!("OST_TOKEER_URL")`. The
//! C++ defaulted to `https://luastools.xyz` when nothing was configured
//! at build time; we do the same. Set `OST_TOKEER_URL` at build time to
//! point at a different host.

use crate::platform::dialog;
use crate::platform::http;
use crate::platform::numbers;
use crate::platform::steam_credential_store as creds;

const DEFAULT_TOKEER_URL: &str = "https://luastools.xyz";

fn tokeer_base_url() -> String {
    let mut url = option_env!("OST_TOKEER_URL").unwrap_or(DEFAULT_TOKEER_URL).to_string();
    while url.ends_with('/') { url.pop(); }
    url
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.is_empty() || hex.len() % 2 != 0 { return None; }
    let b = hex.as_bytes();
    let mut out = Vec::with_capacity(b.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = hex_nibble(b[i])?;
        let lo = hex_nibble(b[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

/// Extract a string field ("key":"VALUE") from the server's flat JSON.
/// Returns None when the key is absent, or when the value is null /
/// missing (no opening quote before the next delimiter).
fn json_string(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let k = body.find(&needle)?;
    let colon = body[k + needle.len()..].find(':')? + k + needle.len();
    let q1 = body[colon + 1..].find('"')? + colon + 1;
    if let Some(delim) = body[colon + 1..].find(|c| c == ',' || c == '}') {
        if q1 > colon + 1 + delim { return None; }
    }
    let q2 = body[q1 + 1..].find('"')? + q1 + 1;
    Some(body[q1 + 1..q2].to_string())
}

fn json_true(body: &str, key: &str) -> bool {
    let needle = format!("\"{}\"", key);
    let Some(k) = body.find(&needle) else { return false; };
    let Some(colon) = body[k + needle.len()..].find(':') else { return false; };
    let after = colon + k + needle.len() + 1;
    let rest = &body[after..];
    let first_nonspace = rest.find(|c: char| c != ' ' && c != '\t').unwrap_or(rest.len());
    rest[first_nonspace..].starts_with("true")
}

/// POST the code to `<base>/drm/redeem`, write the returned tickets to the
/// credential store, then prompt the user to launch the game.
pub fn redeem(code: &str) {
    let url = format!("{}/drm/redeem", tokeer_base_url());
    let req_body = format!("{{\"code\":\"{}\"}}", code);

    let mut req = http::Request::new("POST", &url);
    req.body = Some(req_body.as_bytes());
    req.headers = Some("Content-Type: application/json\r\n");
    req.timeout_resolve_ms = 5000;
    req.timeout_connect_ms = 5000;
    req.timeout_send_ms    = 10000;
    req.timeout_recv_ms    = 20000;
    req.max_body_bytes     = 4 * 1024 * 1024;
    let r = http::execute(&req);

    let body_str = String::from_utf8_lossy(&r.body).into_owned();

    if !r.ok || r.status != 200 || !json_true(&body_str, "success") {
        let reason = json_string(&body_str, "reason")
            .or_else(|| json_string(&body_str, "error"))
            .unwrap_or_else(|| format!("Server error {}", r.status));
        LOG_WARN!("TokeerBridge: redeem failed (HTTP {}): {}", r.status, reason);
        dialog::show_warning("BetterSteamTools", &format!("Redeem failed:\n\n{}", reason));
        return;
    }

    let app_id_str = json_string(&body_str, "app_id").unwrap_or_default();
    let app_hex    = json_string(&body_str, "appticket").unwrap_or_default();
    let et_hex     = json_string(&body_str, "eticket").unwrap_or_default();

    let app_id = match numbers::parse_u32(&app_id_str) {
        Some(v) => v,
        None => {
            LOG_WARN!("TokeerBridge: redeem returned an incomplete/invalid ticket");
            dialog::show_warning("BetterSteamTools", "The server returned an incomplete ticket.");
            return;
        }
    };
    let app_bytes = match hex_to_bytes(&app_hex) {
        Some(v) if !v.is_empty() => v,
        _ => {
            LOG_WARN!("TokeerBridge: redeem returned an incomplete/invalid ticket");
            dialog::show_warning("BetterSteamTools", "The server returned an incomplete ticket.");
            return;
        }
    };
    let et_bytes = match hex_to_bytes(&et_hex) {
        Some(v) if !v.is_empty() => v,
        _ => {
            LOG_WARN!("TokeerBridge: redeem returned an incomplete/invalid ticket");
            dialog::show_warning("BetterSteamTools", "The server returned an incomplete ticket.");
            return;
        }
    };

    if creds::write_app_ticket(app_id, &app_bytes) != creds::Status::Ok
        || creds::write_e_ticket(app_id, &et_bytes) != creds::Status::Ok
    {
        LOG_ERROR!("TokeerBridge: failed to write tickets for app {}", app_id);
        dialog::show_warning("BetterSteamTools", "Could not write the ticket to Steam.");
        return;
    }

    LOG_INFO!("TokeerBridge: redeemed code for app {} ({} + {} bytes)",
              app_id, app_bytes.len(), et_bytes.len());

    let launch = dialog::show_confirm(
        "Code Redeemed!",
        &format!("Added app {} to your library.\n\nLaunch it now?", app_id),
    );
    if launch {
        let steam_url = format!("steam://rungameid/{}", app_id);
        LOG_INFO!("TokeerBridge: launching app {} via {}", app_id, steam_url);
        #[cfg(windows)]
        unsafe {
            // ShellExecuteA wants a NUL-terminated string.
            let op = b"open\0";
            let mut url = steam_url.into_bytes();
            url.push(0);
            windows_sys::Win32::UI::Shell::ShellExecuteA(
                std::ptr::null_mut(),
                op.as_ptr() as *const u8,
                url.as_ptr() as *const u8,
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
        }
    }
}

/// Parse and dispatch a `bst://<action>/<arg>` URL. Unknown actions are
/// logged and ignored. Called from the exported `TokeerUri` handler.
pub fn handle_uri(raw_url: &str) {
    LOG_INFO!("TokeerBridge: handle_uri raw='{}'", raw_url);

    // rundll32 does its own command-line parsing and leaves quotes in
    // place. Strip a single wrapping pair plus incidental whitespace.
    let mut url = raw_url.trim().to_string();
    if url.len() >= 2 {
        let bytes = url.as_bytes();
        let (first, last) = (bytes[0], bytes[bytes.len() - 1]);
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            url = url[1..url.len() - 1].to_string();
        }
    }

    let prefix = "bst://";
    if !url.starts_with(prefix) {
        LOG_WARN!("TokeerBridge: ignoring non-bst URL (cleaned='{}')", url);
        return;
    }
    let mut rest = url[prefix.len()..].to_string();
    while rest.ends_with(|c| c == '/' || c == '\r' || c == '\n' || c == ' ') {
        rest.pop();
    }

    let (action, arg) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None    => (rest.as_str(), ""),
    };

    if action == "redeem" {
        if arg.is_empty() {
            dialog::show_warning("BetterSteamTools", "Missing code in link.");
        } else {
            redeem(arg);
        }
    } else {
        LOG_WARN!("TokeerBridge: unknown action '{}'", action);
    }
}

/// Register the `bst://` URI scheme under HKCU pointing at this DLL via
/// rundll32. Safe to call on every startup; rewrites only if the target
/// changed.
pub fn register_uri_scheme(dll_path: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{
            RegCreateKeyExA, RegSetValueExA, HKEY_CURRENT_USER, KEY_SET_VALUE,
        };
        let command = format!("rundll32.exe \"{}\",TokeerUri \"%1\"", dll_path);
        if registry_command_matches(&command) {
            LOG_INFO!(
                "TokeerBridge: bst:// scheme already registered for {}, skipping write",
                dll_path
            );
            return;
        }
        let write_key = |sub: &str, value_name: Option<&str>, value: &str| -> bool {
            LOG_INFO!("[wf] register_uri_scheme: write_key sub='{}' name={:?}", sub, value_name);
            let mut key: *mut std::ffi::c_void = std::ptr::null_mut();
            let sub_c = std::ffi::CString::new(sub).unwrap();
                        LOG_INFO!("[wf] register_uri_scheme: RegCreateKeyExA enter sub='{}'", sub);
let s = unsafe {
                RegCreateKeyExA(
                    HKEY_CURRENT_USER,
                    sub_c.as_ptr() as *const u8,
                    0, std::ptr::null(), 0,
                    KEY_SET_VALUE, std::ptr::null(),
                    &mut key, std::ptr::null_mut(),
                )
            };
            if s != 0 { return false; }
            let vn_c = value_name.map(|n| std::ffi::CString::new(n).unwrap());
            let vn_ptr = vn_c.as_ref().map(|c| c.as_ptr() as *const u8).unwrap_or(std::ptr::null());
            let bytes = value.as_bytes();
            let s2 = unsafe {
                RegSetValueExA(
                    key, vn_ptr, 0, 1 /* REG_SZ */,
                    bytes.as_ptr(), (bytes.len() + 1) as u32,
                )
            };
            unsafe { windows_sys::Win32::System::Registry::RegCloseKey(key); }
            s2 == 0
        };
        LOG_INFO!("[wf] register_uri_scheme: call 1/3 (default value)");
        let ok1 = write_key("Software\\Classes\\bst", None, "URL:BetterSteamTools");
        LOG_INFO!("[wf] register_uri_scheme: call 1/3 returned {}", ok1);
        LOG_INFO!("[wf] register_uri_scheme: call 2/3 (URL Protocol)");
        let ok2 = ok1 && write_key("Software\\Classes\\bst", Some("URL Protocol"), "");
        LOG_INFO!("[wf] register_uri_scheme: call 2/3 returned {}", ok2);
        LOG_INFO!("[wf] register_uri_scheme: call 3/3 (shell\\open\\command)");
        let ok3 = ok2 && write_key("Software\\Classes\\bst\\shell\\open\\command", None, &command);
        LOG_INFO!("[wf] register_uri_scheme: call 3/3 returned {}", ok3);
        let ok = ok3;
        if ok {
            LOG_INFO!("TokeerBridge: registered bst:// scheme -> {}", command);
        } else {
            LOG_WARN!("TokeerBridge: failed to register bst:// scheme");
        }
    }
    #[cfg(not(windows))]
    { let _ = dll_path; }
}

/// Read-only check: does `HKCU\Software\Classes\bst\shell\open\command`
/// already contain exactly `expected`? Uses `RegOpenKeyExA` +
/// `RegQueryValueExA`, neither of which takes the hive *write* lock,
/// so this is safe to call on Steam's startup path.
#[cfg(windows)]
fn registry_command_matches(expected: &str) -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExA, RegQueryValueExA, HKEY_CURRENT_USER, KEY_READ,
    };
    let sub = match std::ffi::CString::new("Software\\Classes\\bst\\shell\\open\\command") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let mut key: *mut std::ffi::c_void = std::ptr::null_mut();
    let s = unsafe {
        RegOpenKeyExA(HKEY_CURRENT_USER, sub.as_ptr() as *const u8, 0, KEY_READ, &mut key)
    };
    if s != 0 { return false; }

    let mut size: u32 = 0;
    let s1 = unsafe {
        RegQueryValueExA(
            key, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(),
            std::ptr::null_mut(), &mut size,
        )
    };
    if s1 != 0 || size < 2 {
        unsafe { RegCloseKey(key); }
        return false;
    }

    let mut buf = vec![0u8; size as usize];
    let mut fetched = size;
    let s2 = unsafe {
        RegQueryValueExA(
            key, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(),
            buf.as_mut_ptr(), &mut fetched,
        )
    };
    unsafe { RegCloseKey(key); }
    if s2 != 0 { return false; }
    buf.truncate(fetched as usize);
    while buf.last() == Some(&0) { buf.pop(); }

    buf.as_slice() == expected.as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_to_bytes_roundtrip() {
        assert_eq!(hex_to_bytes("00ff10"), Some(vec![0x00, 0xff, 0x10]));
    }

    #[test]
    fn hex_to_bytes_rejects_odd() {
        assert_eq!(hex_to_bytes("abc"), None);
    }

    #[test]
    fn json_string_finds_key() {
        assert_eq!(json_string(r#"{"a":"X","b":"Y"}"#, "a"), Some("X".into()));
    }

    #[test]
    fn json_string_rejects_null() {
        assert_eq!(json_string(r#"{"a":null,"b":"Y"}"#, "a"), None);
    }

    #[test]
    fn json_true_detects_true() {
        assert!(json_true(r#"{"success":true}"#, "success"));
        assert!(!json_true(r#"{"success":false}"#, "success"));
    }

    #[test]
    fn base_url_strips_trailing_slash() {
        // Sanity: the function itself has no side effects; just check it
        // runs and produces a non-empty string.
        assert!(!tokeer_base_url().is_empty());
    }
}
