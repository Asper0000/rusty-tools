//! `Hooks_IPC_ISteamUser.cpp`. Post-handlers for the four
//! `IClientUser` methods the tool intercepts:
//!
//! * `GetSteamID` — spoof the SteamID for Lua-managed apps.
//! * `GetAppOwnershipTicketExtendedData` — serve a forged or
//!   credential-store appticket.
//! * `RequestEncryptedAppTicket` — record the async call so the
//!   callback can be answered later, and mint a fresh nonce-bound
//!   eticket when the strict-Denuvo path is active.
//! * `GetEncryptedAppTicket` — serve the fresh or credential-store
//!   eticket.

use std::collections::HashMap;
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::bridge;
use crate::dllmain::K_ONLINE_FIX_APPID;
use crate::hook::ipc::{register_handlers, HandlerEntry, IpcHandlerFn};
use crate::hook::misc as hooks_misc;
use crate::hook::pending_api_calls;
use crate::steam::ipc_facades::{
    GetAppOwnershipTicketExtendedDataReq, GetAppOwnershipTicketExtendedDataResp,
    GetEncryptedAppTicketResp, GetSteamIDResp, RequestEncryptedAppTicketReq,
    RequestEncryptedAppTicketResp,
};
use crate::steam::structs::{CPipeClient, CUtlBuffer};
use crate::steam::structs::CSteamID;
use crate::steam::types::AppId_t;
use crate::utils::tickets::app_ticket::{self, AppTicketSource};
use crate::utils::tickets::eticket_client;

/// Fresh, nonce-bound etickets minted on-demand in
/// `RequestEncryptedAppTicket`, consumed by `GetEncryptedAppTicket` on
/// the same launch.
static G_FRESH_ETICKET: Lazy<Mutex<HashMap<AppId_t, Vec<u8>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Handlers ────────────────────────────────────────────────────────

unsafe fn handler_post_get_steam_id(
    _pipe: *mut CPipeClient,
    _p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
) {
    let app_id = hooks_misc::resolve_app_id();
    let resp = GetSteamIDResp::from_cutl(p_write);
    if !resp.ok() { return; }

    // Spoof whenever a pool-account ticket exists for this app, not just
    // inside the Denuvo auth window. Denuvo reads its cached offline
    // license on second launch and calls GetSteamID BEFORE or AFTER the
    // auth window to verify it — if we only spoof inside the window the
    // real SteamID leaks out and mismatches the license -> 012.
    // GetSpoofSteamID returns 0 for apps with no credential-store ticket
    // (real owners, non-tracked apps), so the spoof is naturally scoped.
    let spoofed = app_ticket::get_spoof_steam_id(app_id);
    if spoofed == 0 { return; }

    let original = resp.return_value().m_unAll64Bits;
    LOG_IPC_DEBUG!(
        "IClientUser::GetSteamID: AppId={} Original=0x{:X}({}) -> Spoofed=0x{:X}({})",
        app_id, original, original, spoofed, spoofed,
    );
    resp.set_return_value(CSteamID::from_u64(spoofed));
}

unsafe fn handler_post_get_app_ownership_ticket_extended_data(
    pipe: *mut CPipeClient,
    p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
) {
    let req = GetAppOwnershipTicketExtendedDataReq::from_cutl(p_read);
    if !req.ok() { return; }

    let cb_max = req.cb_max_ticket();
    if cb_max < 0 { return; }

    let app_id_raw = req.un_app_id();
    let app_id = if app_id_raw == K_ONLINE_FIX_APPID {
        hooks_misc::resolve_app_id()
    } else {
        app_id_raw
    };

    let ticket_source = if bridge::denuvo_auth_is_authorized_pipe(pipe) {
        AppTicketSource::CredentialStoreOnly
    } else {
        // Outside the auth window: prefer credential-store ticket
        // (pool SteamID) over ForgeOnly (which uses app 7's ticket and
        // carries the real SteamID). When the 858 network spoof is also
        // active, both paths must agree on the same SteamID or Denuvo
        // cross-checks them and rejects.
        LOG_IPC_DEBUG!(
            "IClientUser::GetAppOwnershipTicketExtendedData: AppId={} not in authorization window, credential store preferred",
            app_id,
        );
        AppTicketSource::CredentialStoreThenForge
    };

    let ticket = match app_ticket::get_app_ownership_ticket(app_id, ticket_source) {
        Some(t) => t,
        None => return,
    };

    if ticket.data.len() > cb_max as usize {
        LOG_IPC_WARN!(
            "IClientUser::GetAppOwnershipTicketExtendedData: AppId={} ticket too large ({} bytes) for buffer ({} bytes)",
            app_id, ticket.data.len(), cb_max,
        );
        return;
    }

    let resp = GetAppOwnershipTicketExtendedDataResp::from_cutl(p_write, cb_max as usize);
    if !resp.ok() { return; }

    resp.set_return_value(ticket.total_size);
    if !resp.set_p_ticket(&ticket.data) { return; }
    resp.set_pi_app_id(ticket.app_id_offset);
    resp.set_pi_steam_id(ticket.steam_id_offset);
    resp.set_pi_signature(ticket.signature_offset);
    resp.set_pcb_signature(ticket.signature_size);

    LOG_IPC_DEBUG!(
        "IClientUser::GetAppOwnershipTicketExtendedData: AppId={} total={} appIdOff={} steamIdOff={} sigOff={} sigSize={}",
        app_id, ticket.total_size, ticket.app_id_offset,
        ticket.steam_id_offset, ticket.signature_offset, ticket.signature_size,
    );
}

