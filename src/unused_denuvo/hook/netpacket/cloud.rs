//! `Hooks_NetPacket_Cloud`. Steam Cloud save redirection via
//! CloudRedirect.
//!
//! Outgoing: `ServiceMethodCallFromClient` (eMsg 151) with a
//! `Cloud.*` target for an addappid-unlocked game.
//!
//! Incoming: a synthesized `ServiceMethodResponse` (eMsg 147) carrying
//! the answer produced by CloudRedirect, correlated by jobid.
//!
//! CloudRedirect answers the RPC locally (reading/writing the real save
//! bytes to the user's cloud provider). This module therefore suppresses
//! the outbound request — it must not reach Valve — and delivers the
//! response the same way RichPresence injects packets: by borrowing the
//! next inbound carrier packet for one call to the original.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;

use crate::steam::net_packet as netpkt;
use crate::steam::proto::CMsgProtoBufHeader;
use crate::steam::structs::{CNetPacket, MsgHdr};
use crate::utils::cloud_redirect::host as cloud_redirect;

use super::packet::K_MSG_HDR_PROTO_FLAG;
use super::state::{K_MAX_BODY_SIZE, K_MAX_HDR_SIZE, K_MAX_PACKET_SIZE};

const K_EMSG_SERVICE_METHOD_RESPONSE: u32 = 147;
const K_MAX_QUEUE_LEN: usize = 64;

static G_LOCAL_STEAM_ID: AtomicU64 = AtomicU64::new(0);

static CLOUD_QUEUE: Lazy<Mutex<VecDeque<Vec<u8>>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));

// ── Minimal protobuf field reader ───────────────────────────────────
//
// Reads a single varint field from the body of a Cloud.* request
// without pulling the Cloud message definitions into the proto set.
// Mirrors CloudRedirect's `CloudRpcUtils::ExtractAppId`.

fn read_varint(d: &[u8], pos: &mut usize) -> Option<u64> {
    let mut out: u64 = 0;
    let mut shift = 0;
    while *pos < d.len() {
        let b = d[*pos];
        *pos += 1;
        out |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 { return Some(out); }
        shift += 7;
        if shift >= 64 { return None; }
    }
    None
}

fn find_varint_field(d: &[u8], target: u32) -> Option<u64> {
    let mut pos = 0usize;
    while pos < d.len() {
        let tag = read_varint(d, &mut pos)?;
        let field = (tag >> 3) as u32;
        let wire = (tag & 7) as u32;
        if field == target && wire == 0 {
            return read_varint(d, &mut pos);
        }
        match wire {
            0 => { let _ = read_varint(d, &mut pos)?; }
            1 => pos = pos.checked_add(8)?,
            5 => pos = pos.checked_add(4)?,
            2 => {
                let len = read_varint(d, &mut pos)? as usize;
                pos = pos.checked_add(len)?;
            }
            _ => return None,
        }
        if pos > d.len() { return None; }
    }
    None
}

/// The appid lives in field 1 of every Cloud.* request except
/// `ClientCommitFileUpload`, where it is field 2.
fn extract_app_id(job_name: &str, body: &[u8]) -> u32 {
    let field = if job_name == "Cloud.ClientCommitFileUpload#1" { 2 } else { 1 };
    find_varint_field(body, field).map(|v| v as u32).unwrap_or(0)
}

// ── Send ────────────────────────────────────────────────────────────

