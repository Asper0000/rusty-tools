//! `Pipe/PipeManager.cpp`. `OnHandshake` resolves the caller once,
//! caches the process snapshot, then lets each Pipe feature react.

use std::collections::HashMap;
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::hook::misc as hooks_misc;
use crate::pipe::injection;
use crate::pipe::pipe_types::{make_pipe_key, PipeContext, PipeKey, ProcessKey};
use crate::pipe::process_inspector::{self, ProcessSnapshot};
use crate::steam::structs::CPipeClient;
use crate::steam::types::AppId_t;
use crate::utils::config::lua_config;

/// steamclient doesn't always finish binding a brand-new pipe to its
/// appid by the literal handshake instant. Observed empirically:
/// `GetAppIDForCurrentPipe` returns invalid at handshake time, then the
/// correct appid ~20 ms later once the game's first real IPC call
/// lands. `OnHandshake` only runs once per pipe, so a wrong
/// `trackedApp=false` on that single call permanently mis-tracks the
/// process. Retry briefly instead of accepting the first sample.
const K_APPID_RESOLVE_RETRIES: i32 = 10;
const K_APPID_RESOLVE_RETRY_DELAY: Duration = Duration::from_millis(20);

static G_PROCESSES: Lazy<Mutex<HashMap<ProcessKey, ProcessSnapshot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn make_process_key(snapshot: &ProcessSnapshot) -> ProcessKey {
    ProcessKey { pid: snapshot.pid, creation_time: snapshot.creation_time }
}

fn try_reuse_cached_process(pid: u32) -> Option<ProcessSnapshot> {
    let current = process_inspector::get_process_creation_time(pid)?;
    let key = ProcessKey { pid, creation_time: current };
    let g = G_PROCESSES.lock();
    g.get(&key).cloned()
}

fn resolve_process(pid: u32) -> ProcessSnapshot {
    if let Some(cached) = try_reuse_cached_process(pid) { return cached; }

    // First sighting: inspect once and cache so sibling pipes reuse it.
    // A dead process yields creation_time=0, so skip caching that junk
    // key.
    let snapshot = process_inspector::inspect_process(pid);
    let key = make_process_key(&snapshot);
    if key.is_valid() {
        G_PROCESSES.lock().insert(key, snapshot.clone());
    }
    snapshot
}

/// Env-based appid first (cheap, and a missing env var will never
/// appear no matter how long we wait, so it's only tried once). Falls
/// back to the pipe's own appid, retrying briefly since that binding
/// can lag the handshake by a few ms. Returns 0 if every attempt comes
/// up empty and the process-name config lookup also fails.
fn resolve_app_id_with_retry(
    snapshot: &ProcessSnapshot,
    out_from_pipe: &mut bool,
) -> AppId_t {
    *out_from_pipe = false;

    let env_app_id = snapshot.resolve_app_id();
    if env_app_id != 0 { return env_app_id; }

    for attempt in 0..K_APPID_RESOLVE_RETRIES {
        let pipe_app_id = hooks_misc::resolve_app_id();
        if pipe_app_id != 0 {
            *out_from_pipe = true;
            if attempt > 0 {
                LOG_PIPE_DEBUG!(
                    "PipeManager: pipe appid resolved on retry attempt={} appid={}",
                    attempt, pipe_app_id,
                );
            }
            return pipe_app_id;
        }
        std::thread::sleep(K_APPID_RESOLVE_RETRY_DELAY);
    }

    // Neither env var nor IPC pipe binding resolved an appid — the
    // game launched without SteamAppId and never called
    // IClientUtils::GetAppID in the retry window. Fall back to an
    // explicit process-name mapping from `addprocess()` in LuaConfig.
    if !snapshot.image_name.is_empty() {
        let config_app_id = lua_config::get_app_id_for_process(&snapshot.image_name);
        if config_app_id != 0 {
            LOG_PIPE_DEBUG!(
                "PipeManager: process-name config appid image={} appid={}",
                snapshot.image_name, config_app_id,
            );
            return config_app_id;
        }
    }

    0
}

/// # Safety
/// `pipe`, if non-null, must point at a live `CPipeClient`.
pub unsafe fn on_handshake(pipe: *mut CPipeClient) {
    if pipe.is_null() {
        LOG_PIPE_INFO!("PipeManager: ignore null pipe handshake");
        return;
    }

    let pipe_key = make_pipe_key(pipe);
    if pipe_key.pid == 0 {
        LOG_PIPE_DEBUG!(
            "PipeManager: ignore handshake with invalid pipe key {}",
            pipe_key.debug_string(),
        );
        return;
    }

    let snapshot = resolve_process(pipe_key.pid);
    let process_key = make_process_key(&snapshot);
    if !process_key.is_valid() {
        LOG_PIPE_DEBUG!(
            "PipeManager: ignore handshake with invalid process key {} snapshot={}",
            process_key.debug_string(), snapshot.debug_string(),
        );
        return;
    }

    // Env-based appid first; fall back to the pipe's appid (with a
    // short retry) for games that launch WITHOUT exporting SteamAppId
    // (a launcher / child process). Without this, those games never
    // get tracked and DenuvoAuth never runs.
    let mut app_id_from_pipe = false;
    let app_id = resolve_app_id_with_retry(&snapshot, &mut app_id_from_pipe);
    let tracked_app = app_id != 0 && lua_config::has_depot(app_id, false);

    // `likely_game_process` is env-derived (needs SteamAppId exported),
    // so it's false for env-less games. A pipe that resolves to a
    // configured depot is a tracked game regardless, and
    // `DenuvoAuth::Apply` requires `gameProcess && trackedApp` — so
    // treat a tracked depot as a game process even without the env.
    let game_process = snapshot.likely_game_process || tracked_app;

    let ctx = PipeContext {
        pipe,
        process: process_key,
        app_id,
        game_process,
        tracked_app,
        owned: tracked_app && lua_config::is_owned(app_id),
    };

    LOG_PIPE_INFO!(
        "PipeManager: handshake {} process={} appid={} appIdFromPipe={} gameProcess={} trackedApp={} snapshot={}",
        pipe_key.debug_string(), process_key.debug_string(), app_id,
        app_id_from_pipe, game_process, tracked_app, snapshot.debug_string(),
    );

    // Feature side effects run without holding the registry lock.
    injection::apply(&ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_pipe_is_ignored() {
        // Should not panic and should not create any state.
        unsafe { on_handshake(std::ptr::null_mut()) };
    }

    #[test]
    fn make_process_key_matches_snapshot() {
        let snap = ProcessSnapshot { pid: 5, creation_time: 10, ..Default::default() };
        assert_eq!(make_process_key(&snap), ProcessKey { pid: 5, creation_time: 10 });
    }
}
