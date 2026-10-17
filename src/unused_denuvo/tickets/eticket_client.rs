//! On-demand eticket mint. Source: `Utils/Tickets/EticketClient.cpp`.
//!
//! Backend URL resolves in two steps, matching the C++:
//!   1. `seteticketurl()` in the Lua config (runtime override).
//!   2. `option_env!("OST_ETICKET_URL")` — set at build time with
//!      `cargo:rustc-env=OST_ETICKET_URL=...` via `.cargo/config.toml`
//!      or the environment.
//!
//! Empty in both cases means the feature is off and no network call is
//! ever made.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::platform::http;
use crate::utils::config::lua_config;

const K_RESOLVE_MS: u32 = 2000;
const K_CONNECT_MS: u32 = 2000;
const K_SEND_MS:    u32 = 3000;
const K_RECV_MS:    u32 = 8000;

#[derive(Clone, Debug, Default)]
struct CachedTickets {
    eticket: Vec<u8>,
    ownership: Vec<u8>,
    steam_id: u64,
}

#[derive(Default)]
struct Cache {
    tickets: HashMap<u32, CachedTickets>,
    no_owner_apps: HashSet<u32>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut g = CACHE.lock().expect("eticket cache poisoned");
    if g.is_none() { *g = Some(Cache::default()); }
    f(g.as_mut().unwrap())
}

fn eticket_url() -> String {
    let configured = lua_config::get_eticket_url();
    if !configured.is_empty() { return configured; }
    option_env!("OST_ETICKET_URL").unwrap_or("").to_string()
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0F) as usize] as char);
    }
    out
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
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

/// Extract a string field ("key":"VALUE") from the backend's flat JSON.
/// Returns false when the key is absent, its value is null, or the
/// value is empty.
fn extract_string_field(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let k = body.find(&needle)?;
    let colon = body[k + needle.len()..].find(':')? + k + needle.len();
    let q1 = body[colon + 1..].find('"')? + colon + 1;
    // A null value has no opening quote before the next delimiter.
    if let Some(delim) = body[colon + 1..].find(|c| c == ',' || c == '}') {
        if q1 > colon + 1 + delim { return None; }
    }
    let q2 = body[q1 + 1..].find('"')? + q1 + 1;
    let v = &body[q1 + 1..q2];
    if v.is_empty() { None } else { Some(v.to_string()) }
}

