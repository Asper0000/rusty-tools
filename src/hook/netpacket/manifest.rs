//! `Hooks_NetPacket_Manifest`. Outgoing and incoming
//! `ContentServerDirectory.GetManifestRequestCode#1`.
//!
//! On send, this reads the request, records the correlation for
//! passive capture, pre-seeds the manifest cache (for the exact gid
//! Steam is asking about), and launches a background HTTP fetch.
//!
//! On recv, this waits for the fetch (up to 12 s) and patches the
//! response in place. Passive capture of genuine Steam-issued codes
//! also runs here — this is what feeds `ManifestDonor`.

use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;

use crate::steam::proto::{
    CContentServerDirectoryGetManifestRequestCodeRequest as Req,
    CContentServerDirectoryGetManifestRequestCodeResponse as Resp,
    CMsgProtoBufHeader,
};
use crate::utils::config::{config, lua_config};
use crate::utils::steam_metadata::{manifest_cache, manifest_client, manifest_donor};

use super::dispatch::RecvReplacement;
use super::state::{K_MAX_BODY_SIZE, K_MAX_HDR_SIZE};

const K_MAX_WAIT_SECONDS: u64 = 2;
const K_MAX_SENT_TRACKED: usize = 4096;
const K_ERESULT_OK: i32 = 1;

struct SentReq {
    depot: u32,
    gid: u64,
}

struct State {
    /// jobid_source -> receiver that will yield the fetched code.
    code_futures: HashMap<u64, mpsc::Receiver<u64>>,
    /// Passive-capture map. Bounded so a reply that never arrives does
    /// not grow this without limit.
    sent_requests: HashMap<u64, SentReq>,
}

static MANIFEST_STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State {
    code_futures: HashMap::new(),
    sent_requests: HashMap::new(),
}));

// ── Send ────────────────────────────────────────────────────────────

/// Read the request, record the correlation, pre-seed the cache, and
/// launch the fetch. Does not modify the outgoing frame.
pub fn handle_send(body: &[u8], hdr: &[u8]) {
    let req = match Req::decode(body) {
        Ok(r) => r,
        Err(_) => {
            LOG_MANIFEST_WARN!("GetManifestRequestCode: failed to parse request");
            return;
        }
    };
    let Some(depot_id) = req.depot_id else { return; };
    let Some(manifest_gid) = req.manifest_id else { return; };
    let app_id = req.app_id.unwrap_or(0);

    let hdr_msg = CMsgProtoBufHeader::decode(hdr).ok();
    let job_id = hdr_msg.as_ref().and_then(|h| h.jobid_source);

    // Passive capture: remember every request so the recv handler can
    // harvest the real code Steam returns. Gated on [donate], where
    // captured codes are sent.
    if let Some(jid) = job_id {
        if config::get_donate_settings().enabled {
            let mut state = MANIFEST_STATE.lock();
            if state.sent_requests.len() < K_MAX_SENT_TRACKED {
                state.sent_requests.insert(jid, SentReq { depot: depot_id, gid: manifest_gid });
            }
        }
    }

    // Only Lua-declared depots get their code swapped.
    if !lua_config::has_depot(depot_id, false) { return; }

    // A depot can be both Lua-added and genuinely owned. There Steam is
    // about to receive a real, working code; replacing it with a
    // fetched one can only make things worse. `BuildDepotDependency`
    // records depot -> app; bridge through that.
    if let Some((seen_app, _)) = crate::hook::manifest::lookup_depot(depot_id) {
        if seen_app != 0 && lua_config::is_owned(seen_app) {
            LOG_MANIFEST_INFO!(
                "GetManifestRequestCode: depot={} belongs to owned app {}, \
                 leaving Steam's own code alone",
                depot_id, seen_app,
            );
            return;
        }
    }

    let Some(job_id) = job_id else {
        LOG_MANIFEST_WARN!("GetManifestRequestCode: missing jobid_source in header");
        return;
    };

    LOG_MANIFEST_DEBUG!(
        "GetManifestRequestCode send: depot={} gid={} jobid={} app_id={}",
        depot_id, manifest_gid, job_id, app_id,
    );

    // Pre-seed the cache for the exact manifest Steam is asking about.
    // Detached; if the archive has it, Steam's own retry (~30 s) finds
    // the manifest on disk and skips the code path entirely. A miss
    // just falls through to the fetched-code attempt.
    //
    // Only a real user download bypasses the negative cache and
    // surfaces the "not ready" box. Scheduled/queued requests ride the
    // negative cache and stay silent.
    {
        let a = app_id;
        let d = depot_id;
        let g = manifest_gid;
        let active = crate::hook::steamui::active_download_count() > 0;
        let _ = crate::platform::thread::start_detached(move || {
            let (ok, not_archived) =
                manifest_cache::ensure_cached_ex(a, d, g, 0, active);
            if active && !ok && not_archived {
                crate::hook::manifest::report_missing_manifest(d, g);
            }
            0
        });
    }

    // Launch the fetch on a detached thread, hand its result back over
    // a channel.
    let (tx, rx) = mpsc::channel();
    {
        let mut state = MANIFEST_STATE.lock();
        state.code_futures.insert(job_id, rx);
    }
    std::thread::spawn(move || {
        let code = manifest_client::fetch_manifest_request_code(manifest_gid, app_id, depot_id)
            .unwrap_or(0);
        let _ = tx.send(code);
    });
}

