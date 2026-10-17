//! App ownership / encrypted tickets. Source: `Utils/Tickets/AppTicket.cpp`.

use crate::bridge;
use crate::platform::steam_credential_store as creds;
use crate::utils::config::lua_config;

pub const K_APP_TICKET_STEAMID_OFFSET: u32 = 8;
pub const K_APP_TICKET_APPID_OFFSET: u32 = 16;
pub const K_APP_TICKET_SIGNATURE_SIZE: u32 = 128;

const K_LOCAL_APP_TICKET_SOURCE_APPID: u32 = 7;
const K_STEAMID_TICKET_MINIMUM_SIZE: usize = 16;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AppTicketSource {
    CredentialStoreOnly,
    ForgeOnly,
    CredentialStoreThenForge,
}

#[derive(Clone, Debug)]
pub struct AppOwnershipTicket {
    pub data: Vec<u8>,
    pub total_size: u32,
    pub app_id_offset: u32,
    pub steam_id_offset: u32,
    pub signature_offset: u32,
    pub signature_size: u32,
}

impl Default for AppOwnershipTicket {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            total_size: 0,
            app_id_offset: K_APP_TICKET_APPID_OFFSET,
            steam_id_offset: K_APP_TICKET_STEAMID_OFFSET,
            signature_offset: 0,
            signature_size: K_APP_TICKET_SIGNATURE_SIZE,
        }
    }
}

fn get_steam_id_from_credential_store(app_id: u32) -> u64 {
    match creds::get_steam_id(app_id) {
        Ok(v) => {
            LOG_DEBUG!("GetSpoofSteamID for AppId {}: SteamID credential -> 0x{:X}({})",
                       app_id, v, v);
            v
        }
        Err(status) => {
            LOG_TRACE!("GetSpoofSteamID for AppId {}: SteamID unavailable in credential store ({})",
                       app_id, status.as_str());
            0
        }
    }
}

pub fn get_app_ownership_ticket_from_credential_store(app_id: u32) -> Vec<u8> {
    if !lua_config::has_depot(app_id, true) {
        LOG_DEBUG!("GetAppOwnershipTicketFromCredentialStore for AppId {}: not in addappid, skip", app_id);
        return Vec::new();
    }
    match creds::get_app_ticket(app_id) {
        Ok(ticket) => {
            LOG_INFO!("Successfully retrieved App Ownership Ticket from credential store, AppId: {}, Ticket Size: {}",
                      app_id, ticket.len());
            ticket
        }
        Err(status) => {
            LOG_TRACE!("Read App Ownership Ticket for AppId {}: cached credential unavailable ({})",
                       app_id, status.as_str());
            Vec::new()
        }
    }
}

/// Exploit steamdrmp's off-by-four ticket parsing: take the local source
/// appticket (for app 7), strip its 128-byte signature, insert our 4-byte
/// appid, and re-append the signature.
fn forge_local_app_ownership_ticket(app_id: u32) -> Vec<u8> {
    let source = bridge::hooks_get_cache_app_ownership_ticket(K_LOCAL_APP_TICKET_SOURCE_APPID);
    if source.len() <= K_APP_TICKET_SIGNATURE_SIZE as usize {
        LOG_DEBUG!("ForgeLocalAppOwnershipTicket for AppId {}: no source appticket", app_id);
        return Vec::new();
    }

    let signed_size = source.len() - K_APP_TICKET_SIGNATURE_SIZE as usize;
    let mut ticket = Vec::with_capacity(source.len() + 4);
    ticket.extend_from_slice(&source[..signed_size]);
    ticket.extend_from_slice(&app_id.to_le_bytes());
    ticket.extend_from_slice(&source[signed_size..]);

    LOG_INFO!("Forged App Ownership Ticket, AppId: {}, SourceAppId: {}, Physical Size: {}, Total Size: {}",
              app_id, K_LOCAL_APP_TICKET_SOURCE_APPID, ticket.len(), source.len());
    ticket
}

pub fn get_app_ownership_ticket(app_id: u32, source: AppTicketSource) -> Option<AppOwnershipTicket> {
    if matches!(source, AppTicketSource::CredentialStoreOnly | AppTicketSource::CredentialStoreThenForge) {
        let data = get_app_ownership_ticket_from_credential_store(app_id);
        if !data.is_empty() && data.len() >= 4 {
            let sig_off = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            return Some(AppOwnershipTicket {
                total_size: data.len() as u32,
                app_id_offset: K_APP_TICKET_APPID_OFFSET,
                steam_id_offset: K_APP_TICKET_STEAMID_OFFSET,
                signature_offset: sig_off,
                signature_size: K_APP_TICKET_SIGNATURE_SIZE,
                data,
            });
        }
    }
    if matches!(source, AppTicketSource::CredentialStoreOnly) { return None; }

    let data = forge_local_app_ownership_ticket(app_id);
    if data.is_empty() { return None; }

    let total_size = (data.len() - 4) as u32;
    let app_id_offset = total_size - K_APP_TICKET_SIGNATURE_SIZE;
    Some(AppOwnershipTicket {
        total_size,
        app_id_offset,
        steam_id_offset: K_APP_TICKET_STEAMID_OFFSET,
        signature_offset: app_id_offset + 4,
        signature_size: K_APP_TICKET_SIGNATURE_SIZE,
        data,
    })
}

