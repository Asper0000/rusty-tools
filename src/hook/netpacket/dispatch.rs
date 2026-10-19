//! eMsg dispatch. Mirrors the C++ `SendJob` / `RecvJob` and
//! `SendServiceJob` / `RecvServiceJob` inside the anonymous namespace.

use prost::Message;

use crate::steam::proto::CMsgProtoBufHeader;
use crate::utils::cloud_redirect::host as cloud_redirect;
use crate::utils::support::fnv::fnv1a_hash;

use super::{
    access_token, family_sharing, licenses, manifest,
    manifest_probe, msg_name, onlinefix,
    packet::{self, K_MSG_HDR_PROTO_FLAG},
    richpresence, userstats,
};

// ── Outcome of a dispatch ───────────────────────────────────────────

/// Result of dispatching an outgoing frame.
pub enum SendReplacement {
    /// Leave the packet untouched.
    None,
    /// Replace the body, keep the header.
    Body(Vec<u8>),
    /// Do not forward the frame; report success to the caller.
    Suppress,
}

/// Result of dispatching an incoming frame.
pub enum RecvReplacement {
    /// Leave the packet untouched.
    None,
    /// Replace the body, keep the header.
    Body(Vec<u8>),
    /// Replace the header, keep the body.
    Hdr(Vec<u8>),
    /// Replace both.
    Both { hdr: Vec<u8>, body: Vec<u8> },
}

// ── FNV-1a hashes of the service job names dispatched on 151/147 ────

const HASH_JOB_NOTIFY_RUNNING_APPS: u32 =
    fnv1a_hash("FamilyGroupsClient.NotifyRunningApps#1");
const HASH_JOB_GET_USER_STATS: u32 =
    fnv1a_hash("Player.GetUserStats#1");
const HASH_JOB_GET_MANIFEST_REQUEST_CODE: u32 =
    fnv1a_hash("ContentServerDirectory.GetManifestRequestCode#1");

// ── Service job dispatch (151 / 147) ────────────────────────────────

fn send_service_job(target: &str, body: &[u8], hdr: &[u8]) -> Option<SendReplacement> {
    LOG_NETPACKET_DEBUG!("Send target_job_name: {}", target);

    // Steam Cloud save redirection: hand every "Cloud.*" request to
    // CloudRedirect. If it answers, suppress the outbound frame (the
    // synthesized response is delivered from the RecvPkt hook).
    //
    // ExitSyncDone / ConflictResolution are notifications that must
    // reach Steam's internal cloud state machine untouched.
    //
    // Only swallow the request if we can actually deliver the answer.
    // Drain runs from the RecvPkt hook, which does nothing until the
    // packet layout is known - suppressing the send before then would
    // leave the job waiting forever on a reply that never comes.
    
    match fnv1a_hash(target) {
        HASH_JOB_GET_USER_STATS => {
            userstats::handle_send_get_user_stats(body, hdr).map(SendReplacement::Body)
        }
        HASH_JOB_GET_MANIFEST_REQUEST_CODE => {
            manifest::handle_send(body, hdr);
            None
        }
        _ => None,
    }
}

fn recv_service_job(target: &str, body: &[u8], hdr: &[u8]) -> RecvReplacement {
    LOG_NETPACKET_DEBUG!("Recv target_job_name: {}", target);

    match fnv1a_hash(target) {
        HASH_JOB_NOTIFY_RUNNING_APPS => family_sharing::clear_body(body),
        HASH_JOB_GET_USER_STATS => userstats::handle_recv_get_user_stats_response(hdr, body),
        HASH_JOB_GET_MANIFEST_REQUEST_CODE => {
            // Replies to our own originated requests are consumed here;
            // the normal handler would ignore them anyway, but this
            // keeps the two paths from having to reason about each
            // other.
            if manifest_probe::handle_recv(body, hdr) {
                return RecvReplacement::None;
            }
            manifest::handle_recv(body, hdr)
        }
        _ => RecvReplacement::None,
    }
}

// ── eMsg dispatch ───────────────────────────────────────────────────