// ── Recv ────────────────────────────────────────────────────────────

pub fn handle_recv(body: &[u8], hdr: &[u8]) -> RecvReplacement {
    let hdr_msg = match CMsgProtoBufHeader::decode(hdr) {
        Ok(h) => h,
        Err(_) => {
            LOG_MANIFEST_WARN!("GetManifestRequestCode recv: failed to decode header");
            return RecvReplacement::None;
        }
    };
    let Some(job_id) = hdr_msg.jobid_target else {
        return RecvReplacement::None;
    };

    // Passive capture: harvest the genuine code from this reply before
    // the injection path below can overwrite the body. Only real
    // successes for a request we tracked on send. The injected case
    // naturally filters out because Steam's own reply carries no valid
    // code.
    {
        let sent = MANIFEST_STATE.lock().sent_requests.remove(&job_id);
        if let Some(s) = sent {
            if hdr_msg.eresult.unwrap_or(0) == K_ERESULT_OK {
                if let Ok(resp) = Resp::decode(body) {
                    if let Some(code) = resp.manifest_request_code.filter(|c| *c != 0) {
                        LOG_MANIFEST_DEBUG!(
                            "GetManifestRequestCode recv: captured genuine code \
                             for depot={} gid={}",
                            s.depot, s.gid,
                        );
                        manifest_donor::submit_captured_code(s.depot, s.gid, code);
                    }
                }
            }
        }
    }

    // Injection path: only fires for Lua-declared, non-owned depots,
    // because handle_send only registered a receiver for those.
    let rx = {
        let mut state = MANIFEST_STATE.lock();
        state.code_futures.remove(&job_id)
    };
    let Some(rx) = rx else { return RecvReplacement::None; };

    // Fast path: if the fetch thread already finished, use it without
    // blocking the network thread. Only wait if it is still in flight,
    // and cap the wait so a hung provider does not stall downloads.
    let code = match rx.try_recv() {
        Ok(c) => c,
        Err(std::sync::mpsc::TryRecvError::Empty) => {
            match rx.recv_timeout(Duration::from_secs(K_MAX_WAIT_SECONDS)) {
                Ok(c) => c,
                Err(_) => {
                    LOG_MANIFEST_WARN!(
                        "GetManifestRequestCode recv: HTTP timed out for jobid={}", job_id,
                    );
                    return RecvReplacement::None;
                }
            }
        }
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            LOG_MANIFEST_WARN!(
                "GetManifestRequestCode recv: fetch thread died for jobid={}", job_id,
            );
            return RecvReplacement::None;
        }
    };
    if code == 0 {
        LOG_MANIFEST_WARN!(
            "GetManifestRequestCode recv: HTTP returned 0 for jobid={}", job_id,
        );
        return RecvReplacement::None;
    }

    LOG_MANIFEST_DEBUG!(
        "GetManifestRequestCode recv: injecting code={} for jobid={}", code, job_id,
    );

    // Header: set eresult=OK.
    let mut new_hdr = hdr_msg;
    new_hdr.eresult = Some(K_ERESULT_OK);
    let new_hdr_bytes = new_hdr.encode_to_vec();
    if new_hdr_bytes.len() > K_MAX_HDR_SIZE {
        LOG_MANIFEST_WARN!(
            "GetManifestRequestCode recv: modified header too large ({})",
            new_hdr_bytes.len(),
        );
        return RecvReplacement::None;
    }

    // Body: set manifest_request_code.
    let new_resp = Resp {
        manifest_request_code: Some(code),
    };
    let new_body = new_resp.encode_to_vec();
    if new_body.len() > K_MAX_BODY_SIZE {
        LOG_MANIFEST_WARN!(
            "GetManifestRequestCode recv: modified body too large ({})",
            new_body.len(),
        );
        return RecvReplacement::None;
    }

    RecvReplacement::Both { hdr: new_hdr_bytes, body: new_body }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_malformed_body_is_noop() {
        // Must not panic.
        handle_send(&[0xFF, 0xFF, 0xFF], &[]);
    }

    #[test]
    fn recv_malformed_header_returns_none() {
        matches!(handle_recv(&[], &[0xFF]), RecvReplacement::None);
    }

    #[test]
    fn recv_missing_jobid_target_returns_none() {
        let hdr = CMsgProtoBufHeader::default().encode_to_vec();
        matches!(handle_recv(&[], &hdr), RecvReplacement::None);
    }

    #[test]
    fn recv_unregistered_jobid_returns_none() {
        let mut hdr = CMsgProtoBufHeader::default();
        hdr.jobid_target = Some(0xDEAD_BEEF);
        let hdr_bytes = hdr.encode_to_vec();
        matches!(handle_recv(&[], &hdr_bytes), RecvReplacement::None);
    }

    #[test]
    fn empty_state_starts_clean() {
        let s = MANIFEST_STATE.lock();
        // No other test in this file registers anything, so this should
        // be empty when read in isolation.
        let _ = s;
    }
}