pub fn get_encrypted_ticket_from_credential_store(app_id: u32) -> Vec<u8> {
    if !lua_config::has_depot(app_id, true) {
        LOG_DEBUG!("GetEncryptedTicketFromCredentialStore for AppId {}: not in addappid, skip", app_id);
        return Vec::new();
    }
    match creds::get_e_ticket(app_id) {
        Ok(ticket) => {
            LOG_INFO!("Successfully retrieved Encrypted App Ticket from credential store, AppId: {}, Ticket Size: {}",
                      app_id, ticket.len());
            ticket
        }
        Err(status) => {
            LOG_TRACE!("Read Encrypted App Ticket for AppId {}: cached credential unavailable ({})",
                       app_id, status.as_str());
            Vec::new()
        }
    }
}

pub fn write_app_ownership_ticket(app_id: u32, data: &[u8]) -> bool {
    match creds::write_app_ticket(app_id, data) {
        creds::Status::Ok => {
            LOG_INFO!("Wrote AppTicket for AppId {} ({} bytes)", app_id, data.len());
            true
        }
        s => {
            LOG_ERROR!("Failed to write AppTicket for AppId {} to credential store: {}",
                       app_id, s.as_str());
            false
        }
    }
}

pub fn write_encrypted_ticket(app_id: u32, data: &[u8]) -> bool {
    match creds::write_e_ticket(app_id, data) {
        creds::Status::Ok => {
            LOG_INFO!("Wrote ETicket for AppId {} ({} bytes)", app_id, data.len());
            true
        }
        s => {
            LOG_ERROR!("Failed to write ETicket for AppId {} to credential store: {}",
                       app_id, s.as_str());
            false
        }
    }
}

pub fn write_steam_id(app_id: u32, steam_id: u64) -> bool {
    match creds::write_steam_id(app_id, steam_id) {
        creds::Status::Ok => {
            LOG_INFO!("Wrote SteamID for AppId {} ({})", app_id, steam_id);
            true
        }
        s => {
            LOG_ERROR!("Failed to write SteamID for AppId {} to credential store: {}",
                       app_id, s.as_str());
            false
        }
    }
}

pub fn extract_steam_id_from_ticket_bytes(ticket: &[u8]) -> u64 {
    if ticket.len() < K_STEAMID_TICKET_MINIMUM_SIZE { return 0; }
    // Layout: [u32 Size][u32 Version][u64 SteamID][...].
    // Read bytes [8..16] as little-endian u64. Uses byte-by-byte
    // construction because the Vec's alignment is not guaranteed.
    u64::from_le_bytes([
        ticket[8], ticket[9], ticket[10], ticket[11],
        ticket[12], ticket[13], ticket[14], ticket[15],
    ])
}

pub fn get_spoof_steam_id(app_id: u32) -> u64 {
    if !lua_config::has_depot(app_id, true) {
        LOG_DEBUG!("GetSpoofSteamID for AppId {}: not in addappid, skip spoofing", app_id);
        return 0;
    }
    let credential_steam_id = get_steam_id_from_credential_store(app_id);
    if credential_steam_id != 0 { return credential_steam_id; }

    let ticket = get_app_ownership_ticket_from_credential_store(app_id);
    let steam_id = extract_steam_id_from_ticket_bytes(&ticket);
    if steam_id != 0 {
        LOG_DEBUG!("GetSpoofSteamID for AppId {}: -> 0x{:X}({})", app_id, steam_id, steam_id);
    }
    steam_id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_steam_id_short_ticket() {
        assert_eq!(extract_steam_id_from_ticket_bytes(&[]), 0);
        assert_eq!(extract_steam_id_from_ticket_bytes(&[0u8; 15]), 0);
    }

    #[test]
    fn extract_steam_id_reads_offset_8() {
        let mut bytes = vec![0u8; 24];
        let id: u64 = 0x1122334455667788;
        bytes[8..16].copy_from_slice(&id.to_le_bytes());
        assert_eq!(extract_steam_id_from_ticket_bytes(&bytes), id);
    }

    #[test]
    fn credential_store_path_extracts_signature_offset() {
        // Simulate a 24-byte ticket whose first u32 says the signature
        // starts at 0x40.
        let mut data = vec![0u8; 24];
        data[0..4].copy_from_slice(&0x40u32.to_le_bytes());
        let sig_off = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        assert_eq!(sig_off, 0x40);
    }
}