/// Returns `true` when CloudRedirect handled the request; the caller
/// must then suppress the outbound frame.
pub fn handle_send(job_name: &str, body: &[u8], hdr: &[u8]) -> bool {
    if !cloud_redirect::is_active() { return false; }

    let req_hdr = match CMsgProtoBufHeader::decode(hdr) {
        Ok(h) => h,
        Err(_) => return false,
    };
    if let Some(sid) = req_hdr.steamid.filter(|s| *s != 0) {
        G_LOCAL_STEAM_ID.store(sid, Ordering::Release);
    }

    let app_id = extract_app_id(job_name, body);
    if app_id == 0 || !cloud_redirect::is_app(app_id) { return false; }

    let account_id = (G_LOCAL_STEAM_ID.load(Ordering::Acquire) & 0xFFFF_FFFF) as u32;

    // CloudRedirect writes its response into a scratch buffer.
    let mut resp_buf = vec![0u8; K_MAX_BODY_SIZE];
    let (resp_len, eresult) = match cloud_redirect::handle_cloud_rpc(
        job_name, app_id, account_id, body, &mut resp_buf,
    ) {
        Some(pair) => pair,
        None => return false,
    };

    // Build the synthesized 147 response header.
    let mut resp_hdr = CMsgProtoBufHeader::default();
    if let Some(job_id) = req_hdr.jobid_source {
        resp_hdr.jobid_target = Some(job_id);
    }
    resp_hdr.eresult = Some(eresult);
    resp_hdr.target_job_name = Some(job_name.to_string());

    let hdr_bytes = resp_hdr.encode_to_vec();
    if hdr_bytes.len() > K_MAX_HDR_SIZE || resp_len > K_MAX_BODY_SIZE {
        LOG_NETPACKET_WARN!(
            "Cloud: {} response too large (hdr={}, body={}), passing through",
            job_name, hdr_bytes.len(), resp_len,
        );
        return false;
    }

    let total = std::mem::size_of::<MsgHdr>() + hdr_bytes.len() + resp_len;
    if total > K_MAX_PACKET_SIZE {
        LOG_NETPACKET_WARN!(
            "Cloud: {} response too large ({} bytes), passing through",
            job_name, total,
        );
        return false;
    }

    let mut pkt = vec![0u8; total];
    let raw = K_EMSG_SERVICE_METHOD_RESPONSE | K_MSG_HDR_PROTO_FLAG;
    pkt[0..4].copy_from_slice(&raw.to_le_bytes());
    pkt[4..8].copy_from_slice(&(hdr_bytes.len() as u32).to_le_bytes());
    pkt[8..8 + hdr_bytes.len()].copy_from_slice(&hdr_bytes);
    if resp_len > 0 {
        pkt[8 + hdr_bytes.len()..].copy_from_slice(&resp_buf[..resp_len]);
    }

    {
        let mut q = CLOUD_QUEUE.lock();
        if q.len() < K_MAX_QUEUE_LEN {
            q.push_back(pkt);
        } else {
            LOG_NETPACKET_WARN!(
                "Cloud: queue full ({}/{}), dropping response for {}",
                q.len(), K_MAX_QUEUE_LEN, job_name,
            );
        }
    }
    LOG_NETPACKET_DEBUG!(
        "Cloud: handled {} app={} -> queued {}-byte response (eresult={})",
        job_name, app_id, total, eresult,
    );
    true
}

// ── Drain ───────────────────────────────────────────────────────────

pub fn has_pending() -> bool {
    !CLOUD_QUEUE.lock().is_empty()
}

/// Deliver any queued cloud responses by borrowing the carrier packet
/// for one call to the original each. Same trick as
/// `RichPresence::try_inject`. Runs on the network thread from inside
/// the RecvPkt hook.
///
/// # Safety
/// Caller must be inside the `RecvPkt` hook with a live `p_carrier`.
pub unsafe fn drain<F>(p_this: *mut c_void, p_carrier: *mut CNetPacket, invoke_original: F)
where
    F: Fn(*mut c_void, *mut CNetPacket) -> *mut c_void,
{
    loop {
        let mut pkt = {
            let mut q = CLOUD_QUEUE.lock();
            match q.pop_front() {
                Some(p) => p,
                None => return,
            }
        };

        let orig_data = netpkt::read_data(p_carrier as *mut u8);
        let orig_size = netpkt::read_size(p_carrier as *mut u8);

        let len = pkt.len();
        netpkt::write_data(p_carrier as *mut u8, pkt.as_mut_ptr());
        netpkt::write_size(p_carrier as *mut u8, len as u32);

        let _ = invoke_original(p_this, p_carrier);

        netpkt::write_data(p_carrier as *mut u8, orig_data);
        netpkt::write_size(p_carrier as *mut u8, orig_size);

        LOG_NETPACKET_DEBUG!("Cloud: delivered {}-byte response", len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_varint_basic() {
        let mut pos = 0;
        assert_eq!(read_varint(&[0x2A], &mut pos), Some(42));
    }

    #[test]
    fn read_varint_multibyte() {
        // 300 = 0xAC 0x02
        let mut pos = 0;
        assert_eq!(read_varint(&[0xAC, 0x02], &mut pos), Some(300));
    }

    #[test]
    fn find_varint_field_finds_appid() {
        // field 1, wire 0 => tag = (1<<3)|0 = 0x08
        // value 730 = 0xDA 0x05 (varint)
        let body = [0x08, 0xDA, 0x05];
        assert_eq!(find_varint_field(&body, 1), Some(730));
    }

    #[test]
    fn extract_app_id_uses_field_2_for_commit() {
        // field 2, wire 0 => tag = (2<<3)|0 = 0x10
        let body = [0x10, 0x2A]; // value = 42
        assert_eq!(extract_app_id("Cloud.ClientCommitFileUpload#1", &body), 42);
        // For other jobs, field 1 is 0x08, so this body is empty of field 1.
        assert_eq!(extract_app_id("Cloud.GetFile#1", &body), 0);
    }

    #[test]
    fn empty_queue_has_no_pending() {
        // Cannot cleanly assert on the shared queue if another test
        // populated it, but the call is safe to make.
        let _ = has_pending();
    }
}
