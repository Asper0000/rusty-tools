//! `Hooks_NetPacket_RichPresence`. Outgoing `CMsgClientGamesPlayed` and
//! `CMsgClientRichPresenceUpload`; incoming `CMsgClientPersonaState`.
//!
//! Every piece of state in this module is touched only from the network
//! thread: the send hook calls `track_send_games_played` /
//! `track_rp_send`, the recv hook calls `handle_recv` and `try_inject`.
//! The C++ relies on the same single-threaded access pattern.

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;

use crate::hook::misc as hooks_misc;
use crate::steam::net_packet as netpkt;
use crate::steam::proto::{
    c_msg_client_persona_state::friend::Kv,
    CMsgClientGamesPlayed, CMsgClientPersonaState,
    CMsgClientRichPresenceUpload, CMsgProtoBufHeader,
};
use crate::steam::structs::{CNetPacket, MsgHdr};
use crate::utils::cloud_redirect::host as cloud;
use crate::utils::config::lua_config;

use super::packet::K_MSG_HDR_PROTO_FLAG;
use super::state::{K_MAX_BODY_SIZE, K_MAX_HDR_SIZE, K_MAX_PACKET_SIZE};

const K_STATUS_FLAG_RICH_PRESENCE: u32 = 0x1000;

// ── Scalars ─────────────────────────────────────────────────────────

static G_PLAYING_APP_ID: AtomicU32 = AtomicU32::new(0);
static G_LOCAL_STEAM_ID: AtomicU64 = AtomicU64::new(0);
static G_HAVE_SELF_CACHED: AtomicBool = AtomicBool::new(false);
static G_INJECT_PENDING: AtomicBool = AtomicBool::new(false);

/// Per-AppId RichPresence KVs, captured from outbound
/// `CMsgClientRichPresenceUpload`. Keyed by AppId so a multi-game stack
/// does not conflate KV state.
static G_RP_KVS_BY_APP_ID: Lazy<Mutex<HashMap<u32, Vec<(String, String)>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Cached buffers ──────────────────────────────────────────────────

struct RpBuffers {
    self_hdr: [u8; K_MAX_HDR_SIZE],
    cb_self_hdr: u32,
    self_body: [u8; K_MAX_BODY_SIZE],
    cb_self_body: u32,
    inject_pkt: [u8; K_MAX_PACKET_SIZE],
    cb_inject_pkt: u32,
}

impl RpBuffers {
    const fn new() -> Self {
        Self {
            self_hdr: [0u8; K_MAX_HDR_SIZE],
            cb_self_hdr: 0,
            self_body: [0u8; K_MAX_BODY_SIZE],
            cb_self_body: 0,
            inject_pkt: [0u8; K_MAX_PACKET_SIZE],
            cb_inject_pkt: 0,
        }
    }
}

struct RpGlobal(UnsafeCell<RpBuffers>);
unsafe impl Sync for RpGlobal {}
static RP_BUFFERS: RpGlobal = RpGlobal(UnsafeCell::new(RpBuffers::new()));

/// # Safety
/// Caller must be on the network thread. Same contract as
/// `netpacket::state::state`.
#[inline]
unsafe fn buffers() -> &'static mut RpBuffers {
    &mut *RP_BUFFERS.0.get()
}

// ── Read-only public accessors ─────────────────────────────────────

pub fn playing_app_id() -> u32 {
    G_PLAYING_APP_ID.load(Ordering::Acquire)
}

pub fn local_steam_id() -> u64 {
    G_LOCAL_STEAM_ID.load(Ordering::Acquire)
}

pub fn is_inject_pending() -> bool {
    G_INJECT_PENDING.load(Ordering::Acquire)
}

// ── KV1 stream walker ───────────────────────────────────────────────
//
// Binary KV1 format:
//   0x00  start struct   NUL-terminated name follows
//   0x01  string KV      NUL-terminated key + NUL-terminated value
//   0x08  end struct
// Anything else terminates the walk.

