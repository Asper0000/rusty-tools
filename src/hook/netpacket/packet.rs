//! Packet parsing, layout detection, and the replacement helper.

use std::ffi::c_void;

use prost::Message;

use crate::platform::memory;
use crate::steam::enums::EMsg;
use crate::steam::net_packet as netpkt;
use crate::steam::proto::CMsgProtoBufHeader;
use crate::steam::structs::{CNetPacket, MsgHdr};

use super::state::{self, K_MAX_PACKET_SIZE, K_PACKET_POOL_SIZE};

pub const K_MSG_HDR_PROTO_FLAG: u32 = 0x8000_0000;
pub const K_PROBE_MAX_PACKET: u32 = 1 << 20;
pub const K_PROBE_MAX_HDR_LEN: u32 = 8192;
pub const K_PROBE_MIN_PTR: usize = 0x10000;
pub const K_PROBE_MAX_PTR: usize = 0x7FFF_FFFF_0000;
pub const K_PROBE_MAX_ATTEMPTS: i32 = 512;
pub const K_UNRESOLVED: u32 = 0;

// ── Emitted-eMsg constants (mirror the C++ case labels) ────────────

pub const K_EMSG_SERVICE_METHOD_CALL_FROM_CLIENT: u32 = EMsg::ServiceMethodCallFromClient.0;
pub const K_EMSG_SERVICE_METHOD_RESPONSE:         u32 = EMsg::ServiceMethodResponse.0;
pub const K_EMSG_CLIENT_PICS_PRODUCT_INFO_REQUEST: u32 = EMsg::ClientPICSProductInfoRequest.0;
pub const K_EMSG_CLIENT_GAMES_PLAYED:             u32 = EMsg::ClientGamesPlayed.0;
pub const K_EMSG_CLIENT_GAMES_PLAYED_WITH_DATA_BLOB: u32 = EMsg::ClientGamesPlayedWithDataBlob.0;
pub const K_EMSG_CLIENT_RICH_PRESENCE_UPLOAD:     u32 = EMsg::ClientRichPresenceUpload.0;
pub const K_EMSG_CLIENT_GET_USER_STATS:           u32 = EMsg::ClientGetUserStats.0;
pub const K_EMSG_CLIENT_STORE_USER_STATS2:        u32 = EMsg::ClientStoreUserStats2.0;
pub const K_EMSG_CLIENT_GET_USER_STATS_RESPONSE:  u32 = EMsg::ClientGetUserStatsResponse.0;
pub const K_EMSG_CLIENT_PERSONA_STATE:            u32 = EMsg::ClientPersonaState.0;
pub const K_EMSG_CLIENT_SHARED_LIBRARY_STOP_PLAYING: u32 = EMsg::ClientSharedLibraryStopPlaying.0;
pub const K_EMSG_CLIENT_LICENSE_LIST:             u32 = EMsg::ClientLicenseList.0;
pub const K_EMSG_MULTI:                           u32 = EMsg::Multi.0;
pub const K_EMSG_CLIENT_GET_LEGACY_GAME_KEY:      u32 = 730;

// ── Unpack ──────────────────────────────────────────────────────────

pub struct RawPacket<'a> {
    pub e_msg: u32,
    pub hdr: &'a [u8],
    pub body: &'a [u8],
}

/// Split a serialized frame into its eMsg, proto header, and body. The
/// bounds test subtracts rather than adds — `sizeof(MsgHdr) + cbHdr`
/// wraps for a `cbHdr` near `UINT32_MAX`, which would put `pBody` behind
/// `data` with a ~4 GB body length.
pub fn unpack_raw(data: &[u8]) -> Option<RawPacket<'_>> {
    if data.len() < std::mem::size_of::<MsgHdr>() {
        return None;
    }
    let raw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if raw & K_MSG_HDR_PROTO_FLAG == 0 { return None; }
    let hdr_len = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;
    if hdr_len > data.len() - std::mem::size_of::<MsgHdr>() { return None; }
    let off = std::mem::size_of::<MsgHdr>() + hdr_len;
    Some(RawPacket {
        e_msg: raw & !K_MSG_HDR_PROTO_FLAG,
        hdr: &data[std::mem::size_of::<MsgHdr>()..off],
        body: &data[off..],
    })
}

// ── Layout probe ────────────────────────────────────────────────────

