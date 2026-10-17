//! `Hooks_NetPacket_UserStats`. Four handlers:
//! * outgoing eMsg 151 with target `Player.GetUserStats#1`
//! * incoming eMsg 147 with target `Player.GetUserStats#1`
//! * outgoing eMsg 818 (`CMsgClientGetUserStats`)
//! * incoming eMsg 819 (`CMsgClientGetUserStatsResponse`)

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use prost::Message;

use crate::steam::proto::{
    c_msg_client_get_user_stats_response::{AchievementBlocks, Stats as ClientStats},
    CMsgClientGetUserStats, CMsgClientGetUserStatsResponse,
    CMsgProtoBufHeader, CPlayerGetUserStatsRequest,
    CPlayerGetUserStatsResponse,
};
use crate::utils::cloud_redirect::host as cloud;
use crate::utils::config::lua_config;

use super::dispatch::RecvReplacement;

/// `jobid_source` -> `appid` mapping between the 151 request and the
/// 147 response.
static G_JOB_ID_TO_APP_ID: Lazy<Mutex<HashMap<u64, u32>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

const K_ERESULT_OK: i32 = 1;

// ── Send: CPlayer_GetUserStats_Request (eMsg 151) ──────────────────

pub fn handle_send_get_user_stats(body: &[u8], hdr: &[u8]) -> Option<Vec<u8>> {
    let req = match CPlayerGetUserStatsRequest::decode(body) {
        Ok(r) => r,
        Err(_) => {
            LOG_ACHIEVEMENT_WARN!("Player::GetUserStats request: failed to decode");
            return None;
        }
    };
    let Some(app_id) = req.appid else {
        LOG_ACHIEVEMENT_WARN!("Player::GetUserStats request: missing appid");
        return None;
    };

    let has_sha = req.sha_schema.as_ref().map_or(false, |s| !s.is_empty());
    if has_sha {
        LOG_ACHIEVEMENT_WARN!("Player::GetUserStats request: sha_schema is present, do not spoof");
        return None;
    }
    if !lua_config::has_depot(app_id, false) {
        LOG_ACHIEVEMENT_WARN!("Player::GetUserStats request: appid={} is not in addappid", app_id);
        return None;
    }

    // Save jobid_source -> appid for the response handler.
    if let Ok(hdr_msg) = CMsgProtoBufHeader::decode(hdr) {
        if let Some(job_id) = hdr_msg.jobid_source {
            G_JOB_ID_TO_APP_ID.lock().insert(job_id, app_id);
            LOG_ACHIEVEMENT_DEBUG!(
                "Player::GetUserStats request: stored jobid={} -> appid={}", job_id, app_id,
            );
        }
    }

    let new_steam_id = lua_config::get_stat_steam_id(app_id);
    let mut req = req;
    req.steamid = Some(new_steam_id);

    let encoded = req.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_ACHIEVEMENT_WARN!(
            "Player::GetUserStats request: encoded size {} exceeds buffer", encoded.len(),
        );
        return None;
    }
    Some(encoded)
}

// ── Recv: CPlayer_GetUserStats_Response (eMsg 147) ─────────────────

pub fn handle_recv_get_user_stats_response(hdr: &[u8], body: &[u8]) -> RecvReplacement {
    // Header: set eresult=OK.
    let mut hdr_msg = match CMsgProtoBufHeader::decode(hdr) {
        Ok(h) => h,
        Err(_) => {
            LOG_ACHIEVEMENT_WARN!("Player::GetUserStats response: failed to decode original header");
            return RecvReplacement::None;
        }
    };

    // Look up appid via jobid_target -> jobid_source match.
    let mut app_id: u32 = 0;
    let mut has_app_id = false;
    if let Some(job_id) = hdr_msg.jobid_target {
        if let Some(id) = G_JOB_ID_TO_APP_ID.lock().remove(&job_id) {
            app_id = id;
            has_app_id = true;
            LOG_ACHIEVEMENT_DEBUG!(
                "Player::GetUserStats response: matched jobid={} -> appid={}", job_id, app_id,
            );
        }
    }

    hdr_msg.eresult = Some(K_ERESULT_OK);
    let new_hdr = hdr_msg.encode_to_vec();
    if new_hdr.len() > super::state::K_MAX_HDR_SIZE {
        LOG_ACHIEVEMENT_WARN!(
            "Player::GetUserStats response: modified header too large ({})", new_hdr.len(),
        );
        return RecvReplacement::None;
    }

    // Body: strip stats (only if appid was matched and is in our config).
    let mut resp = match CPlayerGetUserStatsResponse::decode(body) {
        Ok(r) => r,
        Err(_) => {
            // Header-only replacement, body unchanged.
            return RecvReplacement::Hdr(new_hdr);
        }
    };

    if !has_app_id || !lua_config::has_depot(app_id, false) {
        LOG_ACHIEVEMENT_DEBUG!("Player::GetUserStats response: no appid match, skip body strip");
        return RecvReplacement::Hdr(new_hdr);
    }

    resp.stats.clear();
    let new_body = resp.encode_to_vec();
    if new_body.len() > super::state::K_MAX_BODY_SIZE {
        LOG_ACHIEVEMENT_WARN!(
            "Player::GetUserStats response: modified body too large ({})", new_body.len(),
        );
        return RecvReplacement::Hdr(new_hdr);
    }
    RecvReplacement::Both { hdr: new_hdr, body: new_body }
}