fn extract_string_kvs(data: &[u8], out: &mut Vec<(String, String)>) {
    let mut pos: usize = 0;
    let mut depth: i32 = 0;

    fn read_cstr(data: &[u8], pos: &mut usize) -> Option<String> {
        let start = *pos;
        while *pos < data.len() && data[*pos] != 0 { *pos += 1; }
        if *pos >= data.len() { return None; }
        let s = String::from_utf8_lossy(&data[start..*pos]).into_owned();
        *pos += 1;
        Some(s)
    }

    while pos < data.len() {
        let ty = data[pos];
        pos += 1;
        match ty {
            0x08 => {
                if depth > 0 { depth -= 1; continue; }
                break;
            }
            0x00 => {
                let Some(_name) = read_cstr(data, &mut pos) else { return; };
                depth += 1;
            }
            0x01 => {
                let Some(key) = read_cstr(data, &mut pos) else { return; };
                let Some(value) = read_cstr(data, &mut pos) else { return; };
                out.push((key, value));
            }
            _ => return,
        }
    }
}

// ── ApplyGameFields ─────────────────────────────────────────────────

fn apply_game_fields(msg: &mut CMsgClientPersonaState, entry_idx: usize, app_id: u32) {
    let current_status = msg.status_flags.unwrap_or(0);

    if app_id != 0 {
        let kvs_for_app: Option<Vec<(String, String)>> = {
            let kvs = G_RP_KVS_BY_APP_ID.lock();
            kvs.get(&app_id).cloned()
        };
        let has_kvs = kvs_for_app.as_ref().map_or(false, |v| !v.is_empty());

        let name = hooks_misc::get_game_name_by_app_id(app_id);

        let entry = &mut msg.friends[entry_idx];
        entry.game_played_app_id = Some(app_id);
        entry.gameid = Some(app_id as u64);
        if !name.is_empty() { entry.game_name = Some(name); }
        entry.rich_presence.clear();
        if let Some(list) = kvs_for_app {
            for (k, v) in list {
                entry.rich_presence.push(Kv {
                    key: Some(k),
                    value: Some(v),
                });
            }
        }

        msg.status_flags = Some(if has_kvs {
            current_status | K_STATUS_FLAG_RICH_PRESENCE
        } else {
            current_status & !K_STATUS_FLAG_RICH_PRESENCE
        });
    } else {
        let entry = &mut msg.friends[entry_idx];
        entry.game_played_app_id = None;
        entry.gameid = None;
        entry.game_name = None;
        entry.rich_presence.clear();
        msg.status_flags = Some(current_status | K_STATUS_FLAG_RICH_PRESENCE);
    }
}

// ── BuildInject ─────────────────────────────────────────────────────

fn build_inject(app_id: u32) -> bool {
    if !G_HAVE_SELF_CACHED.load(Ordering::Acquire) { return false; }

    let (hdr_bytes, body_bytes) = unsafe {
        let b = buffers();
        (b.self_hdr[..b.cb_self_hdr as usize].to_vec(),
         b.self_body[..b.cb_self_body as usize].to_vec())
    };
    if body_bytes.is_empty() { return false; }

    let mut msg = match CMsgClientPersonaState::decode(&*body_bytes) {
        Ok(m) => m,
        Err(_) => return false,
    };

    let local_id = local_steam_id();
    let Some(entry_idx) = msg.friends.iter().position(|f|
        f.friendid.map_or(false, |id| id == local_id)) else {
        return false;
    };

    apply_game_fields(&mut msg, entry_idx, app_id);

    let new_body = msg.encode_to_vec();
    let total = std::mem::size_of::<MsgHdr>() + hdr_bytes.len() + new_body.len();
    if total > K_MAX_PACKET_SIZE || new_body.len() > K_MAX_BODY_SIZE {
        LOG_RICHPRESENCE_WARN!("Inject packet too large ({} bytes)", total);
        return false;
    }

    unsafe {
        let b = buffers();
        let base = b.inject_pkt.as_mut_ptr();
        let raw = super::packet::K_EMSG_CLIENT_PERSONA_STATE | K_MSG_HDR_PROTO_FLAG;
        std::ptr::write_unaligned(base as *mut u32, raw);
        std::ptr::write_unaligned(base.add(4) as *mut u32, hdr_bytes.len() as u32);
        let mut off = std::mem::size_of::<MsgHdr>();
        if !hdr_bytes.is_empty() {
            std::ptr::copy_nonoverlapping(hdr_bytes.as_ptr(), base.add(off), hdr_bytes.len());
            off += hdr_bytes.len();
        }
        if !new_body.is_empty() {
            std::ptr::copy_nonoverlapping(new_body.as_ptr(), base.add(off), new_body.len());
        }
        b.cb_inject_pkt = total as u32;
    }

    LOG_RICHPRESENCE_INFO!("Built inject for appid {} ({} bytes)", app_id, total);
    true
}

