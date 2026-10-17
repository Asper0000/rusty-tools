//! `Hooks_NetPacket_ETicket`. Incoming eMsg 5527
//! (`CMsgClientRequestEncryptedAppTicketResponse`).
//!
//! **Not wired.** The C++ has this dispatch case commented out with the
//! note "migrated to IPC Layer
//! `Hooks_IPC_ISteamUser::GetEncryptedAppTicketResponse`". The handler
//! is preserved here for reference so a future patch can re-enable it
//! by uncommenting one line in `dispatch.rs`; today it is not called
//! from anywhere.

use prost::Message;

use crate::steam::proto::{
    CMsgClientRequestEncryptedAppTicketResponse, EncryptedAppTicket,
};
use crate::utils::config::lua_config;
use crate::utils::tickets::app_ticket;

const K_ERESULT_OK: i32 = 1;

/// Serve a stored Encrypted App Ticket in place of the server's non-OK
/// response. Returns the new body when a replacement was made.
pub fn handle_recv(body: &[u8]) -> Option<Vec<u8>> {
    let mut resp = CMsgClientRequestEncryptedAppTicketResponse::decode(body).ok()?;
    if resp.eresult.unwrap_or(2) == K_ERESULT_OK { return None; }
    let app_id = resp.app_id.unwrap_or(0);
    if !lua_config::has_depot(app_id, false) { return None; }

    let ticket = app_ticket::get_encrypted_ticket_from_credential_store(app_id);
    if ticket.is_empty() { return None; }

    let parsed = EncryptedAppTicket::decode(&*ticket).ok()?;
    resp.encrypted_app_ticket = Some(parsed);
    resp.eresult = Some(K_ERESULT_OK);

    let encoded = resp.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_NETPACKET_WARN!(
            "ClientRequestEncryptedAppTicketResponse: modified message too large ({})",
            encoded.len(),
        );
        return None;
    }
    LOG_NETPACKET_DEBUG!(
        "ClientRequestEncryptedAppTicketResponse: served stored ticket for appid={} ({} bytes)",
        app_id, ticket.len(),
    );
    Some(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_body_returns_none() {
        assert!(handle_recv(&[0xFF, 0xFF, 0xFF]).is_none());
    }

    #[test]
    fn ok_response_is_passed_through() {
        // Build a minimal response with eresult = OK.
        use crate::steam::proto::CMsgClientRequestEncryptedAppTicketResponse;
        let resp = CMsgClientRequestEncryptedAppTicketResponse {
            app_id: Some(730),
            eresult: Some(K_ERESULT_OK),
            encrypted_app_ticket: None,
        };
        let body = resp.encode_to_vec();
        assert!(handle_recv(&body).is_none());
    }
}
