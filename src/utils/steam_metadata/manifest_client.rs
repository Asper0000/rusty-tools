//! Manifest request-code client. Source: `ManifestClient.cpp`.

use std::sync::Mutex;

use crate::platform::http;
use crate::utils::config::{config, lua_config};

struct Provider {
    name: &'static str,
    url_template: &'static str,
    url_template_ex: Option<&'static str>,
    parse: fn(&[u8]) -> Option<u64>,
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "opensteamtool",
        url_template: "https://manifest.opensteamtool.com/%llu",
        url_template_ex: Some("https://manifest.opensteamtool.com/%u/%u/%llu"),
        parse: parse_plain_uint,
    },
    Provider {
        name: "wudrm",
        url_template: "http://gmrc.wudrm.com/manifest/%llu",
        url_template_ex: None,
        parse: parse_plain_uint,
    },
    Provider {
        name: "steamrun",
        url_template: "https://manifest.steam.run/api/manifest/%llu",
        url_template_ex: None,
        parse: parse_steam_run_json,
    },
];

static ACTIVE_INDEX: Mutex<usize> = Mutex::new(0);

fn parse_plain_uint(body: &[u8]) -> Option<u64> {
    std::str::from_utf8(body).ok().and_then(|s| s.parse::<u64>().ok())
}

fn parse_steam_run_json(body: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(body).ok()?;
    let key = s.find("\"content\"")?;
    let q1 = s[key + 9..].find('"')? + key + 9;
    let q2 = s[q1 + 1..].find('"')? + q1 + 1;
    parse_plain_uint(s[q1 + 1..q2].as_bytes())
}

/// Builds a URL from a `%llu`/`%u` template. Order of `%u` in `template_ex`
/// matches the C++ snprintf: app_id, depot_id, gid.
fn format_url(template: &str, app_id: u32, depot_id: u32, gid: u64) -> String {
    let mut out = template.to_string();
    if out.contains("%u/%u/%llu") {
        out = out.replacen("%u", &app_id.to_string(), 1);
        out = out.replacen("%u", &depot_id.to_string(), 1);
        out = out.replacen("%llu", &gid.to_string(), 1);
    } else {
        out = out.replacen("%llu", &gid.to_string(), 1);
    }
    out
}

pub fn set_provider(name: &str) -> bool {
    for (i, p) in PROVIDERS.iter().enumerate() {
        if p.name == name {
            *ACTIVE_INDEX.lock().expect("poisoned") = i;
            return true;
        }
    }
    false
}

pub fn active_provider_name() -> &'static str {
    PROVIDERS[*ACTIVE_INDEX.lock().expect("poisoned")].name
}

pub fn shutdown() {}

fn fetch_active(gid: u64, app_id: u32, depot_id: u32) -> Option<u64> {
    let idx = *ACTIVE_INDEX.lock().expect("poisoned");
    let p = &PROVIDERS[idx];
    let to = config::get_manifest_timeouts();

    let (url, shape) = match p.url_template_ex {
        Some(ex) if depot_id != 0 => (format_url(ex, app_id, depot_id, gid), "app/depot/gid"),
        _ => (format_url(p.url_template, app_id, depot_id, gid), "gid-only"),
    };

    let mut req = http::Request::new("GET", &url);
    req.timeout_resolve_ms = to.resolve;
    req.timeout_connect_ms = to.connect;
    req.timeout_send_ms = to.send;
    req.timeout_recv_ms = to.recv;
    let r = http::execute(&req);

    LOG_MANIFEST_INFO!("Manifest {} status={} gid={} depot={} shape={}",
                       p.name, r.status, gid, depot_id, shape);

    if !r.ok || r.status != 200 { return None; }
    (p.parse)(&r.body)
}

pub fn fetch_manifest_request_code(gid: u64, app_id: u32, depot_id: u32) -> Option<u64> {
    if app_id != 0 && depot_id != 0 && lua_config::has_manifest_code_func_ex() {
        if let Some(code) = lua_config::call_manifest_fetch_code_ex(app_id as u64, depot_id as u64, gid) {
            LOG_MANIFEST_INFO!("Manifest gid={} resolved via fetch_manifest_code_ex", gid);
            return Some(code);
        }
        LOG_MANIFEST_WARN!("Manifest gid={} fetch_manifest_code_ex returned nil, trying fetch_manifest_code", gid);
    }
    if lua_config::has_manifest_code_func() {
        if let Some(code) = lua_config::call_manifest_fetch_code(gid) {
            LOG_MANIFEST_INFO!("Manifest gid={} resolved via manifest.lua", gid);
            return Some(code);
        }
        LOG_MANIFEST_WARN!("Manifest gid={} lua returned nil, falling back to config", gid);
    }
    fetch_active(gid, app_id, depot_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_url_ex_shape() {
        let u = format_url("https://x/%u/%u/%llu", 100, 200, 9999);
        assert_eq!(u, "https://x/100/200/9999");
    }

    #[test]
    fn format_url_gid_only() {
        let u = format_url("https://x/%llu", 0, 0, 9999);
        assert_eq!(u, "https://x/9999");
    }

    #[test]
    fn parse_steam_run_extracts_content() {
        let b = br#"{"content": "12345678901234"}"#;
        assert_eq!(parse_steam_run_json(b), Some(12345678901234));
    }
}
