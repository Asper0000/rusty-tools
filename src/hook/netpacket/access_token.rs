//! `Hooks_NetPacket_AccessToken`. Outgoing
//! `CMsgClientPicsProductInfoRequest` (eMsg 8903).

use prost::Message;

use crate::steam::proto::CMsgClientPicsProductInfoRequest;
use crate::utils::config::lua_config;

/// Patch `access_token` into each `apps[i]` entry whose appid is in
/// Lua's `addtoken` set. Returns `Some(new_body)` when at least one
/// patch was applied, `None` otherwise.
pub fn handle_send(body: &[u8]) -> Option<Vec<u8>> {
    let req = CMsgClientPicsProductInfoRequest::decode(body).ok()?;
    LOG_PICS_DEBUG!(
        "CMsgClientPicsProductInfoRequest: {} apps, {} packages",
        req.apps.len(), req.packages.len(),
    );

    let mut needs_patch = false;
    for app in &req.apps {
        let appid = app.appid.unwrap_or(0);
        if lua_config::has_depot(appid, false)
            && lua_config::get_access_token(appid) != 0
        {
            needs_patch = true;
            LOG_PICS_DEBUG!(
                "CMsgClientPICSProductInfoRequest: found appid {} with access_token, need patching",
                appid,
            );
            break;
        }
    }
    if !needs_patch {
        LOG_PICS_TRACE!(
            "CMsgClientPICSProductInfoRequest: no apps need token injection, skip"
        );
        return None;
    }

    let mut req = req;
    let mut injected = 0;
    let mut no_token = 0;
    let mut not_addappid = 0;
    for app in req.apps.iter_mut() {
        let appid = app.appid.unwrap_or(0);
        if lua_config::has_depot(appid, false) {
            let token = lua_config::get_access_token(appid);
            if token != 0 {
                LOG_PICS_DEBUG!(
                    "CMsgClientPICSProductInfoRequest: inject appid={}: {} -> {}",
                    appid,
                    app.access_token.map(|t| t.to_string()).unwrap_or_else(|| "absent".into()),
                    token,
                );
                app.access_token = Some(token);
                injected += 1;
            } else {
                LOG_PICS_WARN!(
                    "CMsgClientPICSProductInfoRequest: skip appid={}: in depot, no token configured",
                    appid,
                );
                no_token += 1;
            }
        } else {
            not_addappid += 1;
        }
    }
    LOG_PICS_DEBUG!(
        "CMsgClientPICSProductInfoRequest: injected={} no_token={} not_in_add_appid={} total={}",
        injected, no_token, not_addappid, req.apps.len(),
    );

    let encoded = req.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_PICS_WARN!(
            "CMsgClientPICSProductInfoRequest: encoded size {} exceeds buffer",
            encoded.len(),
        );
        return None;
    }
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam::proto::CMsgClientPicsProductInfoRequest;

    fn encode(req: &CMsgClientPicsProductInfoRequest) -> Vec<u8> {
        req.encode_to_vec()
    }

    #[test]
    fn no_apps_needs_patch_returns_none() {
        let req = CMsgClientPicsProductInfoRequest {
            apps: vec![crate::steam::proto::c_msg_client_pics_product_info_request::AppInfo {
                appid: Some(0xDEAD_BEEF),
                access_token: None,
                only_public_obsolete: None,
            }],
            ..Default::default()
        };
        let body = encode(&req);
        assert!(handle_send(&body).is_none());
    }

    #[test]
    fn malformed_body_returns_none() {
        assert!(handle_send(&[0xFF, 0xFF, 0xFF]).is_none());
    }
}
