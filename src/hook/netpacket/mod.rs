//! `Hooks_NetPacket.cpp`. Hooks `BBuildAndAsyncSendFrame` (outgoing) and
//! `RecvPkt` (incoming) and dispatches every packet by `EMsg`.

use std::ffi::{c_void, CStr};

use crate::steam::net_packet as netpkt;
use crate::steam::structs::{CNetPacket, ExtendedMsgHdr, MsgHdr};
use crate::{hook_fn, install_hook_c, resolve_c, resolve_fn, resolve_ready, uninstall_hook};

pub mod state;
pub mod packet;
pub mod dispatch;

pub mod access_token;
pub mod family_sharing;
pub mod licenses;
pub mod onlinefix;
pub mod richpresence;
pub mod userstats;

pub mod manifest_probe;

pub mod manifest;
pub mod legacy_key;

pub use dispatch::{RecvReplacement, SendReplacement};

// ── Resolve ─────────────────────────────────────────────────────────

resolve_fn! {
    fn PchMsgNameFromEMsg(e_msg: u32) -> *const i8
}

/// Human-readable name for an `EMsg`, or `"?"` when the resolver has not
/// fired or the lookup fails. The returned pointer is to a static string
/// owned by steamclient, so the borrow is effectively `'static`.
pub fn msg_name(e_msg: u32) -> &'static str {
    if !resolve_ready!(PchMsgNameFromEMsg) { return "?"; }
    let p = unsafe { o_PchMsgNameFromEMsg(e_msg) };
    if p.is_null() { return "?"; }
    let s = unsafe { CStr::from_ptr(p) };
    match s.to_str() {
        Ok(s) => unsafe { std::mem::transmute::<&str, &'static str>(s) },
        Err(_) => "?",
    }
}

// ── Top-level hooks ─────────────────────────────────────────────────

hook_fn! {
    fn BBuildAndAsyncSendFrame(
        p_object: *mut c_void,
        op_code: u32,
        pub_data: *mut u8,
        cub_data: u32,
    ) -> bool {
        const K_BINARY: u32 = 0x02; // k_eWebSocketOpCode_Binary
        if op_code != K_BINARY {
            return o_BBuildAndAsyncSendFrame(p_object, op_code, pub_data, cub_data);
        }

        let size = cub_data as usize;
        if pub_data.is_null() || size < std::mem::size_of::<MsgHdr>() {
            return o_BBuildAndAsyncSendFrame(p_object, op_code, pub_data, cub_data);
        }

        // Capture the connection object and the trampoline address for
        // ManifestProbe. Called on every binary frame so a reconnect
        // updates the captured values.
        let send_fn = unsafe { O_B_BUILD_AND_ASYNC_SEND_FRAME.load() };
        manifest_probe::capture_context(p_object, send_fn, None);

        let frame: Vec<u8> = unsafe { std::slice::from_raw_parts(pub_data, size) }.to_vec();

        // Legacy CD-key request (EMsg 730) is a non-proto struct message
        // that `unpack_raw` skips. Intercept it here: if we can answer
        // locally, suppress the real send; the 785 response is delivered
        // from the RecvPkt hook. Deliberately gated on layout resolution
        // - the drain is inert until then, so a locally-answered request
        // before that would leave the caller waiting on a reply that
        // never arrives.
        if size >= std::mem::size_of::<ExtendedMsgHdr>() && netpkt::is_resolved() {
            let raw = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
            if raw & packet::K_MSG_HDR_PROTO_FLAG == 0
                && raw == packet::K_EMSG_CLIENT_GET_LEGACY_GAME_KEY
                && legacy_key::handle_send(&frame)
            {
                return true; // answered locally; report success so Steam treats it as sent
            }
        }

        let rep = unsafe { dispatch::send_frame(&frame) };

        match rep {
            SendReplacement::None => {
                o_BBuildAndAsyncSendFrame(p_object, op_code, pub_data, cub_data)
            }
            SendReplacement::Suppress => {
                // Report success so Steam treats the frame as sent, but
                // do not forward it.
                true
            }
            SendReplacement::Body(new_body) => {
                let orig_hdr_len = u32::from_le_bytes([
                    frame[4], frame[5], frame[6], frame[7],
                ]) as usize;
                let body_start = std::mem::size_of::<MsgHdr>() + orig_hdr_len;
                if body_start > frame.len() {
                    return o_BBuildAndAsyncSendFrame(p_object, op_code, pub_data, cub_data);
                }
                let new_size = body_start + new_body.len();
                let mut out = vec![0u8; new_size];
                out[..body_start].copy_from_slice(&frame[..body_start]);
                out[body_start..].copy_from_slice(&new_body);
                o_BBuildAndAsyncSendFrame(
                    p_object, op_code, out.as_mut_ptr(), new_size as u32,
                )
            }
        }
    }
}

