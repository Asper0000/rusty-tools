//! Remote TOML fetch + cache. Source: `RemoteToml.cpp`.

use std::path::PathBuf;

use crate::platform::http;
use crate::utils::config::config;
use super::steam_diagnostics;

pub struct Request {
    pub channel: String,
    pub component: String,
    pub dll_path: String,
}

#[derive(Default)]
pub struct Result {
    pub ok: bool,
    pub from_cache: bool,
    pub body: String,
    pub sha256: String,
}

const DEFAULT_TEMPLATES: &[&str] = &[
    "https://raw.githubusercontent.com/madoiscool/steam-monitor/{channel}/{component}/{sha256}.toml",
    "https://cdn.jsdelivr.net/gh/madoiscool/steam-monitor@{channel}/{component}/{sha256}.toml",
    "https://git.lua.tools/luatools/steam-monitor/raw/branch/{channel}/{component}/{sha256}.toml",
];

fn is_valid_template(t: &str) -> bool {
    t.contains("{channel}") && t.contains("{component}") && t.contains("{sha256}")
}

fn expand(tmpl: &str, req: &Request, sha: &str) -> String {
    tmpl.replace("{channel}", &req.channel)
        .replace("{component}", &req.component)
        .replace("{sha256}", sha)
}

fn build_url_templates() -> Vec<String> {
    let configured = config::get_remote_url_templates();
    if configured.is_empty() {
        return DEFAULT_TEMPLATES.iter().map(|s| s.to_string()).collect();
    }
    let mut out = Vec::new();
    for t in &configured {
        if is_valid_template(t) { out.push(t.clone()); }
        else { LOG_WARN!("RemoteToml: remote.url_template entry missing {{channel}}/{{component}}/{{sha256}}, skipping: {}", t); }
    }
    if out.is_empty() { LOG_WARN!("RemoteToml: no valid remote.url_template entries; remote fetch disabled"); }
    out
}

fn refresh_in_background(req: &Request, sha: &str, cache_path: &std::path::Path) {
    let templates = build_url_templates();
    for tmpl in &templates {
        let url = expand(tmpl, req, sha);
        let r = http::execute(&http::Request::new("GET", &url));
        if r.ok && r.status == 200 && !r.body.is_empty() {
            if std::fs::write(cache_path, &r.body).is_ok() {
                LOG_INFO!("RemoteToml({}/{}): background refresh from {}",
                          req.channel, req.component, url);
            }
            return;
        }
    }
    LOG_DEBUG!("RemoteToml({}/{}): background refresh failed (all mirrors)",
               req.channel, req.component);
}

pub fn fetch(req: &Request) -> Result {
    let mut out = Result::default();

    out.sha256 = steam_diagnostics::sha256_of(&req.dll_path);
    if out.sha256.is_empty() {
        LOG_WARN!("RemoteToml({}/{}): Sha256OfFile failed for {}", req.channel, req.component, req.dll_path);
        return out;
    }
    LOG_INFO!("RemoteToml({}/{}): sha256 = {}", req.channel, req.component, out.sha256);

    let dll_path = PathBuf::from(&req.dll_path);
    let steam_root = dll_path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let cache_dir = steam_root.join("rustytools").join(&req.channel).join(&req.component);
    let cache_path = cache_dir.join(format!("{}.toml", out.sha256));

    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        LOG_WARN!("RemoteToml({}/{}): could not create cache dir {} ({})",
                  req.channel, req.component, cache_dir.display(), e);
    }

    // Cache-first: if we already have this SHA's patterns, return
    // them immediately and refresh in the background. This keeps the
    // network off the startup path, which matters because the mirror
    // chain's worst-case time is 3 Ãƒâ€” 30 s and Steam's startup watchdog
    // is much shorter than that.
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if !text.is_empty() {
            LOG_INFO!("RemoteToml({}/{}): cache hit {}",
                      req.channel, req.component, out.sha256);
            out.body = text;
            out.ok = true;
            out.from_cache = true;
            let req_clone = Request {
                channel: req.channel.clone(),
                component: req.component.clone(),
                dll_path: req.dll_path.clone(),
            };
            let sha = out.sha256.clone();
            let path = cache_path.clone();
            std::thread::spawn(move || {
                refresh_in_background(&req_clone, &sha, &path);
            });
            return out;
        }
    }

    let templates = build_url_templates();
    let mut last_status: u32 = 0;
    let mut last_url = String::new();
    let mut got_body: Option<Vec<u8>> = None;

    for tmpl in &templates {
        let url = expand(tmpl, req, &out.sha256);
        last_url = url.clone();
        LOG_INFO!("RemoteToml({}/{}): downloading {}", req.channel, req.component, url);
        let r = http::execute(&http::Request::new("GET", &url));
        last_status = r.status;
        if r.ok && r.status == 200 { got_body = Some(r.body); break; }
        let tail = if last_url == url { "trying next" } else { "no more mirrors" };
        LOG_WARN!("RemoteToml({}/{}): mirror failed ({} ok={} HTTP={}), {}",
                  req.channel, req.component, url, r.ok, r.status, tail);
    }

    if let Some(body) = got_body {
        if !body.is_empty() {
            if let Err(e) = std::fs::write(&cache_path, &body) {
                LOG_WARN!("RemoteToml({}/{}): could not write {} ({})",
                          req.channel, req.component, cache_path.display(), e);
            } else {
                LOG_INFO!("RemoteToml({}/{}): cached to {}",
                          req.channel, req.component, cache_path.display());
            }
            out.body = String::from_utf8_lossy(&body).into_owned();
            out.ok = true;
            return out;
        }
    }

    if cache_path.exists() {
        LOG_WARN!("RemoteToml({}/{}): remote failed (last URL {} HTTP {}); falling back to local cache {}",
                  req.channel, req.component, last_url, last_status, cache_path.display());
        match std::fs::read_to_string(&cache_path) {
            Ok(buf) if !buf.is_empty() => {
                out.body = buf;
                out.ok = true;
                out.from_cache = true;
                return out;
            }
            Ok(_) => LOG_WARN!("RemoteToml({}/{}): cache file empty: {}",
                               req.channel, req.component, cache_path.display()),
            Err(e) => LOG_WARN!("RemoteToml({}/{}): could not open cache file: {} ({})",
                                req.channel, req.component, cache_path.display(), e),
        }
    }

    LOG_WARN!("RemoteToml({}/{}): no source available (last URL: {} HTTP {})",
              req.channel, req.component, last_url, last_status);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_validation() {
        assert!(is_valid_template("x/{channel}/{component}/{sha256}.toml"));
        assert!(!is_valid_template("x/{channel}.toml"));
    }

    #[test]
    fn expand_replaces_all() {
        let req = Request { channel: "p".into(), component: "c".into(), dll_path: String::new() };
        let u = expand("https://x/{channel}/{component}/{sha256}", &req, "abc");
        assert_eq!(u, "https://x/p/c/abc");
    }
}
