//! `Hooks_IPC.cpp`. Hooks `IPCProcessMessage` and dispatches registered
//! pre/post handlers by `(interfaceID, funcHash)`.

use std::ffi::c_void;
use std::sync::Mutex;

use once_cell::sync::Lazy;

use crate::hook::misc as hooks_misc;
use crate::steam::ipc_facades::{IPCInterfaceCall, IPCHandshakeReq, IPCRequest};
use crate::steam::ipc_messages::{EIPCCommand, EIPCInterface};
use crate::steam::structs::{CPipeClient, CUtlBuffer};
use crate::steam::types::HSteamPipe;
use crate::utils::config::lua_config;
use crate::utils::steam_metadata::ipc_loader;
use crate::{hook_fn, install_hook_c, resolve_c, resolve_fn, uninstall_hook};

// ── Handler signature ───────────────────────────────────────────────

pub type IpcHandlerFn = unsafe fn(
    pipe: *mut CPipeClient,
    p_read: *mut CUtlBuffer,
    p_write: *mut CUtlBuffer,
);

pub struct HandlerEntry {
    pub interface_name: &'static str,
    pub method_name: &'static str,
    pub pre: Option<IpcHandlerFn>,
    pub post: Option<IpcHandlerFn>,
}

impl HandlerEntry {
    pub fn debug_string(&self) -> String {
        format!(
            "{}::{} pre={} post={}",
            self.interface_name,
            self.method_name,
            if self.pre.is_some() { "yes" } else { "no" },
            if self.post.is_some() { "yes" } else { "no" },
        )
    }
}

// ── Resolved handler ────────────────────────────────────────────────

#[derive(Copy, Clone)]
struct ResolvedHandler {
    interface_id: u8,
    func_hash: u32,
    pre: Option<IpcHandlerFn>,
    post: Option<IpcHandlerFn>,
}

static G_HANDLERS: Lazy<Mutex<Vec<ResolvedHandler>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

// ── Pipe getter ─────────────────────────────────────────────────────

resolve_fn! {
    fn GetPipeClient(p_engine: *mut c_void, h_steam_pipe: HSteamPipe) -> *mut CPipeClient
}

unsafe fn get_pipe(p_server: *mut c_void, h_steam_pipe: HSteamPipe) -> *mut CPipeClient {
    if O_GET_PIPE_CLIENT.is_null() { return std::ptr::null_mut(); }
    o_GetPipeClient(p_server, h_steam_pipe)
}

// ── Dispatch ────────────────────────────────────────────────────────

#[derive(Copy, Clone)]
struct IpcDispatch {
    pipe: *mut CPipeClient,
    pre: Option<IpcHandlerFn>,
    post: Option<IpcHandlerFn>,
    handler_found: bool,
}

impl IpcDispatch {
    fn empty() -> Self {
        Self { pipe: std::ptr::null_mut(), pre: None, post: None, handler_found: false }
    }
    fn enabled(&self) -> bool { !self.pipe.is_null() && self.handler_found }
}

fn find_handler(iface: u8, func_hash: u32) -> Option<ResolvedHandler> {
    let g = G_HANDLERS.lock().expect("ipc handlers mutex poisoned");
    g.iter().find(|h| h.interface_id == iface && h.func_hash == func_hash).copied()
}

unsafe fn resolve_dispatch(
    p_server: *mut c_void,
    h_steam_pipe: HSteamPipe,
    p_read: *mut CUtlBuffer,
) -> IpcDispatch {
    let mut d = IpcDispatch::empty();
    d.pipe = get_pipe(p_server, h_steam_pipe);
    if d.pipe.is_null() { return d; }

    let req = IPCRequest::from_cutl(p_read);
    if !req.ok() { return d; }
    if req.command_raw() != EIPCCommand::InterfaceCall as u8 { return d; }

    // Ignore calls when appId is not resolved or not in the Lua config.
    if !lua_config::has_depot(hooks_misc::resolve_app_id(), false) { return d; }

    let call = IPCInterfaceCall::from_body(req.body());
    if !call.ok() { return d; }

    if let Some(h) = find_handler(call.interface_id_raw(), call.func_hash()) {
        d.pre = h.pre;
        d.post = h.post;
        d.handler_found = true;
    }
    d
}

// ── Handshake ───────────────────────────────────────────────────────

