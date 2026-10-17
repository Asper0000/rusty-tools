//! Per-process Denuvo detection + authorization state. Source:
//! `Pipe/Features/DenuvoAuth/DenuvoAuth.cpp`.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::pipe::pipe_types::{make_pipe_key, PipeContext, PipeKey, ProcessKey};
use crate::pipe::protection_scan;
use crate::platform::steam_credential_store;
use crate::steam::enums::{EAccountType, EUniverse};
use crate::steam::structs::{CPipeClient, CSteamID};
use crate::steam::types::AppId_t;
use crate::utils::config::lua_config;
use crate::utils::tickets::app_ticket;

const K_END_DENUVO_VERIFICATION_HANDSHAKE: u32 = 2;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Stage {
    None,
    Authorizing,
    EndAuthorization,
}

fn stage_str(s: Stage) -> &'static str {
    match s {
        Stage::None => "None",
        Stage::Authorizing => "Authorizing",
        Stage::EndAuthorization => "EndAuthorization",
    }
}

fn parse_universe(name: &str) -> Option<EUniverse> {
    if name.eq_ignore_ascii_case("Public") { return Some(EUniverse::Public); }
    if name.eq_ignore_ascii_case("Beta") { return Some(EUniverse::Beta); }
    if name.eq_ignore_ascii_case("Internal") { return Some(EUniverse::Internal); }
    if name.eq_ignore_ascii_case("Dev") { return Some(EUniverse::Dev); }
    None
}

fn get_current_steam_id_for_denuvo_auth() -> Option<u64> {
    let (account_id, universe_name) = match steam_credential_store::get_active_user() {
        Ok(pair) => pair,
        Err(status) => {
            LOG_PIPE_WARN!(
                "DenuvoAuth: active Steam user unavailable ({})", status.as_str(),
            );
            return None;
        }
    };
    let universe = match parse_universe(&universe_name) {
        Some(u) => u,
        None => {
            LOG_PIPE_WARN!("DenuvoAuth: active Steam user has unsupported universe");
            return None;
        }
    };
    let mut id = CSteamID::default();
    id.set(account_id, universe, EAccountType::Individual);
    Some(id.convert_to_u64())
}

#[derive(Default, Clone)]
struct ProcessAuth {
    scanned: bool,
    denuvo: bool,
    stage: Stage,
    pid: u32,
    handshake_count: u32,
    authorization_pipe: Option<PipeKey>,
    authorized_app_id: AppId_t,
}

impl ProcessAuth {
    fn new() -> Self {
        Self { stage: Stage::None, ..Default::default() }
    }

    fn debug_string(&self) -> String {
        format!(
            "denuvo={} stage={} handshakeCount={} auth_appid={} pid={}",
            self.denuvo,
            stage_str(self.stage),
            self.handshake_count,
            self.authorized_app_id,
            self.pid,
        )
    }

    fn on_handshake(&mut self, ctx: &PipeContext, pipe_key: PipeKey) {
        self.handshake_count += 1;

        if !self.denuvo {
            self.stage = Stage::None;
            return;
        }

        if self.authorization_pipe.is_none() {
            self.authorization_pipe = Some(pipe_key);
            self.authorized_app_id = ctx.app_id;
            self.pid = ctx.process.pid;
            self.stage = Stage::Authorizing;
            LOG_PIPE_INFO!("DenuvoAuth: authorization pipe selected {}", self.debug_string());
        }

        if self.stage == Stage::Authorizing
            && self.handshake_count >= K_END_DENUVO_VERIFICATION_HANDSHAKE
        {
            self.stage = Stage::EndAuthorization;
            LOG_PIPE_INFO!("DenuvoAuth: authorization window ended {}", self.debug_string());
            if lua_config::is_owned(self.authorized_app_id) {
                self.write_steam_id_on_end_authorization();
            }
        }
    }

    fn can_use_authorized_identity(&self, key: PipeKey) -> bool {
        self.denuvo
            && self.stage == Stage::Authorizing
            && self.authorization_pipe == Some(key)
    }

    fn write_steam_id_on_end_authorization(&self) {
        if self.authorized_app_id == 0 {
            LOG_PIPE_WARN!(
                "DenuvoAuth: end authorization skipped SteamID persist without auth app",
            );
            return;
        }
        let Some(steam_id) = get_current_steam_id_for_denuvo_auth() else { return; };
        if steam_id == 0 { return; }

        if app_ticket::write_steam_id(self.authorized_app_id, steam_id) {
            LOG_PIPE_INFO!(
                "DenuvoAuth: persisted end-authorization SteamID auth_appid={} steamid={}",
                self.authorized_app_id, steam_id,
            );
        } else {
            LOG_PIPE_WARN!(
                "DenuvoAuth: failed to persist end-authorization SteamID auth_appid={} steamid={}",
                self.authorized_app_id, steam_id,
            );
        }
    }
}

