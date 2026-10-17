//! `Hooks_IPC_ISteamUtils.cpp`. Post-handler for `IClientUtils::GetAppID`
//! (spoofs the OnlineFix appid). Ticket-related handlers were removed
//! when the Denuvo backend was moved to `unused_denuvo/`.

use crate::hook::ipc::{register_handlers, HandlerEntry, IpcHandlerFn};
use crate::hook::misc as hooks_misc;
use crate::steam::ipc_facades::GetAppIDResp;
use crate::steam::structs::{CPipeClient, CUtlBuffer};

unsafe fn handler_post_get_app_id(
    _pipe: *mut CPipeClient,
    _p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
) {
    if hooks_misc::should_report_onlinefix_app_id() { return; }

    let real_app_id = hooks_misc::resolve_app_id();
    if real_app_id == 0 { return; }

    let resp = GetAppIDResp::from_cutl(p_write);
    if !resp.ok() { return; }

    let current = resp.return_value();
    if current == real_app_id { return; }
    resp.set_return_value(real_app_id);
    LOG_IPC_INFO!("GetAppID: spoof response {} -> {}", current, real_app_id);
}

pub fn register() {
    let entries = [
        HandlerEntry {
            interface_name: "IClientUtils",
            method_name: "GetAppID",
            pre: None,
            post: Some(handler_post_get_app_id as IpcHandlerFn),
        },
    ];
    register_handlers(&entries);
}