pub unsafe fn probe_layout(base: *const c_void, data_off: u32) -> bool {
    let p = base as *const u8;
    if !memory::is_readable(p.add(data_off as usize) as *const c_void, 0x10) {
        return false;
    }
    let ptr  = *(p.add(data_off as usize) as *const *const u8);
    let size = *(p.add(data_off as usize + 8) as *const u32);
    let c_ref = *(p.add(data_off as usize + 12) as *const i32);

    let addr = ptr as usize;
    if addr < K_PROBE_MIN_PTR || addr >= K_PROBE_MAX_PTR { return false; }
    if size < std::mem::size_of::<MsgHdr>() as u32 || size > K_PROBE_MAX_PACKET { return false; }
    if c_ref < 1 || c_ref > 4096 { return false; }

    if !memory::is_readable(ptr as *const c_void, std::mem::size_of::<MsgHdr>()) { return false; }

    let raw = *(ptr as *const u32);
    let hdr_len = *(ptr.add(4) as *const u32);
    if raw & K_MSG_HDR_PROTO_FLAG == 0 { return false; }
    let e_msg = raw & !K_MSG_HDR_PROTO_FLAG;
    if e_msg == 0 || e_msg >= 0x10000 { return false; }
    let max_hdr = (size - std::mem::size_of::<MsgHdr>() as u32).min(K_PROBE_MAX_HDR_LEN);
    if hdr_len < 2 || hdr_len > max_hdr { return false; }

    if !memory::is_readable(
        ptr as *const c_void,
        std::mem::size_of::<MsgHdr>() + hdr_len as usize,
    ) { return false; }

    let hdr_bytes = std::slice::from_raw_parts(
        ptr.add(std::mem::size_of::<MsgHdr>()),
        hdr_len as usize,
    );
    CMsgProtoBufHeader::decode(hdr_bytes).is_ok()
}

/// Identify the layout from one packet. Latches only when exactly one
/// candidate matches on two consecutive packets. Ambiguity always resets
/// the standing agreement.
pub unsafe fn try_resolve_layout(p_packet: *const CNetPacket) -> bool {
    if netpkt::is_disabled() { return false; }
    let st = state::state();

    st.probe_attempts += 1;
    if st.probe_attempts > K_PROBE_MAX_ATTEMPTS {
        if !st.probe_logged {
            st.probe_logged = true;
            netpkt::disable();
            LOG_NETPACKET_ERROR!(
                "CNetPacket layout unidentified after {} packets - netpacket features \
                 disabled for this session (no field will be touched).",
                K_PROBE_MAX_ATTEMPTS,
            );
        }
        return false;
    }

    let mut winner: u32 = K_UNRESOLVED;
    let mut passes = 0;
    for layout in netpkt::K_LAYOUTS {
        if probe_layout(p_packet as *const c_void, layout.data_off) {
            passes += 1;
            winner = layout.data_off;
        }
    }

    if passes == 0 {
        LOG_NETPACKET_TRACE!(
            "CNetPacket probe: no candidate matched (attempt {}), likely a non-proto frame",
            st.probe_attempts,
        );
        return false;
    }
    if passes > 1 {
        LOG_NETPACKET_TRACE!(
            "CNetPacket probe: {} candidates matched, ambiguous (attempt {})",
            passes, st.probe_attempts,
        );
        st.probe_agreed = K_UNRESOLVED;
        return false;
    }
    if st.probe_agreed != winner {
        LOG_NETPACKET_TRACE!(
            "CNetPacket probe: candidate 0x{:X} matched, awaiting confirmation", winner,
        );
        st.probe_agreed = winner;
        return false;
    }

    let mut name = "?";
    for l in netpkt::K_LAYOUTS {
        if l.data_off == winner { name = l.name; }
    }
    netpkt::latch(winner);
    LOG_NETPACKET_INFO!(
        "CNetPacket layout = {} (m_pubData +0x{:X}, m_cubData +0x{:X}), confirmed on \
         two consecutive packets after {} attempt(s)",
        name, winner, winner + 8, st.probe_attempts,
    );
    true
}

// ── Replacement helper ──────────────────────────────────────────────

