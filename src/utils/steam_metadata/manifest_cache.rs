//! Manifest archive cache. Source: `ManifestCache.cpp`.
//!
//! Reads and writes to `<steam>/depotcache/<depot>_<gid>.manifest`.
//! Negative-cache TTL for 404s; atomic write via rename.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::platform::{dynamic_library, http};
use crate::utils::config::config;

const ARCHIVE_BASE_URL: &str = "https://manifest.luastools.xyz";
const MAX_BODY_BYTES: u32 = 64 * 1024 * 1024;
const RECV_TIMEOUT_MS: u32 = 60000;
const NEGATIVE_TTL: Duration = Duration::from_secs(600);
const MIN_BODY_BYTES: usize = 16;
const PAYLOAD_MAGIC: [u8; 4] = [0xD0, 0x17, 0xF6, 0x71];
const EOF_MAGIC: [u8; 4] = [0xAB, 0x15, 0xC4, 0x32];

struct NegativeCache {
    map: HashMap<String, Instant>,
    in_flight: HashSet<String>,
}

static CACHE: Mutex<Option<NegativeCache>> = Mutex::new(None);

fn with_cache<R>(f: impl FnOnce(&mut NegativeCache) -> R) -> R {
    let mut g = CACHE.lock().expect("poisoned");
    if g.is_none() {
        *g = Some(NegativeCache { map: HashMap::new(), in_flight: HashSet::new() });
    }
    f(g.as_mut().unwrap())
}

struct InFlightGuard { key: String, acquired: bool }
impl InFlightGuard {
    fn new(key: String) -> Self {
        let acquired = with_cache(|c| c.in_flight.insert(key.clone()));
        Self { key, acquired }
    }
}
impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if !self.acquired { return; }
        with_cache(|c| { c.in_flight.remove(&self.key); });
    }
}

fn depot_cache_dir() -> Option<PathBuf> {
    let exe = dynamic_library::get_main_executable_path();
    if exe.as_os_str().is_empty() { return None; }
    Some(exe.parent()?.join("depotcache"))
}

fn looks_like_manifest(body: &[u8]) -> bool {
    if body.len() < MIN_BODY_BYTES { return false; }
    if &body[..4] != PAYLOAD_MAGIC { return false; }
    if &body[body.len() - 4..] != EOF_MAGIC { return false; }
    true
}

fn write_atomic(dest: &std::path::Path, body: &[u8]) -> bool {
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            LOG_MANIFEST_WARN!("ManifestCache: cannot create {}: {}", parent.display(), e);
            return false;
        }
    }
    let tmp = dest.with_extension(format!("{}.tmp", std::process::id()));
    if let Err(e) = std::fs::write(&tmp, body) {
        LOG_MANIFEST_WARN!("ManifestCache: write failed for {}: {}", tmp.display(), e);
        return false;
    }
    match std::fs::rename(&tmp, dest) {
        Ok(_) => true,
        Err(e) => {
            LOG_MANIFEST_WARN!("ManifestCache: rename {} -> {} failed: {}",
                               tmp.display(), dest.display(), e);
            let _ = std::fs::remove_file(&tmp);
            false
        }
    }
}

/// Ensure `<steam>/depotcache/<depot>_<gid>.manifest` exists, fetching from
/// the archive if not. Returns true when the file is present afterwards.
pub fn ensure_cached(
    app: u32,
    depot: u32,
    gid: u64,
    recv_timeout_ms: u32,
    bypass_negative_cache: bool,
) -> bool {
    ensure_cached_ex(app, depot, gid, recv_timeout_ms, bypass_negative_cache).0
}

/// Extended version that also reports "definitively not archived" (404).
pub fn ensure_cached_ex(
    _app: u32,
    depot: u32,
    gid: u64,
    recv_timeout_ms: u32,
    bypass_negative_cache: bool,
) -> (bool, bool) {
    if depot == 0 || gid == 0 { return (false, false); }

    let Some(dir) = depot_cache_dir() else {
        LOG_MANIFEST_WARN!("ManifestCache: could not resolve depotcache dir");
        return (false, false);
    };
    let dest = dir.join(format!("{}_{}.manifest", depot, gid));
    if dest.exists() { return (true, false); }

    let key = format!("{}_{}", depot, gid);

    if !bypass_negative_cache {
        let cached_at = with_cache(|c| c.map.get(&key).copied());
        if let Some(t) = cached_at {
            if t.elapsed() < NEGATIVE_TTL { return (false, false); }
            with_cache(|c| { c.map.remove(&key); });
        }
    }

    let guard = InFlightGuard::new(key.clone());
    if !guard.acquired { return (false, false); }

    if dest.exists() { return (true, false); }

    let url = format!("{}/m/{}/{}", ARCHIVE_BASE_URL, depot, gid);
    let to = config::get_manifest_timeouts();
    let mut req = http::Request::new("GET", &url);
    req.timeout_resolve_ms = to.resolve;
    req.timeout_connect_ms = to.connect;
    req.timeout_send_ms = to.send;
    req.timeout_recv_ms = if recv_timeout_ms != 0 { recv_timeout_ms } else { RECV_TIMEOUT_MS };
    req.max_body_bytes = MAX_BODY_BYTES;

    let resp = http::execute(&req);

    if !resp.ok || resp.status != 200 {
        let not_archived = resp.ok && resp.status == 404;
        if not_archived {
            with_cache(|c| { c.map.insert(key, Instant::now()); });
        }
        LOG_MANIFEST_TRACE!("ManifestCache: miss depot={} gid={} status={}", depot, gid, resp.status);
        return (false, not_archived);
    }

    if !looks_like_manifest(&resp.body) {
        LOG_MANIFEST_WARN!("ManifestCache: rejected body for depot={} gid={} ({} bytes)",
                           depot, gid, resp.body.len());
        return (false, false);
    }

    if !write_atomic(&dest, &resp.body) { return (false, false); }

    LOG_MANIFEST_INFO!("ManifestCache: cached depot={} gid={} ({} bytes) -> {}",
                       depot, gid, resp.body.len(), dest.display());
    (true, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_manifest_requires_magics() {
        let mut body = vec![0u8; 32];
        body[..4].copy_from_slice(&PAYLOAD_MAGIC);
        body[28..].copy_from_slice(&EOF_MAGIC);
        assert!(looks_like_manifest(&body));

        body[0] = 0;
        assert!(!looks_like_manifest(&body));
    }

    #[test]
    fn looks_like_manifest_rejects_short() {
        assert!(!looks_like_manifest(&[0; 4]));
    }
}