// ── TryInject ───────────────────────────────────────────────────────

/// Deliver the pending manufactured PersonaState by borrowing the
/// carrier's data pointer for one call to the original, then restoring.
///
/// # Safety
/// Caller must be inside the `RecvPkt` hook with a live `p_carrier`.
pub unsafe fn try_inject<F>(
    p_this: *mut c_void,
    p_carrier: *mut CNetPacket,
    invoke_original: F,
) where
    F: FnOnce(*mut c_void, *mut CNetPacket) -> *mut c_void,
{
    if !G_INJECT_PENDING.load(Ordering::Acquire) { return; }
    let b = buffers();
    if b.cb_inject_pkt == 0 { return; }

    G_INJECT_PENDING.store(false, Ordering::Release);

    let orig_data = netpkt::read_data(p_carrier as *mut u8);
    let orig_size = netpkt::read_size(p_carrier as *mut u8);

    let buf_ptr = b.inject_pkt.as_mut_ptr();
    netpkt::write_data(p_carrier as *mut u8, buf_ptr);
    netpkt::write_size(p_carrier as *mut u8, b.cb_inject_pkt);

    let _ = invoke_original(p_this, p_carrier);

    netpkt::write_data(p_carrier as *mut u8, orig_data);
    netpkt::write_size(p_carrier as *mut u8, orig_size);

    LOG_RICHPRESENCE_INFO!(
        "Delivered manufactured self-PersonaState ({} bytes)", b.cb_inject_pkt,
    );
}

// ── TrackRPSend ─────────────────────────────────────────────────────

pub fn track_rp_send(body: &[u8]) {
    let local_id = local_steam_id();
    let playing = playing_app_id();
    if local_id == 0 || playing == 0 { return; }

    let up = match CMsgClientRichPresenceUpload::decode(body) {
        Ok(u) => u,
        Err(_) => return,
    };
    let Some(kv_bytes) = up.rich_presence_kv.as_ref() else { return; };

    let mut kvs: Vec<(String, String)> = Vec::new();
    extract_string_kvs(kv_bytes, &mut kvs);
    LOG_RICHPRESENCE_DEBUG!(
        "RP upload appid={}: kv_bytes={} extracted={} pairs",
        playing, kv_bytes.len(), kvs.len(),
    );

    G_RP_KVS_BY_APP_ID.lock().insert(playing, kvs);

    if build_inject(playing) {
        G_INJECT_PENDING.store(true, Ordering::Release);
    }
}

// ── TrackSend ───────────────────────────────────────────────────────