unsafe fn handle_handshake(
    p_server: *mut c_void,
    h_steam_pipe: HSteamPipe,
    p_read: *mut CUtlBuffer,
) {
    let req = IPCRequest::from_cutl(p_read);
    if !req.ok() { return; }
    if req.command_raw() != EIPCCommand::Handshake as u8 { return; }

    let hs = IPCHandshakeReq::from_body(req.body());
    if !hs.ok() { return; }

    let pipe = get_pipe(p_server, h_steam_pipe);
    if pipe.is_null() { return; }

    // set client PID
    (*pipe).m_clientPID = hs.pid();

    LOG_IPC_DEBUG!(
        "Received handshake from pipe={:p} pid={} tid={} ver={}",
        pipe, hs.pid(), hs.tid(), hs.version(),
    );
    crate::bridge::pipe_manager_on_handshake(pipe);
}

// ── NetworkingSockets detection ─────────────────────────────────────

unsafe fn detect_networking_sockets(p_read: *mut CUtlBuffer) {
    if !hooks_misc::is_onlinefix_active() { return; }
    if hooks_misc::should_report_onlinefix_app_id() { return; }

    let req = IPCRequest::from_cutl(p_read);
    if !req.ok() { return; }
    if req.command_raw() != EIPCCommand::InterfaceCall as u8 { return; }

    let call = IPCInterfaceCall::from_body(req.body());
    if !call.ok() { return; }
    if call.interface_id_raw() == EIPCInterface::IClientNetworkingSocketsSerialized as u8 {
        hooks_misc::notify_networking_sockets_used();
    }
}

// ── Hook ────────────────────────────────────────────────────────────

hook_fn! {
    fn IPCProcessMessage(
        p_server: *mut c_void,
        h_steam_pipe: HSteamPipe,
        p_read: *mut CUtlBuffer,
        p_write: *mut CUtlBuffer,
    ) -> bool {
        handle_handshake(p_server, h_steam_pipe, p_read);
        detect_networking_sockets(p_read);

        let dispatch = resolve_dispatch(p_server, h_steam_pipe, p_read);
        if !dispatch.enabled() {
            return o_IPCProcessMessage(p_server, h_steam_pipe, p_read, p_write);
        }

        if let Some(pre) = dispatch.pre {
            pre(dispatch.pipe, p_read, p_write);
        }

        let result = o_IPCProcessMessage(p_server, h_steam_pipe, p_read, p_write);

        if result {
            if let Some(post) = dispatch.post {
                post(dispatch.pipe, p_read, p_write);
            }
        }
        result
    }
}

// ── Public API ──────────────────────────────────────────────────────

pub fn install() -> bool {
    resolve_c!(GetPipeClient);
    crate::hook::ipc_isteamutils::register();

    {
        let g = G_HANDLERS.lock().expect("ipc handlers mutex poisoned");
        LOG_IPC_INFO!("Hooks_IPC: {} handlers registered", g.len());
    }

    install_hook_c!(IPCProcessMessage)
}

pub fn uninstall() -> bool {
    uninstall_hook!(IPCProcessMessage)
}

pub fn register_handlers(entries: &[HandlerEntry]) {
    let mut g = G_HANDLERS.lock().expect("ipc handlers mutex poisoned");
    for e in entries {
        match ipc_loader::find_by_name(e.interface_name, e.method_name) {
            None => {
                LOG_IPC_WARN!("[Handler Disabled] no IPC spec for {}", e.debug_string());
            }
            Some(m) => {
                g.push(ResolvedHandler {
                    interface_id: m.interface_id,
                    func_hash: m.func_hash,
                    pre: e.pre,
                    post: e.post,
                });
                LOG_IPC_DEBUG!(
                    "Hooks_IPC: resolved {} -> hash=0x{:08X} fencepost=0x{:08X} argc={}",
                    e.debug_string(), m.func_hash, m.fencepost, m.argc,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_entry_debug_string() {
        let e = HandlerEntry {
            interface_name: "IClientUser",
            method_name: "GetSteamID",
            pre: None,
            post: None,
        };
        assert_eq!(e.debug_string(), "IClientUser::GetSteamID pre=no post=no");
    }

    #[test]
    fn find_handler_empty_registry_returns_none() {
        // The registry is process-global. Other tests in this crate do
        // not register handlers, so a fresh lookup on an unused hash
        // returns None.
        assert!(find_handler(1, 0xDEAD_BEEF).is_none());
    }
}
