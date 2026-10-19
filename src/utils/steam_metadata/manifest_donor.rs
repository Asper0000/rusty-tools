//! Manifest code donor. Source: `ManifestDonor.cpp`.
//!
//! Two paths:
//!   * active worker mints codes for owned depots in the wanted list;
//!   * `submit_captured_code` fires on every real download.
//!
//! Both post to `<base>/manifestcode/submit`. The wanted list is fetched
//! from `<base>/manifestwanted`. Both are plain text lines
//! `app:depot:gid` / `depot:gid:code`.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

use crate::bridge;
use crate::platform::http;
use crate::utils::config::config;

const DEFAULT_BASE_URL: &str = "https://manifest.luastools.xyz";

/// Per-mint timeout for the donor worker. The IPC-facing path uses the
/// full 30 s budget because Steam itself is waiting on the reply; the
/// donor runs detached and 25 slow mints x 30 s = 12 min per cycle.
const K_DONOR_MINT_TIMEOUT_MS: u32 = 5000;

static RUNNING: AtomicBool = AtomicBool::new(false);
static MINTED_THIS_SESSION: AtomicU32 = AtomicU32::new(0);

struct Wanted { app_id: u32, depot_id: u32, gid: u64 }

struct WantedCache {
    items: Vec<Wanted>,
    fetched_at: Option<Instant>,
}

static WANTED: Mutex<Option<WantedCache>> = Mutex::new(None);

fn base_url() -> String {
    let cfg = config::get_donate_settings();
    let mut base = if cfg.url.is_empty() { DEFAULT_BASE_URL.to_string() } else { cfg.url };
    while base.ends_with('/') { base.pop(); }
    base
}

fn parse_wanted_line(line: &str) -> Option<Wanted> {
    let mut it = line.split(':');
    let app: u32 = it.next()?.parse().ok()?;
    let depot: u32 = it.next()?.parse().ok()?;
    let gid: u64 = it.next()?.parse().ok()?;
    if depot == 0 || gid == 0 { return None; }
    Some(Wanted { app_id: app, depot_id: depot, gid })
}

fn fetch_wanted() -> Vec<Wanted> {
    let url = format!("{}/manifestwanted", base_url());
    let mut req = http::Request::new("GET", &url);
    req.timeout_resolve_ms = 5000;
    req.timeout_connect_ms = 5000;
    req.timeout_send_ms = 10000;
    req.timeout_recv_ms = 30000;
    req.max_body_bytes = 32 * 1024 * 1024;

    let r = http::execute(&req);
    if !r.ok || r.status != 200 {
        LOG_MANIFEST_DEBUG!("Donor: wanted list unavailable (ok={} status={})", r.ok, r.status);
        return Vec::new();
    }

    let text = String::from_utf8_lossy(&r.body);
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() { continue; }
        match parse_wanted_line(trimmed) {
            Some(w) => out.push(w),
            None => LOG_MANIFEST_DEBUG!("Donor: ignoring malformed wanted line \"{}\"", trimmed),
        }
    }
    LOG_MANIFEST_DEBUG!("Donor: {} wanted entr(ies)", out.len());
    out
}

fn submit(body: &str, count: usize) {
    let url = format!("{}/manifestcode/submit", base_url());
    let mut req = http::Request::new("POST", &url);
    req.body = Some(body.as_bytes());
    req.headers = Some("Content-Type: text/plain\r\n");
    let r = http::execute(&req);
    if r.ok && r.status == 200 {
        LOG_MANIFEST_INFO!("Donor: submitted {} code(s)", count);
    } else {
        LOG_MANIFEST_WARN!("Donor: submit failed (ok={} status={}) for {} code(s)",
                           r.ok, r.status, count);
    }
}

fn already_cached(depot: u32, gid: u64) -> bool {
    let url = format!("{}/m/{}/{}", base_url(), depot, gid);
    let mut req = http::Request::new("HEAD", &url);
    req.timeout_resolve_ms = 5000;
    req.timeout_connect_ms = 5000;
    req.timeout_send_ms = 5000;
    req.timeout_recv_ms = 5000;
    let r = http::execute(&req);
    r.ok && r.status == 200
}

fn refresh_wanted_if_stale() {
    let refresh_secs = config::get_donate_settings().wanted_refresh_secs as u64;
    let needs = {
        let g = WANTED.lock().expect("poisoned");
        match g.as_ref() {
            None => true,
            Some(c) => match c.fetched_at {
                None => true,
                Some(t) => t.elapsed() >= Duration::from_secs(refresh_secs),
            },
        }
    };
    if !needs { return; }

    let fresh = fetch_wanted();
    let mut g = WANTED.lock().expect("poisoned");
    if !fresh.is_empty() {
        *g = Some(WantedCache { items: fresh, fetched_at: Some(Instant::now()) });
    } else if g.is_none() {
        *g = Some(WantedCache { items: Vec::new(), fetched_at: Some(Instant::now()) });
    }
}