unsafe fn handler_post_request_encrypted_app_ticket(
    _pipe: *mut CPipeClient,
    p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
) {
    let resp = RequestEncryptedAppTicketResp::from_cutl(p_write);
    if !resp.ok() { return; }

    let app_id = hooks_misc::resolve_app_id();

    {
        let req = RequestEncryptedAppTicketReq::from_cutl(p_read);
        let nonce = if req.ok() { req.p_data() } else { Vec::new() };

        // Whatever account the registry's current static ticket already
        // belongs to (0 if none) — lets the backend pin the mint to that
        // SAME account instead of risking a different pool pick.
        let existing_steam_id = app_ticket::extract_steam_id_from_ticket_bytes(
            &app_ticket::get_app_ownership_ticket_from_credential_store(app_id),
        );

        // Mint a fresh eticket whenever the credential store already has
        // a ticket for this app. The minted eticket is pinned to the
        // same pool account via existing_steam_id, which matches
        // GetSteamID's spoof (also sourced from the credential store via
        // CredentialStoreThenForge) — no error-54 risk.
        if existing_steam_id != 0 {
            if let Some(fresh) =
                eticket_client::fetch_fresh_eticket(app_id, &nonce, existing_steam_id)
            {
                G_FRESH_ETICKET.lock().unwrap().insert(app_id, fresh);
            }
        }
    }

    let have_fresh = G_FRESH_ETICKET.lock().unwrap().contains_key(&app_id);
    let ticket = app_ticket::get_encrypted_ticket_from_credential_store(app_id);
    if ticket.is_empty() && !have_fresh {
        LOG_IPC_DEBUG!("RequestEncryptedAppTicket: AppId={} - no cached eticket, skip", app_id);
        return;
    }

    let h_async_call = resp.return_value();
    pending_api_calls::record_encrypted_ticket(h_async_call, app_id);
    LOG_IPC_DEBUG!(
        "RequestEncryptedAppTicket: AppId={} hAsyncCall=0x{:X} - recorded (fresh={})",
        app_id, h_async_call, have_fresh,
    );
}

unsafe fn handler_post_get_encrypted_app_ticket(
    _pipe: *mut CPipeClient,
    _p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
) {
    let app_id = hooks_misc::resolve_app_id();

    // Prefer a fresh nonce-bound ticket minted in
    // RequestEncryptedAppTicket; fall back to the static credential-store
    // ticket (titles that don't need the on-demand path keep working
    // unchanged).
    let mut ticket = G_FRESH_ETICKET.lock().unwrap().get(&app_id).cloned();
    let from_fresh = ticket.is_some();
    if ticket.is_none() {
        let t = app_ticket::get_encrypted_ticket_from_credential_store(app_id);
        if !t.is_empty() { ticket = Some(t); }
    }
    let ticket = match ticket {
        None => {
            LOG_IPC_DEBUG!("GetEncryptedAppTicket: AppId={} - no cached eticket, skip", app_id);
            return;
        }
        Some(t) => t,
    };
    LOG_IPC_DEBUG!(
        "GetEncryptedAppTicket: AppId={} serving source={}",
        app_id, if from_fresh { "fresh" } else { "store" },
    );

    let ticket_size = ticket.len() as u32;
    let cur_capacity = (*p_write).m_Memory.m_nAllocationCount;
    let new_capacity = cur_capacity.wrapping_add(ticket_size);
    if !hooks_misc::ensure_buffer_capacity(p_write, new_capacity, true) {
        LOG_IPC_DEBUG!("GetEncryptedAppTicket: AppId={} - failed to ensure buffer size", app_id);
        return;
    }

    let resp = GetEncryptedAppTicketResp::from_cutl(p_write);
    if !resp.ok() { return; }
    resp.set_return_value(true);
    resp.set_pcb_ticket(ticket_size);
    if !resp.set_p_ticket(&ticket) { return; }
    LOG_IPC_DEBUG!("GetEncryptedAppTicket: AppId={} pcbTicket={}", app_id, ticket_size);
}

// ── Register ────────────────────────────────────────────────────────

pub fn register() {
    let entries = [
        HandlerEntry {
            interface_name: "IClientUser",
            method_name: "GetSteamID",
            pre: None,
            post: Some(handler_post_get_steam_id as IpcHandlerFn),
        },
        HandlerEntry {
            interface_name: "IClientUser",
            method_name: "GetAppOwnershipTicketExtendedData",
            pre: None,
            post: Some(handler_post_get_app_ownership_ticket_extended_data as IpcHandlerFn),
        },
        HandlerEntry {
            interface_name: "IClientUser",
            method_name: "RequestEncryptedAppTicket",
            pre: None,
            post: Some(handler_post_request_encrypted_app_ticket as IpcHandlerFn),
        },
        HandlerEntry {
            interface_name: "IClientUser",
            method_name: "GetEncryptedAppTicket",
            pre: None,
            post: Some(handler_post_get_encrypted_app_ticket as IpcHandlerFn),
        },
    ];
    register_handlers(&entries);
}