impl Default for Stage {
    fn default() -> Self { Stage::None }
}

static G_PROCESS_AUTH: Lazy<Mutex<HashMap<ProcessKey, ProcessAuth>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static G_PIPE_PROCESS: Lazy<Mutex<HashMap<PipeKey, ProcessKey>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn find_auth_for_pipe(pipe_key: PipeKey) -> Option<ProcessAuth> {
    let process_key = {
        let g = G_PIPE_PROCESS.lock();
        *g.get(&pipe_key)?
    };
    G_PROCESS_AUTH.lock().get(&process_key).cloned()
}

fn ensure_scanned(auth: &mut ProcessAuth, process: &ProcessKey, app_id: AppId_t) {
    if auth.scanned {
        LOG_PIPE_TRACE!(
            "DenuvoAuth: reusing cached protection result {} denuvo={}",
            process.debug_string(), auth.denuvo,
        );
        return;
    }
    auth.scanned = true;
    if lua_config::is_forced_denuvo(app_id) {
        auth.denuvo = true;
        LOG_PIPE_INFO!(
            "DenuvoAuth: forcedenuvo appid={} - skipping ProtectionScan", app_id,
        );
    } else {
        auth.denuvo = protection_scan::scan_protection(process.pid).denuvo_detected;
    }
    if !auth.denuvo {
        auth.stage = Stage::None;
    }
}

/// Entry point from `PipeManager::OnHandshake`.
///
/// # Safety
/// `ctx.pipe`, if non-null, must point at a live `CPipeClient`.
pub unsafe fn apply(ctx: &PipeContext) {
    if !ctx.game_process || !ctx.tracked_app { return; }

    let pipe_key = make_pipe_key(ctx.pipe);
    if !pipe_key.is_valid() { return; }

    // Register the pipe before touching the auth map. A concurrent
    // is_authorized_pipe caller can then see the pipe but not yet find
    // its auth entry, which reads as "not yet in a window" - never a
    // partially-initialized entry. This also means apply() never holds
    // both locks at once.
    G_PIPE_PROCESS.lock().insert(pipe_key, ctx.process);

    let mut auths = G_PROCESS_AUTH.lock();
    let auth = auths.entry(ctx.process).or_insert_with(ProcessAuth::new);
    ensure_scanned(auth, &ctx.process, ctx.app_id);
    auth.on_handshake(ctx, pipe_key);
}

/// True only while the pipe is the selected authorization pipe and
/// Denuvo has not reached the end-authorization handshake.
pub fn is_authorized_pipe(pipe: *const CPipeClient) -> bool {
    if pipe.is_null() {
        LOG_PIPE_TRACE!("DenuvoAuth: pipe not in authorization window: null pipe");
        return false;
    }
    let pipe_key = unsafe { make_pipe_key(pipe) };
    let Some(auth) = find_auth_for_pipe(pipe_key) else {
        LOG_PIPE_TRACE!(
            "DenuvoAuth: pipe not tracked by DenuvoAuth {}", pipe_key.debug_string(),
        );
        return false;
    };
    if !auth.can_use_authorized_identity(pipe_key) {
        LOG_PIPE_TRACE!(
            "DenuvoAuth: pipe not in authorization window {} {}",
            pipe_key.debug_string(), auth.debug_string(),
        );
        return false;
    }
    LOG_PIPE_INFO!(
        "DenuvoAuth: pipe in authorization window {} {}",
        pipe_key.debug_string(), auth.debug_string(),
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_universe_covers_all_known() {
        assert_eq!(parse_universe("Public"), Some(EUniverse::Public));
        assert_eq!(parse_universe("public"), Some(EUniverse::Public));
        assert_eq!(parse_universe("BETA"), Some(EUniverse::Beta));
        assert_eq!(parse_universe("Internal"), Some(EUniverse::Internal));
        assert_eq!(parse_universe("Dev"), Some(EUniverse::Dev));
        assert_eq!(parse_universe("Nope"), None);
    }

    #[test]
    fn stage_strings() {
        assert_eq!(stage_str(Stage::None), "None");
        assert_eq!(stage_str(Stage::Authorizing), "Authorizing");
        assert_eq!(stage_str(Stage::EndAuthorization), "EndAuthorization");
    }

    #[test]
    fn process_auth_new_is_default() {
        let a = ProcessAuth::new();
        assert!(!a.denuvo);
        assert_eq!(a.stage, Stage::None);
        assert_eq!(a.handshake_count, 0);
    }
}