fn run_cycle() {
    let cfg = config::get_donate_settings();

    // Owned depot set from the hook layer (stubbed today: not ready).
    let owned = bridge::hooks_get_owned_depots();
    if !owned.0 {
        LOG_MANIFEST_DEBUG!("Donor: owned depots not resolved yet, skipping cycle");
        return;
    }
    if cfg.max_mints_per_session != 0
        && MINTED_THIS_SESSION.load(Ordering::Relaxed) >= cfg.max_mints_per_session
    {
        LOG_MANIFEST_DEBUG!("Donor: session cap reached ({}), idle until restart", cfg.max_mints_per_session);
        return;
    }

    refresh_wanted_if_stale();
    let wanted: Vec<Wanted> = {
        let g = WANTED.lock().expect("poisoned");
        g.as_ref().map(|c| c.items.iter().map(|w| Wanted { app_id: w.app_id, depot_id: w.depot_id, gid: w.gid }).collect()).unwrap_or_default()
    };
    if wanted.is_empty() { return; }

    let mut body = String::new();
    let mut submitted: usize = 0;
    let mut skipped: usize = 0;
    let mut refused: usize = 0;
    let mut already: usize = 0;
    let mut minted = false;

    for w in wanted {
        if !RUNNING.load(Ordering::Relaxed) { break; }
        if submitted >= cfg.max_mints_per_cycle as usize { break; }
        if cfg.max_mints_per_session != 0
            && MINTED_THIS_SESSION.load(Ordering::Relaxed) >= cfg.max_mints_per_session { break; }

        if !owned.1.contains(&w.depot_id) { skipped += 1; continue; }
        if already_cached(w.depot_id, w.gid) { already += 1; continue; }

        if minted && cfg.min_mint_interval_ms != 0 {
            std::thread::sleep(Duration::from_millis(cfg.min_mint_interval_ms as u64));
            if !RUNNING.load(Ordering::Relaxed) { break; }
        }
        minted = true;

        let mut app_id = w.app_id;
        if app_id == 0 {
            if let Some((known_app, _)) = bridge::hooks_lookup_depot(w.depot_id) {
                if known_app != 0 {
                    app_id = known_app;
                    LOG_MANIFEST_DEBUG!("Donor: depot={} app_id 0 -> {}", w.depot_id, app_id);
                }
            }
        }

        let code = bridge::hooks_request_manifest_code_with_timeout(
            app_id, w.depot_id, w.gid, K_DONOR_MINT_TIMEOUT_MS,
        );
        MINTED_THIS_SESSION.fetch_add(1, Ordering::Relaxed);

        if code == 0 { refused += 1; }
        else {
            body.push_str(&format!("{}:{}:{}\n", w.depot_id, w.gid, code));
            submitted += 1;
            LOG_MANIFEST_DEBUG!("Donor: minted depot={} gid={}", w.depot_id, w.gid);
        }
    }

    LOG_MANIFEST_INFO!("Donor: cycle done — {} minted, {} already archived, {} not owned, {} refused by Steam ({} this session)",
                       submitted, already, skipped, refused, MINTED_THIS_SESSION.load(Ordering::Relaxed));

    if submitted > 0 { submit(&body, submitted); }
}

fn worker_main() {
    LOG_MANIFEST_INFO!("Donor: started");
    while RUNNING.load(Ordering::Relaxed) {
        let cfg = config::get_donate_settings();
        if cfg.enabled { run_cycle(); }
        for _ in 0..cfg.interval_secs {
            if !RUNNING.load(Ordering::Relaxed) { break; }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    LOG_MANIFEST_INFO!("Donor: stopped");
}

struct CapturedCode {
    depot_id: u32,
    gid: u64,
    code: u64,
}

static CAPTURE_TX: OnceLock<SyncSender<CapturedCode>> = OnceLock::new();

fn capture_worker(rx: Receiver<CapturedCode>) {
    loop {
        let Ok(first) = rx.recv() else { return };
        let mut batch = vec![first];
        // Drain whatever else is queued without blocking.
        while batch.len() < 50 {
            match rx.try_recv() {
                Ok(c) => batch.push(c),
                Err(_) => break,
            }
        }
        let mut body = String::new();
        for c in &batch {
            body.push_str(&format!("{}:{}:{}\n", c.depot_id, c.gid, c.code));
        }
        submit(&body, batch.len());
        // Brief pacing between batches so a burst of captures during a
        // large download does not hammer the donor endpoint.
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Called from `manifest::handle_recv` on the network thread. Non-blocking:
/// codes go into a bounded queue drained by a single background worker.
/// If the queue is full, the code is dropped (donor can pick it up on a
/// future download).
pub fn submit_captured_code(depot_id: u32, gid: u64, code: u64) {
    if depot_id == 0 || gid == 0 || code == 0 { return; }
    if !config::get_donate_settings().enabled { return; }

    let tx = CAPTURE_TX.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<CapturedCode>(256);
        if let Err(e) = std::thread::Builder::new()
            .name("ost-donor-capture".into())
            .spawn(move || capture_worker(rx))
        {
            LOG_MANIFEST_WARN!("Donor: capture worker spawn failed: {}", e);
        }
        tx
    });
    let _ = tx.try_send(CapturedCode { depot_id, gid, code });
}

pub fn start() {
    if !config::get_donate_settings().enabled {
        LOG_MANIFEST_INFO!("Donor: disabled by config");
        return;
    }
    if RUNNING.swap(true, Ordering::Relaxed) {
        LOG_MANIFEST_WARN!("Donor: already running");
        return;
    }
    std::thread::Builder::new()
        .name("ost-donor".into())
        .spawn(worker_main)
        .map(|_| ())
        .unwrap_or_else(|_| {
            LOG_MANIFEST_WARN!("Donor: failed to start worker thread");
            RUNNING.store(false, Ordering::Relaxed);
        });
}

pub fn stop() { RUNNING.store(false, Ordering::Relaxed); }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_wanted_line_accepts_app_zero() {
        let w = parse_wanted_line("0:100:200").unwrap();
        assert_eq!(w.app_id, 0);
        assert_eq!(w.depot_id, 100);
        assert_eq!(w.gid, 200);
    }

    #[test]
    fn parse_wanted_line_rejects_depot_zero() {
        assert!(parse_wanted_line("0:0:200").is_none());
    }

    #[test]
    fn parse_wanted_line_rejects_malformed() {
        assert!(parse_wanted_line("garbage").is_none());
        assert!(parse_wanted_line("1:2").is_none());
        assert!(parse_wanted_line("1:2:x").is_none());
    }
}
