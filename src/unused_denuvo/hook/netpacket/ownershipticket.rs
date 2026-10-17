//! `Hooks_NetPacket_OwnershipTicket`. Incoming eMsg 858
//! (`CMsgClientGetAppOwnershipTicketResponse`).
//!
//! Some Denuvo titles verify ownership via this network message instead
//! of the IPC `GetAppOwnershipTicketExtendedData` path, so the IPC
//! ownership spoof never engages and the real (non-owning) account
//! leaks through as error 88500012. This handler supplies a
//! credential-store or freshly-minted ticket when Steam returns a
//! non-OK response.

use prost::Message;

use crate::steam::proto::CMsgClientGetAppOwnershipTicketResponse;
use crate::utils::config::lua_config;
use crate::utils::tickets::{app_ticket, eticket_client};

const K_ERESULT_OK: i32 = 1;

pub fn handle_recv(body: &[u8]) -> Option<Vec<u8>> {
    let mut resp = CMsgClientGetAppOwnershipTicketResponse::decode(body).ok()?;

    // Steam already returned a valid ticket (account owns it) - leave it.
    if resp.eresult.unwrap_or(2) == K_ERESULT_OK { return None; }

    let app_id = resp.app_id.unwrap_or(0);
    if !lua_config::has_depot(app_id, false) { return None; }

    let orig_eresult = resp.eresult.unwrap_or(2);

    // Prefer the credential-store ticket when it is already valid: that
    // ensures GetAppOwnershipTicketExtendedData and the 858 response
    // hand Denuvo the identical bytes. Serving a different
    // (backend-minted) ticket here caused a cross-check mismatch even
    // when the SteamID was the same account. Only mint from the backend
    // when the credential store has no valid ticket.
    let stored = app_ticket::get_app_ownership_ticket_from_credential_store(app_id);
    let existing_steam_id = app_ticket::extract_steam_id_from_ticket_bytes(&stored);

    let ticket_bytes = if existing_steam_id != 0 {
        stored
    } else {
        match eticket_client::fetch_ownership_ticket(app_id, &[], 0) {
            Some(t) => t,
            None => {
                LOG_NETPACKET_WARN!(
                    "OwnershipTicketResponse[858]: appid={} eresult={} but no owner ticket available",
                    app_id, orig_eresult,
                );
                return None;
            }
        }
    };

    let ticket_len = ticket_bytes.len();
    resp.ticket = Some(ticket_bytes);
    resp.eresult = Some(K_ERESULT_OK);

    let encoded = resp.encode_to_vec();
    if encoded.len() > super::state::K_MAX_BODY_SIZE {
        LOG_NETPACKET_WARN!(
            "OwnershipTicketResponse[858]: modified message too large ({})",
            encoded.len(),
        );
        return None;
    }
    LOG_NETPACKET_INFO!(
        "OwnershipTicketResponse[858]: spoofed appid={} ticket_bytes={} (orig eresult={} -> OK)",
        app_id, ticket_len, orig_eresult,
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
        use crate::steam::proto::CMsgClientGetAppOwnershipTicketResponse;
        let resp = CMsgClientGetAppOwnershipTicketResponse {
            eresult: Some(K_ERESULT_OK),
            app_id: Some(730),
            ticket: None,
        };
        let body = resp.encode_to_vec();
        assert!(handle_recv(&body).is_none());
    }

    #[test]
    fn non_owned_app_is_passed_through() {
        // eresult != OK, but appid is not in the (empty) Lua config.
        use crate::steam::proto::CMsgClientGetAppOwnershipTicketResponse;
        let resp = CMsgClientGetAppOwnershipTicketResponse {
            eresult: Some(15), // AccessDenied
            app_id: Some(0xDEAD_BEEF),
            ticket: None,
        };
        let body = resp.encode_to_vec();
        assert!(handle_recv(&body).is_none());
    }
}