/// Ensure a mint has been attempted for `app_id`. On success, copies
/// the cached tickets into `out`.
///
/// Returns false if the feature is disabled, the backend refused, or
/// the response was unusable. On "no owning account" (a 409 without
/// `foreign_account:true`), marks the app as sticky-no-owner for the
/// session.
fn ensure_fetched(
    app_id: u32,
    nonce: &[u8],
    existing_steam_id: u64,
    out: &mut CachedTickets,
) -> bool {
    if eticket_url().is_empty() { return false; }

    {
        let cached = with_cache(|c| {
            if c.no_owner_apps.contains(&app_id) { return None; }
            c.tickets.get(&app_id).cloned()
        });
        if let Some(hit) = cached {
            // Evict if the registry's current account differs from the
            // cached one.
            let cached_id = hit.steam_id;
            if existing_steam_id != 0 && cached_id != 0 && cached_id != existing_steam_id {
                LOG_IPC_DEBUG!("EticketClient: appid={} registry SteamID={} differs from cached SteamID={} — evicting stale cache entry and re-minting",
                               app_id, existing_steam_id, cached_id);
                with_cache(|c| { c.tickets.remove(&app_id); });
            } else {
                *out = hit;
                return true;
            }
        }
        // If we didn't early-return, either the entry was evicted or
        // never existed. The `no_owner_apps` check ran inside the
        // closure above; if it returned None with no cached entry, we
        // might still be blocked.
        let blocked = with_cache(|c| c.no_owner_apps.contains(&app_id));
        if blocked { return false; }
    }

    let nonce_hex = to_hex(nonce);
    let mut req_body = format!("{{\"app_id\":\"{}\",\"nonce\":\"{}\"", app_id, nonce_hex);
    if existing_steam_id != 0 {
        req_body.push_str(&format!(",\"existing_steam_id\":\"{}\"", existing_steam_id));
    }
    req_body.push('}');

    let url = eticket_url();
    let mut req = http::Request::new("POST", &url);
    req.body = Some(req_body.as_bytes());
    req.headers = Some("Content-Type: application/json\r\n");
    req.timeout_resolve_ms = K_RESOLVE_MS;
    req.timeout_connect_ms = K_CONNECT_MS;
    req.timeout_send_ms    = K_SEND_MS;
    req.timeout_recv_ms    = K_RECV_MS;

    let r = http::execute(&req);

    if !r.ok {
        LOG_IPC_WARN!("EticketClient: on-demand fetch failed appid={} status={} ok={} (fallback to credential store)",
                      app_id, r.status, r.ok);
        return false;
    }

    let body_str = String::from_utf8_lossy(&r.body).into_owned();

    if r.status == 409 {
        if body_str.contains("\"foreign_account\":true") {
            LOG_IPC_DEBUG!("EticketClient: appid={} existing ticket belongs to an account outside our pool — skipping on-demand override for this launch",
                           app_id);
        } else {
            with_cache(|c| { c.no_owner_apps.insert(app_id); });
            LOG_IPC_DEBUG!("EticketClient: appid={} no owning account in pool — skipping on-demand for this session",
                           app_id);
        }
        return false;
    }

    if r.status != 200 {
        LOG_IPC_WARN!("EticketClient: on-demand fetch failed appid={} status={} ok={} (fallback to credential store)",
                      app_id, r.status, r.ok);
        return false;
    }

    let mut fetched = CachedTickets::default();
    if let Some(hex) = extract_string_field(&body_str, "eticket") {
        if let Some(bytes) = from_hex(&hex) { fetched.eticket = bytes; }
    }
    if let Some(hex) = extract_string_field(&body_str, "appticket") {
        if let Some(bytes) = from_hex(&hex) { fetched.ownership = bytes; }
    }
    if fetched.eticket.is_empty() && fetched.ownership.is_empty() {
        LOG_IPC_WARN!("EticketClient: backend returned no usable tickets appid={} bytes={}",
                      app_id, r.body.len());
        return false;
    }
    if let Some(sid) = extract_string_field(&body_str, "steam_id") {
        fetched.steam_id = sid.parse::<u64>().unwrap_or(0);
    }

    with_cache(|c| { c.tickets.insert(app_id, fetched.clone()); });
    LOG_IPC_INFO!("EticketClient: minted appid={} eticket_bytes={} ownership_bytes={} nonce_bytes={}",
                  app_id, fetched.eticket.len(), fetched.ownership.len(), nonce.len());
    *out = fetched;
    true
}

pub fn fetch_fresh_eticket(app_id: u32, nonce: &[u8], existing_steam_id: u64) -> Option<Vec<u8>> {
    let mut t = CachedTickets::default();
    if !ensure_fetched(app_id, nonce, existing_steam_id, &mut t) || t.eticket.is_empty() {
        return None;
    }
    Some(t.eticket)
}

pub fn fetch_ownership_ticket(app_id: u32, nonce: &[u8], existing_steam_id: u64) -> Option<Vec<u8>> {
    let mut t = CachedTickets::default();
    if !ensure_fetched(app_id, nonce, existing_steam_id, &mut t) || t.ownership.is_empty() {
        return None;
    }
    Some(t.ownership)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_hex_basic() {
        assert_eq!(to_hex(&[0x00, 0xFF, 0x10]), "00FF10");
    }

    #[test]
    fn from_hex_roundtrip() {
        let bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
        assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes));
    }

    #[test]
    fn from_hex_rejects_odd_length() {
        assert_eq!(from_hex("abc"), None);
    }

    #[test]
    fn from_hex_rejects_empty() {
        assert_eq!(from_hex(""), None);
    }

    #[test]
    fn extract_string_field_finds_key() {
        let body = r#"{"eticket":"ABCD","steam_id":"12345"}"#;
        assert_eq!(extract_string_field(body, "eticket"), Some("ABCD".into()));
        assert_eq!(extract_string_field(body, "steam_id"), Some("12345".into()));
    }

    #[test]
    fn extract_string_field_rejects_null() {
        let body = r#"{"appticket":null,"eticket":"AB"}"#;
        assert_eq!(extract_string_field(body, "appticket"), None);
    }

    #[test]
    fn extract_string_field_missing_key() {
        let body = r#"{"eticket":"AB"}"#;
        assert_eq!(extract_string_field(body, "nope"), None);
    }
}