hook_fn! {
    fn RecvPkt(p_this: *mut c_void, p_packet: *mut CNetPacket) -> *mut c_void {
        if p_packet.is_null() {
            return o_RecvPkt(p_this, p_packet);
        }

        let resolved = unsafe {
            netpkt::is_resolved() || packet::try_resolve_layout(p_packet)
        };
        if !resolved {
            return o_RecvPkt(p_this, p_packet);
        }

        // Delivery order matches the C++: resolve layout, TryInject,
        // drain Cloud, drain LegacyKey, then dispatch. Each of the three
        // swaps the carrier's data pointer for the duration of one call
        // to the original, then restores.
        if richpresence::is_inject_pending() {
            unsafe {
                richpresence::try_inject(p_this, p_packet, |t, p| o_RecvPkt(t, p));
            }
        }
                if legacy_key::has_pending() {
            unsafe {
                legacy_key::drain(p_this, p_packet, |t, p| o_RecvPkt(t, p));
            }
        }

        let data_ptr = unsafe { netpkt::read_data(p_packet as *mut u8) };
        let size = unsafe { netpkt::read_size(p_packet as *mut u8) } as usize;
        if data_ptr.is_null() || size < std::mem::size_of::<MsgHdr>() {
            return o_RecvPkt(p_this, p_packet);
        }
        let frame: Vec<u8> = unsafe { std::slice::from_raw_parts(data_ptr, size) }.to_vec();

        let rep = unsafe { dispatch::recv_frame(&frame) };

        match rep {
            RecvReplacement::None => o_RecvPkt(p_this, p_packet),
            RecvReplacement::Body(new_body) => {
                let _ = unsafe {
                    packet::replace_recv_packet(p_packet, &frame, None, Some(&new_body))
                };
                o_RecvPkt(p_this, p_packet)
            }
            RecvReplacement::Hdr(new_hdr) => {
                let _ = unsafe {
                    packet::replace_recv_packet(p_packet, &frame, Some(&new_hdr), None)
                };
                o_RecvPkt(p_this, p_packet)
            }
            RecvReplacement::Both { hdr, body } => {
                let _ = unsafe {
                    packet::replace_recv_packet(p_packet, &frame, Some(&hdr), Some(&body))
                };
                o_RecvPkt(p_this, p_packet)
            }
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────

pub fn install() -> bool {
    resolve_c!(PchMsgNameFromEMsg);

    let ok1 = install_hook_c!(BBuildAndAsyncSendFrame);
    let ok2 = install_hook_c!(RecvPkt);
    ok1 && ok2
}

pub fn uninstall() -> bool {
    let ok1 = uninstall_hook!(BBuildAndAsyncSendFrame);
    let ok2 = uninstall_hook!(RecvPkt);
    ok1 && ok2
}

/// Ask Steam for a manifest request code, as this account. Blocks the
/// caller until the reply arrives or the pending-entry timeout fires.
pub fn request_manifest_code(app_id: u32, depot_id: u32, gid: u64) -> u64 {
    request_manifest_code_with_timeout(app_id, depot_id, gid, manifest_probe::K_PENDING_TIMEOUT)
}

/// Same as [`request_manifest_code`] with an explicit timeout. The donor
/// worker uses a shorter budget so a stalled reply cannot block one
/// mint cycle for the full IPC-facing timeout per depot.
pub fn request_manifest_code_with_timeout(
    app_id: u32, depot_id: u32, gid: u64, timeout: std::time::Duration,
) -> u64 {
    let rx = manifest_probe::request(app_id, depot_id, gid);
    rx.recv_timeout(timeout).unwrap_or(0)
}

/// Manual test convenience: resolve a depot via `LookupDepot` and probe.
/// Fire-and-forget; the reply handler logs the outcome. Detached so the
/// caller is not blocked on a network round-trip.
pub fn probe_manifest(depot_id: u32) -> bool {
    let Some((app_id, gid)) = crate::hook::manifest::lookup_depot(depot_id) else {
        LOG_MANIFEST_WARN!(
            "ManifestProbe: depot {} not seen yet - it only becomes known once its \
             app is installed or updated in this session",
            depot_id,
        );
        return false;
    };
    let _ = crate::platform::thread::start_detached(move || {
        let rx = manifest_probe::request(app_id, depot_id, gid);
        let _ = rx.recv_timeout(manifest_probe::K_PENDING_TIMEOUT);
        0
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_name_before_resolve_is_question_mark() {
        assert_eq!(msg_name(0), "?");
    }
}
