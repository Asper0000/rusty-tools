//! Legacy third-party CD key ("Updating product key").
//!
//! `k_EMsgClientGetLegacyGameKey` (730) is a non-proto STRUCT message,
//! so it never reaches the proto SendJob/RecvJob path (`unpack_raw`
//! bails on the missing proto flag). We catch the outbound request
//! straight off the send hook, answer it locally with a resolved key,
//! suppress the real send, and deliver the synthesized 785 response
//! from the RecvPkt hook — the same "answered locally" trick the Cloud
//! path uses.

use std::collections::VecDeque;
use std::ffi::c_void;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::steam::net_packet as netpkt;
use crate::steam::structs::{CNetPacket, ExtendedMsgHdr, MsgClientGetLegacyGameKeyResponse};
use crate::steam::types::k_GIDNil;
use crate::utils::tickets::legacy_cd_key;

use super::state::K_MAX_PACKET_SIZE;

const K_EMSG_CLIENT_GET_LEGACY_GAME_KEY: u32 =
    super::packet::K_EMSG_CLIENT_GET_LEGACY_GAME_KEY;
const K_EMSG_CLIENT_GET_LEGACY_GAME_KEY_RESPONSE: u32 = 785;
const K_ERESULT_OK: i32 = 1;
const K_MAX_QUEUE_LEN: usize = 64;

static LEGACY_QUEUE: Lazy<Mutex<VecDeque<Vec<u8>>>> =
    Lazy::new(|| Mutex::new(VecDeque::new()));

/// Returns `true` when we answered the request locally; the caller must
/// then suppress the outbound frame.
pub fn handle_send(frame: &[u8]) -> bool {
    let hdr_size = std::mem::size_of::<ExtendedMsgHdr>();
    let body_size = std::mem::size_of::<MsgClientGetLegacyGameKeyResponse>();

    // Request body is a bare `u32 m_unAppId` at offset hdr_size.
    if frame.len() < hdr_size + 4 { return false; }

    // ExtendedMsgHdr field offsets (packed struct): m_ulSteamID is at
    // 24, m_JobIDSource at 15.
    let req_steamid = u64::from_le_bytes([
        frame[24], frame[25], frame[26], frame[27],
        frame[28], frame[29], frame[30], frame[31],
    ]);
    let req_job_source = u64::from_le_bytes([
        frame[15], frame[16], frame[17], frame[18],
        frame[19], frame[20], frame[21], frame[22],
    ]);

    let app_id = u32::from_le_bytes([
        frame[hdr_size], frame[hdr_size + 1],
        frame[hdr_size + 2], frame[hdr_size + 3],
    ]);
    let account_id = (req_steamid & 0xFFFF_FFFF) as u32;

    let Some(key) = legacy_cd_key::resolve(app_id, account_id) else {
        return false; // not a managed app — let Steam's real flow run
    };

    // Steam stores the legacy key as a NUL-terminated string; length
    // counts the terminator.
    let cch_key = (key.len() + 1) as u32;
    let total = hdr_size + body_size + cch_key as usize;
    if total > K_MAX_PACKET_SIZE {
        LOG_NETPACKET_WARN!(
            "LegacyKey: app={} response too large ({} bytes), passing through",
            app_id, total,
        );
        return false;
    }

    let mut pkt = vec![0u8; total];
    // Copy the whole request header, then override the three fields
    // that distinguish a response from a request.
    pkt[..hdr_size].copy_from_slice(&frame[..hdr_size]);
    // eMsg -> 785 (non-proto, flag stays clear)
    pkt[0..4].copy_from_slice(&K_EMSG_CLIENT_GET_LEGACY_GAME_KEY_RESPONSE.to_le_bytes());
    // m_JobIDTarget = request's m_JobIDSource (offset 7..15)
    pkt[7..15].copy_from_slice(&req_job_source.to_le_bytes());
    // m_JobIDSource = k_GIDNil (offset 15..23)
    pkt[15..23].copy_from_slice(&k_GIDNil.to_le_bytes());

    // Response body: u32 m_unAppId, i32 m_eResult, u32 m_cchKey, then
    // the NUL-terminated key.
    pkt[hdr_size..hdr_size + 4].copy_from_slice(&app_id.to_le_bytes());
    pkt[hdr_size + 4..hdr_size + 8].copy_from_slice(&K_ERESULT_OK.to_le_bytes());
    pkt[hdr_size + 8..hdr_size + 12].copy_from_slice(&cch_key.to_le_bytes());
    let key_off = hdr_size + body_size;
    pkt[key_off..key_off + key.len()].copy_from_slice(key.as_bytes());
    // NUL terminator already zeroed by `vec![0u8; total]`.

    {
        let mut q = LEGACY_QUEUE.lock();
        if q.len() < K_MAX_QUEUE_LEN {
            q.push_back(pkt);
        }
    }
    LOG_NETPACKET_DEBUG!(
        "LegacyKey: app={} answered locally ({} bytes, key '{}')",
        app_id, total, key,
    );
    true
}

pub fn has_pending() -> bool {
    !LEGACY_QUEUE.lock().is_empty()
}

/// Deliver queued responses by borrowing the carrier packet for one
/// call to the original each. Same shape as `cloud::drain`.
///
/// # Safety
/// Caller must be inside the `RecvPkt` hook with a live `p_carrier`.
pub unsafe fn drain<F>(p_this: *mut c_void, p_carrier: *mut CNetPacket, invoke_original: F)
where
    F: Fn(*mut c_void, *mut CNetPacket) -> *mut c_void,
{
    loop {
        let mut pkt = {
            let mut q = LEGACY_QUEUE.lock();
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

        LOG_NETPACKET_DEBUG!("LegacyKey: delivered {}-byte response", len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_frame_rejected() {
        // A frame shorter than ExtendedMsgHdr + u32 must be rejected.
        assert!(!handle_send(&[0u8; 8]));
    }

    #[test]
    fn unmanaged_app_passes_through() {
        // Build a minimal ExtendedMsgHdr + appId frame. App id is 0,
        // which is not in the (empty) Lua config.
        let hdr_size = std::mem::size_of::<ExtendedMsgHdr>();
        let mut f = vec![0u8; hdr_size + 4];
        // eMsg (offset 0..4) = 730
        f[0..4].copy_from_slice(&730u32.to_le_bytes());
        // m_JobIDSource (offset 15..23) — arbitrary
        f[15..23].copy_from_slice(&0x1122334455667788u64.to_le_bytes());
        // m_ulSteamID (offset 24..32) — arbitrary
        f[24..32].copy_from_slice(&0x1100001000000042u64.to_le_bytes());
        // appId (offset hdr_size) = 730
        f[hdr_size..hdr_size + 4].copy_from_slice(&730u32.to_le_bytes());
        assert!(!handle_send(&f));
    }

    #[test]
    fn empty_queue_has_no_pending() {
        let _ = has_pending();
    }
}