pub fn track_send_games_played(body: &[u8], hdr: &[u8]) {
    let msg = match CMsgClientGamesPlayed::decode(body) {
        Ok(m) => m,
        Err(_) => return,
    };

    let mut local_id = local_steam_id();
    if local_id == 0 {
        if let Ok(hdr_msg) = CMsgProtoBufHeader::decode(hdr) {
            if let Some(id) = hdr_msg.steamid {
                if id != 0 {
                    G_LOCAL_STEAM_ID.store(id, Ordering::Release);
                    local_id = id;
                    LOG_RICHPRESENCE_DEBUG!("Captured local SteamID 0x{:X}", id);
                    cloud::set_account_id(id as u32);
                }
            }
        }
    }
    let _ = local_id;

    let topmost = if msg.games_played.is_empty() {
        0u32
    } else {
        let last = msg.games_played.last().unwrap();
        (last.game_id.unwrap_or(0) & 0xFFFF_FFFF) as u32
    };

    let k_onlinefix: u32 = crate::dllmain::K_ONLINE_FIX_APPID;
    let new_tracked = if topmost != 0
        && topmost != k_onlinefix
        && lua_config::has_depot(topmost, false)
    {
        topmost
    } else {
        0
    };

    let old_tracked = playing_app_id();
    if old_tracked == new_tracked { return; }
    G_PLAYING_APP_ID.store(new_tracked, Ordering::Release);

    if old_tracked != 0 { cloud::notify_app_running(old_tracked, false); }
    if new_tracked != 0 { cloud::notify_app_running(new_tracked, true); }

    if new_tracked != 0 {
        LOG_RICHPRESENCE_INFO!("Tracking topmost appid {}", new_tracked);
        if build_inject(new_tracked) {
            G_INJECT_PENDING.store(true, Ordering::Release);
        }
    } else if topmost == 0 {
        LOG_RICHPRESENCE_DEBUG!("GamesPlayed empty, scheduling cache clear");
        if build_inject(0) {
            G_INJECT_PENDING.store(true, Ordering::Release);
        }
    } else {
        LOG_RICHPRESENCE_DEBUG!(
            "Topmost is appid {} (owned or onlinefix); deferring to server", topmost,
        );
    }
}

// ── HandleRecv ──────────────────────────────────────────────────────

pub fn handle_recv(body: &[u8], hdr: &[u8]) -> Option<Vec<u8>> {
    let mut msg = match CMsgClientPersonaState::decode(body) {
        Ok(m) => m,
        Err(_) => return None,
    };

    let local_id = local_steam_id();
    let Some(entry_idx) = msg.friends.iter().position(|f|
        f.friendid.map_or(false, |id| id == local_id)) else {
        return None;
    };

    LOG_RICHPRESENCE_DEBUG!(
        "Recv self PersonaState: status_flags=0x{:X} friends_size={}",
        msg.status_flags.unwrap_or(0), msg.friends.len(),
    );

    // Cache the original bytes (pre-mutation) as the inject template.
    if hdr.len() <= K_MAX_HDR_SIZE && body.len() <= K_MAX_BODY_SIZE {
        unsafe {
            let b = buffers();
            b.self_hdr[..hdr.len()].copy_from_slice(hdr);
            b.cb_self_hdr = hdr.len() as u32;
            b.self_body[..body.len()].copy_from_slice(body);
            b.cb_self_body = body.len() as u32;
        }
        G_HAVE_SELF_CACHED.store(true, Ordering::Release);
    }

    let app_id = playing_app_id();
    if app_id == 0 { return None; }

    apply_game_fields(&mut msg, entry_idx, app_id);
    let new_body = msg.encode_to_vec();
    if new_body.len() > K_MAX_BODY_SIZE {
        LOG_RICHPRESENCE_WARN!("In-place patch too large ({} bytes)", new_body.len());
        return None;
    }
    LOG_RICHPRESENCE_INFO!("Patched live self push with appid {}", app_id);
    Some(new_body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_zero() {
        assert_eq!(playing_app_id(), 0);
        assert_eq!(local_steam_id(), 0);
        assert!(!is_inject_pending());
    }

    #[test]
    fn extract_string_kvs_empty_input_is_noop() {
        let mut out = Vec::new();
        extract_string_kvs(&[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn extract_string_kvs_flat_struct() {
        // 0x00 "RP\0" then 0x01 "key\0" "val\0" then 0x08.
        let mut data = Vec::new();
        data.push(0x00);
        data.extend_from_slice(b"RP\0");
        data.push(0x01);
        data.extend_from_slice(b"key\0val\0");
        data.push(0x08);
        let mut out = Vec::new();
        extract_string_kvs(&data, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], ("key".to_string(), "val".to_string()));
    }

    #[test]
    fn extract_string_kvs_unknown_type_terminates() {
        let data = [0x00, b'R', b'P', 0x00, 0xFF];
        let mut out = Vec::new();
        extract_string_kvs(&data, &mut out);
        assert!(out.is_empty());
    }
}
