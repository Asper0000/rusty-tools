//! `Hooks_NetPacket_ManifestProbe`. Originates
//! `ContentServerDirectory.GetManifestRequestCode#1` rather than
//! waiting for Steam to ask, and delivers the code that comes back.
//!
//! ## Origin vs response
//!
//! Everything else in this module reacts: it rewrites frames Steam is
//! already sending, or manufactures inbound ones. This is the only path
//! that initiates, which needs three things Steam normally supplies and
//! we have to borrow:
//!
//!   * the websocket object    - only visible inside the send hook
//!   * a valid header          - `steamid` / `session` must be right,
//!                               so a real outbound header is kept as a
//!                               template rather than built from parts
//!   * a jobid                 - taken from a high, distinctive range
//!                               so it can never collide with Steam's
//!                               own
//!
//! ## Correlation
//!
//! The reply is correlated by `jobid_target` == the `jobid_source` we
//! set. A per-jobid `mpsc::Sender<u64>` parked in the global map
//! carries the code to a [`request`] caller. The `std::future` the C++
//! returns from its `Request` becomes an `mpsc::Receiver<u64>` here,
//! since Rust has no standard future whose result can be waited on
//! with a timeout from synchronous code.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;

use crate::steam::proto::{
    CContentServerDirectoryGetManifestRequestCodeRequest as Req,
    CContentServerDirectoryGetManifestRequestCodeResponse as Resp,
    CMsgProtoBufHeader,
};
use crate::steam::structs::MsgHdr;
use crate::steam::types::AppId_t;

use super::packet::K_MSG_HDR_PROTO_FLAG;
use super::state::{K_MAX_BODY_SIZE, K_MAX_HDR_SIZE};

/// The websocket send fn Steam uses for the connection. The signature
/// matches the `BBuildAndAsyncSendFrame` hook.
pub type SendFrameFn = unsafe extern "system" fn(
    *mut c_void, u32, *mut u8, u32,
) -> bool;

const K_BINARY_OPCODE: u32 = 0x02;
const K_EMSG_SERVICE_METHOD_CALL_FROM_CLIENT: u32 = 151;

/// Well clear of Steam's own job ids, which count up from small values.
const K_JOB_ID_BASE: u64 = 0x7E51_0000_0000_0000;

/// A disconnect between send and reply would otherwise leave a promise
/// nobody ever fulfils and a caller blocked on it forever. Swept on
/// each new request.
pub const K_PENDING_TIMEOUT: Duration = Duration::from_secs(30);

struct Pending {
    depot_id: u32,
    tx: Sender<u64>,
    sent_at: Instant,
}

struct ProbeState {
    /// Stored as `usize` so `ProbeState` is `Send` without an `unsafe
    /// impl`. `0` means null.
    p_websocket: usize,
    /// The trampoline address captured from the hook, stored as `usize`
    /// for the same reason. `0` means "not captured yet".
    send_frame: usize,
    hdr_template: Vec<u8>,
    next_job_id: u64,
    pending: HashMap<u64, Pending>,
}

static PROBE: Lazy<Mutex<ProbeState>> = Lazy::new(|| Mutex::new(ProbeState {
    p_websocket: 0,
    send_frame: 0,
    hdr_template: Vec::new(),
    next_job_id: K_JOB_ID_BASE,
    pending: HashMap::new(),
}));

/// Capture the connection object, the trampoline address, and (first
/// time only) a real outbound header. Called from the send hook for
/// every binary frame, and from the 151 dispatch path with an actual
/// header. `None` means "leave the captured header alone"; the first
/// non-empty `hdr` wins.
pub fn capture_context(
    p_object: *mut c_void,
    send_fn: *mut c_void,
    hdr: Option<&[u8]>,
) {
    let mut s = PROBE.lock();
    if !p_object.is_null() {
        s.p_websocket = p_object as usize;
    }
    if !send_fn.is_null() {
        s.send_frame = send_fn as usize;
    }
    if s.hdr_template.is_empty() {
        if let Some(h) = hdr {
            if !h.is_empty() {
                s.hdr_template = h.to_vec();
            }
        }
    }
}

fn expire_stale(s: &mut ProbeState) {
    let now = Instant::now();
    let stale: Vec<u64> = s
        .pending
        .iter()
        .filter(|(_, p)| now.duration_since(p.sent_at) > K_PENDING_TIMEOUT)
        .map(|(k, _)| *k)
        .collect();
    for job_id in stale {
        if let Some(p) = s.pending.remove(&job_id) {
            LOG_MANIFEST_WARN!(
                "ManifestProbe: no reply for depot {} (jobid={}) after {}s",
                p.depot_id, job_id, K_PENDING_TIMEOUT.as_secs(),
            );
            let _ = p.tx.send(0);
        }
    }
}