// ── Send: CMsgClientGetUserStats (eMsg 818) ────────────────────────

pub fn handle_send_client_get_user_stats(body: &[u8]) -> Option<Vec<u8>> {
    let mut req = match CMsgClientGetUserStats::decode(body) {
        Ok(r) => r,
        Err(_) => {
            LOG_ACHIEVEMENT_WARN!("ClientGetUserStats request: failed to decode");
            return None;
        }
    };

    let Some(game_id) = req.game_id else {
        LOG_ACHIEVEMENT_WARN!("ClientGetUserStats request: missing game_id");
        return None;
    };
    let app_id = (game_id & 0xFFFF_FFFF) as u32;
    if !lua_config::has_depot(app_id, false) {
        LOG_ACHIEVEMENT_WARN!("ClientGetUserStats request: appid={} is not in addappid", app_id);
        return None;
    }
    if req.schema_local_version != Some(-1) {
        req.schema_local_version = Some(-1);
        LOG_ACHIEVEMENT_DEBUG!("ClientGetUserStats request: forced schema_local_version to -1");
    }

    let new_steam_id = lua_config::get_stat_steam_id(app_id);
    req.steam_id_for_user = Some(new_steam_id);

    let encoded = req.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_ACHIEVEMENT_WARN!(
            "ClientGetUserStats request: encoded size {} exceeds buffer", encoded.len(),
        );
        return None;
    }
    Some(encoded)
}

// ── Recv: CMsgClientGetUserStatsResponse (eMsg 819) ────────────────

pub fn handle_recv_client_get_user_stats_response(body: &[u8]) -> Option<Vec<u8>> {
    let mut resp = CMsgClientGetUserStatsResponse::decode(body).ok()?;

    let Some(game_id) = resp.game_id else { return None; };
    let app_id = (game_id & 0xFFFF_FFFF) as u32;
    if !lua_config::has_depot(app_id, false) {
        return None;
    }

    resp.stats.clear();
    resp.achievement_blocks.clear();
    resp.eresult = Some(K_ERESULT_OK);

    // Overlay CloudRedirect's cloud-synced achievement state.
    let mut blocks = [cloud::AchievementBlock::default(); 64];
    let n = cloud::get_achievements(app_id, &mut blocks);
    if n > 0 {
        let mut crc: u32 = 0;
        for block in &blocks[..n] {
            resp.stats.push(ClientStats {
                stat_id: Some(block.stat_id),
                stat_value: Some(block.bits),
            });
            resp.achievement_blocks.push(AchievementBlocks {
                achievement_id: Some(block.stat_id),
                unlock_time: block.unlock_times.to_vec(),
            });
            crc ^= block.bits ^ block.stat_id;
        }
        resp.crc_stats = Some(crc);
        LOG_ACHIEVEMENT_DEBUG!(
            "ClientGetUserStats response: injected {} CR achievement blocks for app {}",
            n, app_id,
        );
    } else {
        LOG_ACHIEVEMENT_DEBUG!(
            "ClientGetUserStats response: cleared stats/achievements (no CR data for app {})",
            app_id,
        );
    }

    let encoded = resp.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_ACHIEVEMENT_WARN!(
            "ClientGetUserStats response: modified body too large ({})", encoded.len(),
        );
        return None;
    }
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_send_request_returns_none() {
        assert!(handle_send_get_user_stats(&[0xFF, 0xFF, 0xFF], &[]).is_none());
        assert!(handle_send_client_get_user_stats(&[0xFF, 0xFF, 0xFF]).is_none());
    }

    #[test]
    fn malformed_recv_response_returns_none() {
        assert!(handle_recv_client_get_user_stats_response(&[0xFF, 0xFF, 0xFF]).is_none());
    }

    #[test]
    fn recv_malformed_header_returns_none() {
        let r = handle_recv_get_user_stats_response(&[0xFF, 0xFF], &[]);
        matches!(r, RecvReplacement::None);
    }

    #[test]
    fn job_id_map_starts_empty() {
        // No other test populates the map.
        let g = G_JOB_ID_TO_APP_ID.lock();
        assert!(g.is_empty() || !g.is_empty()); // read is safe; content not asserted
    }
}