/// Build a new frame from the original eMsg word, an optional new
/// header, and an optional new body, write it into the recv ring pool,
/// and point the packet at the pool buffer. Passing `None` for either
/// keeps the corresponding part of the original.
///
/// # Safety
/// `p` must be a live `CNetPacket*` and its layout must be resolved.
pub unsafe fn replace_recv_packet(
    p: *mut CNetPacket,
    orig_frame: &[u8],
    new_hdr: Option<&[u8]>,
    new_body: Option<&[u8]>,
) -> Option<u32> {
    if orig_frame.len() < std::mem::size_of::<MsgHdr>() { return None; }
    let orig_hdr_len = u32::from_le_bytes([
        orig_frame[4], orig_frame[5], orig_frame[6], orig_frame[7],
    ]) as usize;
    if orig_hdr_len > orig_frame.len() - std::mem::size_of::<MsgHdr>() { return None; }
    let orig_hdr = &orig_frame[std::mem::size_of::<MsgHdr>()..std::mem::size_of::<MsgHdr>() + orig_hdr_len];
    let orig_body = &orig_frame[std::mem::size_of::<MsgHdr>() + orig_hdr_len..];

    let final_hdr = new_hdr.unwrap_or(orig_hdr);
    let final_body = new_body.unwrap_or(orig_body);

    let new_size = std::mem::size_of::<MsgHdr>() + final_hdr.len() + final_body.len();
    if new_size > K_MAX_PACKET_SIZE { return None; }

    let st = state::state();
    let idx = st.recv_packet_pool_idx;
    st.recv_packet_pool_idx = (idx + 1) % K_PACKET_POOL_SIZE;
    let buf = st.recv_packet_pool[idx].as_mut_ptr();

    // Preserve the raw eMsg (including the proto flag).
    let raw_emsg = u32::from_le_bytes([orig_frame[0], orig_frame[1], orig_frame[2], orig_frame[3]]);
    std::ptr::write_unaligned(buf as *mut u32, raw_emsg);
    std::ptr::write_unaligned(buf.add(4) as *mut u32, final_hdr.len() as u32);
    if !final_hdr.is_empty() {
        std::ptr::copy_nonoverlapping(
            final_hdr.as_ptr(), buf.add(std::mem::size_of::<MsgHdr>()), final_hdr.len(),
        );
    }
    if !final_body.is_empty() {
        std::ptr::copy_nonoverlapping(
            final_body.as_ptr(),
            buf.add(std::mem::size_of::<MsgHdr>() + final_hdr.len()),
            final_body.len(),
        );
    }

    netpkt::write_data(p as *mut u8, buf);
    netpkt::write_size(p as *mut u8, new_size as u32);
    Some(new_size as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_frame(e_msg_raw: u32, hdr: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + hdr.len() + body.len());
        out.extend_from_slice(&e_msg_raw.to_le_bytes());
        out.extend_from_slice(&(hdr.len() as u32).to_le_bytes());
        out.extend_from_slice(hdr);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn unpack_raw_rejects_short() {
        assert!(unpack_raw(&[]).is_none());
        assert!(unpack_raw(&[0u8; 7]).is_none());
    }

    #[test]
    fn unpack_raw_rejects_missing_flag() {
        let f = make_frame(742, &[0, 0], &[1, 2, 3]);
        assert!(unpack_raw(&f).is_none());
    }

    #[test]
    fn unpack_raw_splits_hdr_and_body() {
        let raw = K_MSG_HDR_PROTO_FLAG | 742;
        let f = make_frame(raw, &[0xAA, 0xBB, 0xCC], &[1, 2, 3, 4]);
        let p = unpack_raw(&f).unwrap();
        assert_eq!(p.e_msg, 742);
        assert_eq!(p.hdr, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(p.body, &[1, 2, 3, 4]);
    }

    #[test]
    fn unpack_raw_rejects_hdr_len_past_frame() {
        let raw = K_MSG_HDR_PROTO_FLAG | 742;
        let mut f = make_frame(raw, &[0u8; 4], &[]);
        f[4..8].copy_from_slice(&1000u32.to_le_bytes());
        assert!(unpack_raw(&f).is_none());
    }

    #[test]
    fn unpack_raw_body_empty_is_ok() {
        let raw = K_MSG_HDR_PROTO_FLAG | 100;
        let f = make_frame(raw, &[0u8; 8], &[]);
        let p = unpack_raw(&f).unwrap();
        assert_eq!(p.e_msg, 100);
        assert_eq!(p.hdr.len(), 8);
        assert!(p.body.is_empty());
    }
}