/// Sends the request and hands back a receiver for the code. A receiver
/// that yields `0` means the request failed, was refused by Steam, or
/// timed out. Callers treat all three the same way.
pub fn request(app_id: AppId_t, depot_id: u32, manifest_gid: u64) -> mpsc::Receiver<u64> {
    let (tx, rx) = mpsc::channel();

    // Every early exit sends 0 rather than dropping the sender, so a
    // caller waiting on the receiver is never left hanging on a request
    // that was never sent.
    let (hdr_template, ws, send, job_id) = {
        let mut s = PROBE.lock();
        expire_stale(&mut s);

        if s.p_websocket == 0 || s.send_frame == 0 || s.hdr_template.is_empty() {
            LOG_MANIFEST_WARN!(
                "ManifestProbe: no send context captured yet (websocket={}, \
                 sendFn={}, header={} bytes) - is Steam logged in?",
                s.p_websocket != 0,
                s.send_frame != 0,
                s.hdr_template.len(),
            );
            let _ = tx.send(0);
            return rx;
        }

        let job_id = s.next_job_id.wrapping_add(1);
        s.next_job_id = job_id;
        (s.hdr_template.clone(), s.p_websocket, s.send_frame, job_id)
    };

    // Modify the captured header template: set target_job_name, set
    // jobid_source, clear jobid_target.
    let mut hdr = match CMsgProtoBufHeader::decode(&*hdr_template) {
        Ok(h) => h,
        Err(_) => {
            LOG_MANIFEST_WARN!("ManifestProbe: header template failed to parse");
            let _ = tx.send(0);
            return rx;
        }
    };
    hdr.target_job_name = Some("ContentServerDirectory.GetManifestRequestCode#1".to_string());
    hdr.jobid_source = Some(job_id);
    hdr.jobid_target = None;

    let req = Req {
        app_id: Some(app_id),
        depot_id: Some(depot_id),
        manifest_id: Some(manifest_gid),
        ..Default::default()
    };

    let hdr_bytes = hdr.encode_to_vec();
    let body_bytes = req.encode_to_vec();
    if hdr_bytes.len() > K_MAX_HDR_SIZE || body_bytes.len() > K_MAX_BODY_SIZE {
        LOG_MANIFEST_WARN!(
            "ManifestProbe: frame too large (hdr={}, body={})",
            hdr_bytes.len(), body_bytes.len(),
        );
        let _ = tx.send(0);
        return rx;
    }

    let frame_size = std::mem::size_of::<MsgHdr>() + hdr_bytes.len() + body_bytes.len();
    let mut frame = vec![0u8; frame_size];
    let raw_emsg = K_EMSG_SERVICE_METHOD_CALL_FROM_CLIENT | K_MSG_HDR_PROTO_FLAG;
    frame[0..4].copy_from_slice(&raw_emsg.to_le_bytes());
    frame[4..8].copy_from_slice(&(hdr_bytes.len() as u32).to_le_bytes());
    frame[8..8 + hdr_bytes.len()].copy_from_slice(&hdr_bytes);
    frame[8 + hdr_bytes.len()..].copy_from_slice(&body_bytes);

    // Register the pending entry before sending: the reply can arrive on
    // another thread the instant the frame goes out, and it must find
    // the entry already there.
    {
        let mut s = PROBE.lock();
        s.pending.insert(job_id, Pending {
            depot_id,
            tx: tx.clone(),
            sent_at: Instant::now(),
        });
    }

    LOG_MANIFEST_INFO!(
        "ManifestProbe send: app={} depot={} gid={} jobid={} ({} bytes)",
        app_id, depot_id, manifest_gid, job_id, frame.len(),
    );

    let send_fn: SendFrameFn = unsafe { std::mem::transmute(send) };
    let ws_ptr = ws as *mut c_void;
    let sent = unsafe {
        send_fn(ws_ptr, K_BINARY_OPCODE, frame.as_mut_ptr(), frame.len() as u32)
    };
    if !sent {
        LOG_MANIFEST_WARN!("ManifestProbe: send returned false for jobid={}", job_id);
        let mut s = PROBE.lock();
        if let Some(p) = s.pending.remove(&job_id) {
            let _ = p.tx.send(0);
        }
    }

    rx
}

/// Returns `true` when the reply belonged to us; the normal Manifest
/// handler should then skip it.
pub fn handle_recv(body: &[u8], hdr: &[u8]) -> bool {
    let hdr_msg = match CMsgProtoBufHeader::decode(hdr) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let Some(job_id) = hdr_msg.jobid_target else { return false; };
    if job_id < K_JOB_ID_BASE { return false; }

    let (depot_id, tx) = {
        let mut s = PROBE.lock();
        match s.pending.remove(&job_id) {
            None => return false,
            Some(p) => (p.depot_id, p.tx),
        }
    };

    let code = Resp::decode(body)
        .ok()
        .and_then(|r| r.manifest_request_code)
        .unwrap_or(0);
    let eresult = hdr_msg.eresult.unwrap_or(0);

    if code != 0 {
        LOG_MANIFEST_INFO!(
            "ManifestProbe recv: depot={} -> code={} (eresult={})",
            depot_id, code, eresult,
        );
    } else {
        LOG_MANIFEST_WARN!(
            "ManifestProbe recv: depot={} -> NO CODE (eresult={})",
            depot_id, eresult,
        );
    }
    let _ = tx.send(code);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_recv_malformed_header_returns_false() {
        assert!(!handle_recv(&[], &[0xFF, 0xFF]));
    }

    #[test]
    fn handle_recv_header_without_jobid_target_returns_false() {
        let hdr = CMsgProtoBufHeader::default();
        let hdr_bytes = hdr.encode_to_vec();
        assert!(!handle_recv(&[], &hdr_bytes));
    }

    #[test]
    fn handle_recv_low_jobid_returns_false() {
        // A header with a jobid_target well below K_JOB_ID_BASE must not
        // match any pending entry.
        let mut hdr = CMsgProtoBufHeader::default();
        hdr.jobid_target = Some(0x1234);
        let hdr_bytes = hdr.encode_to_vec();
        assert!(!handle_recv(&[], &hdr_bytes));
    }

    #[test]
    fn job_id_base_is_high() {
        // Sanity: K_JOB_ID_BASE must be well outside the range Steam
        // uses for its own job ids.
        assert!(K_JOB_ID_BASE > 0x7E00_0000_0000_0000);
    }
}