pub unsafe fn send_frame(frame: &[u8]) -> SendReplacement {
    let Some(p) = packet::unpack_raw(frame) else {
        return SendReplacement::None;
    };
    let st = super::state::state();
    st.need_replace_send = false;
    st.suppress_send = false;

    LOG_NETPACKET_DEBUG!(
        "Send eMsg {}({}) (cbBody={}, cbHdr={})",
        msg_name(p.e_msg), p.e_msg, p.body.len(), p.hdr.len(),
    );

    match p.e_msg {
        packet::K_EMSG_SERVICE_METHOD_CALL_FROM_CLIENT => {
            // Keep one real outbound 151 header as the template for
            // ManifestProbe-originated requests.
            manifest_probe::capture_context(
                std::ptr::null_mut(), std::ptr::null_mut(), Some(p.hdr),
            );
            if let Ok(hdr_msg) = CMsgProtoBufHeader::decode(p.hdr) {
                if let Some(target) = hdr_msg.target_job_name.as_deref() {
                    if let Some(rep) = send_service_job(target, p.body, p.hdr) {
                        return rep;
                    }
                }
            }
            SendReplacement::None
        }
        packet::K_EMSG_CLIENT_PICS_PRODUCT_INFO_REQUEST => {
            match access_token::handle_send(p.body) {
                Some(new_body) => SendReplacement::Body(new_body),
                None => SendReplacement::None,
            }
        }
        packet::K_EMSG_CLIENT_GAMES_PLAYED
        | packet::K_EMSG_CLIENT_GAMES_PLAYED_WITH_DATA_BLOB => {
            richpresence::track_send_games_played(p.body, p.hdr);
            match onlinefix::handle_send(p.body, p.hdr) {
                Some(new_body) => SendReplacement::Body(new_body),
                None => SendReplacement::None,
            }
        }
        packet::K_EMSG_CLIENT_RICH_PRESENCE_UPLOAD => {
            richpresence::track_rp_send(p.body);
            SendReplacement::None
        }
        packet::K_EMSG_CLIENT_GET_USER_STATS => {
            match userstats::handle_send_client_get_user_stats(p.body) {
                Some(new_body) => SendReplacement::Body(new_body),
                None => SendReplacement::None,
            }
        }
        packet::K_EMSG_CLIENT_STORE_USER_STATS2 => {
            let app_id = richpresence::playing_app_id();
            if app_id != 0 {
                cloud_redirect::notify_stats_stored(app_id);
            }
            SendReplacement::None
        }
        _ => SendReplacement::None,
    }
}

pub unsafe fn recv_frame(frame: &[u8]) -> RecvReplacement {
    let Some(p) = packet::unpack_raw(frame) else {
        return RecvReplacement::None;
    };
    let st = super::state::state();
    st.need_replace_body = false;
    st.need_replace_hdr = false;

    if p.e_msg == packet::K_EMSG_MULTI {
        LOG_NETPACKET_TRACE!("Received k_EMsgMulti, skipping dispatch");
        return RecvReplacement::None;
    }

    LOG_NETPACKET_DEBUG!(
        "Recv eMsg {}({}) (cbBody={}, cbHdr={})",
        msg_name(p.e_msg), p.e_msg, p.body.len(), p.hdr.len(),
    );

    match p.e_msg {
        packet::K_EMSG_SERVICE_METHOD_RESPONSE => {
            if let Ok(hdr_msg) = CMsgProtoBufHeader::decode(p.hdr) {
                if let Some(target) = hdr_msg.target_job_name.as_deref() {
                    return recv_service_job(target, p.body, p.hdr);
                }
            }
            RecvReplacement::None
        }
        // Disabled in the C++ with the note "migrated to IPC Layer
        // Hooks_IPC_ISteamUser::GetEncryptedAppTicketResponse". Ported
        // for reference in `super::eticket`; not called here.
        packet::K_EMSG_CLIENT_GET_USER_STATS_RESPONSE => {
            match userstats::handle_recv_client_get_user_stats_response(p.body) {
                Some(new_body) => RecvReplacement::Body(new_body),
                None => RecvReplacement::None,
            }
        }
        packet::K_EMSG_CLIENT_SHARED_LIBRARY_STOP_PLAYING => {
            family_sharing::clear_body(p.body)
        }
        packet::K_EMSG_CLIENT_PERSONA_STATE => {
            match richpresence::handle_recv(p.body, p.hdr) {
                Some(new_body) => RecvReplacement::Body(new_body),
                None => RecvReplacement::None,
            }
        }
        packet::K_EMSG_CLIENT_LICENSE_LIST => {
            licenses::handle_recv(p.body);
            RecvReplacement::None
        }
        _ => RecvReplacement::None,
    }
}

// Keep the flag constant visible.
#[allow(dead_code)]
const _: u32 = K_MSG_HDR_PROTO_FLAG;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_replacement_none_is_default() {
        matches!(SendReplacement::None, SendReplacement::None);
    }

    #[test]
    fn recv_replacement_none_is_default() {
        matches!(RecvReplacement::None, RecvReplacement::None);
    }

    #[test]
    fn hash_constants_are_distinct() {
        assert_ne!(HASH_JOB_NOTIFY_RUNNING_APPS, HASH_JOB_GET_USER_STATS);
        assert_ne!(HASH_JOB_GET_USER_STATS, HASH_JOB_GET_MANIFEST_REQUEST_CODE);
    }

    #[test]
    fn recv_service_job_unknown_target_is_none() {
        matches!(recv_service_job("Unknown.Job#1", &[], &[]), RecvReplacement::None);
    }
}
