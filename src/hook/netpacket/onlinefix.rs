//! `Hooks_NetPacket_OnlineFix`. Outgoing `CMsgClientGamesPlayed` (eMsg
//! 742 / 5410). When a game launched with `-onlinefix` reports appid
//! 480, replace `game_extra_info` with the real game's localized name so
//! friends see the correct title.

use prost::Message;

use crate::dllmain::K_ONLINE_FIX_APPID;
use crate::hook::misc as hooks_misc;
use crate::steam::proto::CMsgClientGamesPlayed;

pub fn handle_send(body: &[u8], _hdr: &[u8]) -> Option<Vec<u8>> {
    let mut msg = match CMsgClientGamesPlayed::decode(body) {
        Ok(m) => m,
        Err(_) => {
            LOG_ONLINEFIX_WARN!("OnlineFix: failed to parse CMsgClientGamesPlayed");
            return None;
        }
    };

    let mut patched = false;
    for game in msg.games_played.iter_mut() {
        let appid = (game.game_id.unwrap_or(0) & 0xFFFF_FFFF) as u32;
        if appid == K_ONLINE_FIX_APPID {
            let real_app_id = hooks_misc::resolve_app_id();
            if real_app_id != 0 && real_app_id != K_ONLINE_FIX_APPID {
                let name = hooks_misc::get_game_name_by_app_id(real_app_id);
                if !name.is_empty() {
                    game.game_extra_info = Some(name.clone());
                    patched = true;
                    LOG_ONLINEFIX_INFO!(
                        "OnlineFix: 480 -> name '{}' (real appid {})",
                        name, real_app_id,
                    );
                }
            }
        }
    }

    if !patched { return None; }
    let encoded = msg.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_ONLINEFIX_WARN!("OnlineFix: encoded size {} exceeds buffer", encoded.len());
        return None;
    }
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam::proto::CMsgClientGamesPlayed;
    use crate::steam::proto::c_msg_client_games_played::GamePlayed;

    fn encode(msg: &CMsgClientGamesPlayed) -> Vec<u8> {
        msg.encode_to_vec()
    }

    #[test]
    fn non_480_game_id_not_patched() {
        let msg = CMsgClientGamesPlayed {
            games_played: vec![GamePlayed {
                game_id: Some(730),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert!(handle_send(&encode(&msg), &[]).is_none());
    }

    #[test]
    fn malformed_body_returns_none() {
        assert!(handle_send(&[0xFF, 0xFF, 0xFF], &[]).is_none());
    }
}
