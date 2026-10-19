//! `Hooks_Misc.cpp`. OnlineFix detection and appid rewriting, plus the
//! `GetAppIDForCurrentPipe` and `GetAppDataFromAppInfo` captures that
//! the rest of the hook layer queries through [`resolve_app_id`].

use std::collections::HashMap;
use std::ffi::{c_void, CStr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::dllmain::K_ONLINE_FIX_APPID;
use crate::hook::veh_common::Int3Site;
use crate::platform::trap::Context;
use crate::steam::structs::{CGameID, CUtlBuffer};
use crate::steam::types::AppId_t;
use crate::{
    arm_capture_c, arm_int3_c, capture_ready, capture_this_fn, hook_fn,
    install_hook_c, resolve_c, resolve_fn, uninstall_hook,
};

// ── Pattern-resolved helper ─────────────────────────────────────────

resolve_fn! {
    fn CUtlBufferEnsureCapacity(
        p_cutl_buffer: *mut CUtlBuffer,
        new_capacity: u32,
    ) -> *mut c_void
}

// ── Captured-this functions ─────────────────────────────────────────

capture_this_fn! {
    fn GetAppIDForCurrentPipe(p_this: *mut c_void) -> AppId_t
}

capture_this_fn! {
    fn GetAppDataFromAppInfo(
        p_this: *mut c_void,
        app_id: AppId_t,
        key: *const i8,
        buf: *mut u8,
        buf_size: i32,
    ) -> i64
}

// ── Module state ────────────────────────────────────────────────────

/// Real AppId of the currently-launched -onlinefix game (0 = none).
static ONLINE_FIX_REAL_APPID: AtomicU32 = AtomicU32::new(0);
/// True once the game opens a SteamNetworkingSockets P2P connection.
static NETWORKING_SOCKETS_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Set by `-realappid` on the launch command line; suppresses the P2P
/// appid flip for this launch only.
static SUPPRESS_APPID_FLIP: AtomicBool = AtomicBool::new(false);

static GAME_NAME_CACHE: Lazy<Mutex<HashMap<AppId_t, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── SpawnProcess int3 callback ──────────────────────────────────────
//
// `strstr`-equivalent search on a NUL-terminated buffer. `needle` must
// not include a trailing NUL; the C++ passes a bare literal.
unsafe fn c_str_contains(haystack: *const i8, needle: &[u8]) -> bool {
    if haystack.is_null() || needle.is_empty() {
        return false;
    }
    let s = CStr::from_ptr(haystack);
    let hb = s.to_bytes();
    if hb.len() < needle.len() {
        return false;
    }
    hb.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

fn on_spawn_process_hit(ctx: &mut Context, _site: &Int3Site) {
    // Argument order per the C++:
    //   arg1 pCUser, arg2 pExePath, arg3 pCommandLine, arg4 pWorkingDir,
    //   arg5 pGameID (CGameID*; low 24 bits are the AppId).
    let p_game_id = ctx.argument(5) as *mut CGameID;
    let cmd_line = ctx.argument(3) as *const i8;
    if p_game_id.is_null() {
        return;
    }

    let (app_id, has_onlinefix, suppress) = unsafe {
        let id = (*p_game_id).app_id(true);
        let onfix = c_str_contains(cmd_line, b"-onlinefix");
        let supp = onfix && c_str_contains(cmd_line, b"-realappid");
        (id, onfix, supp)
    };

    if has_onlinefix {
        ONLINE_FIX_REAL_APPID.store(app_id, Ordering::Release);
        NETWORKING_SOCKETS_ACTIVE.store(false, Ordering::Release);
        SUPPRESS_APPID_FLIP.store(suppress, Ordering::Release);
        unsafe {
            (*p_game_id).set_app_id(K_ONLINE_FIX_APPID);
        }
        LOG_MISC_INFO!(
            "SpawnProcess: appid {} -> {}, realappid={}",
            app_id,
            K_ONLINE_FIX_APPID,
            suppress
        );
    } else {
        ONLINE_FIX_REAL_APPID.store(0, Ordering::Release);
        SUPPRESS_APPID_FLIP.store(false, Ordering::Release);
    }
}

// ── Hooked functions ────────────────────────────────────────────────

hook_fn! {
    fn OptedInMask(p_this: *mut c_void, app_id: AppId_t) -> i64 {
        let mut app_id = app_id;
        let real = ONLINE_FIX_REAL_APPID.load(Ordering::Acquire);
        if app_id == K_ONLINE_FIX_APPID && real != 0 {
            LOG_MISC_INFO!("OptedInMask: appid {} -> {}", app_id, real);
            app_id = real;
        }
        o_OptedInMask(p_this, app_id)
    }
}

hook_fn! {
    fn BuildSpawnEnvBlock(
        p_this: *mut c_void,
        p_cgame_id: *mut CGameID,
        a3: *mut c_void,
        env: *mut c_void,
        p_overlay_cgame_id: *mut CGameID,
        a6: *mut c_void,
        a7: i32,
        a8: *mut c_void,
        a9: *mut c_void,
        a10: u32,
        a11: i8,
    ) -> i64 {
        let real = ONLINE_FIX_REAL_APPID.load(Ordering::Acquire);
        if real != 0 && !p_overlay_cgame_id.is_null() {
            let overlay_app = (*p_overlay_cgame_id).app_id(true);
            if overlay_app == K_ONLINE_FIX_APPID {
                LOG_MISC_INFO!(
                    "BuildSpawnEnvBlock: SetAppID in OverlayCGameID {} -> {}",
                    overlay_app,
                    real
                );
                (*p_overlay_cgame_id).set_app_id(real);
            }
        }
        o_BuildSpawnEnvBlock(
            p_this, p_cgame_id, a3, env,
            p_overlay_cgame_id, a6, a7,
            a8, a9, a10, a11,
        )
    }
}

// `GetOrAddAppData` is declared in the C++ header and defined in the
// .cpp, but its install line is commented out in `HookManager`-adjacent
// code and remains so here. The body would consult
// `LuaConfig::HasDepot` and `CAppData::IsUnresolvedAppInfo` and set
// `bSkipFlag`; since the hook never installs, porting it is deferred
// until a reason to enable it appears.

// ── Public API ──────────────────────────────────────────────────────

pub fn get_app_id_for_current_pipe_wrap() -> AppId_t {
    if !(capture_ready!(GetAppIDForCurrentPipe)) {
        LOG_MISC_WARN!(
            "GetAppIDForCurrentPipeWrap called before capture - returning 0"
        );
        return 0;
    }
    let engine = unsafe { CAPTURED_GET_APP_I_D_FOR_CURRENT_PIPE.load() };
    let app_id = unsafe { o_GetAppIDForCurrentPipe(engine) };
    if app_id == 0 {
        LOG_MISC_TRACE!("GetAppIDForCurrentPipeWrap: AppId=0(Not GamePipe)");
    } else {
        LOG_MISC_TRACE!("GetAppIDForCurrentPipeWrap: AppId={}", app_id);
    }
    app_id
}

pub fn resolve_app_id() -> AppId_t {
    let real = ONLINE_FIX_REAL_APPID.load(Ordering::Acquire);
    if real != 0 {
        return real;
    }
    get_app_id_for_current_pipe_wrap()
}

pub fn is_onlinefix_active() -> bool {
    ONLINE_FIX_REAL_APPID.load(Ordering::Acquire) != 0
}

pub fn notify_networking_sockets_used() {
    if ONLINE_FIX_REAL_APPID.load(Ordering::Acquire) != 0
        && !NETWORKING_SOCKETS_ACTIVE.load(Ordering::Acquire)
    {
        NETWORKING_SOCKETS_ACTIVE.store(true, Ordering::Release);
        LOG_MISC_INFO!(
            "NetworkingSockets active: GetAppID now reports 480 for cert match"
        );
    }
}

pub fn should_report_onlinefix_app_id() -> bool {
    // The flip exists so a P2P socket's appid matches the 480 session
    // cert, which some titles need. It is blunt though: from the moment
    // it trips, every GetAppID answer is the fake appid for the rest of
    // the process's life. Games that ask Steam for their own appid during
    // later startup then get 480 and misbehave. Both behaviours are
    // needed by different games, and the call itself gives no way to
    // tell them apart, so `-realappid` opts out per launch.
    if SUPPRESS_APPID_FLIP.load(Ordering::Acquire) {
        return false;
    }
    ONLINE_FIX_REAL_APPID.load(Ordering::Acquire) != 0
        && NETWORKING_SOCKETS_ACTIVE.load(Ordering::Acquire)
}

pub fn ensure_buffer_capacity(
    p_write: *mut CUtlBuffer,
    new_capacity: u32,
    update_put: bool,
) -> bool {
    if unsafe { O_C_UTL_BUFFER_ENSURE_CAPACITY.is_null() } {
        LOG_MISC_WARN!("EnsureBufferCapacity: oCUtlBufferEnsureCapacity not resolved");
        return false;
    }
    if !p_write.is_null() {
        let cap = unsafe { (*p_write).m_Memory.m_nAllocationCount };
        LOG_MISC_DEBUG!(
            "Before ensuring CUtlBuffer capacity: cap={} put={} get={}",
            cap,
            unsafe { (*p_write).m_Put },
            unsafe { (*p_write).m_Get },
        );
    }
    unsafe {
        o_CUtlBufferEnsureCapacity(p_write, new_capacity);
        if update_put && !p_write.is_null() {
            (*p_write).m_Put = new_capacity as i32;
        }
        if !p_write.is_null() {
            let cap = (*p_write).m_Memory.m_nAllocationCount;
            LOG_MISC_DEBUG!(
                "After ensuring CUtlBuffer capacity: cap={} put={} get={}",
                cap,
                (*p_write).m_Put,
                (*p_write).m_Get,
            );
        }
    }
    true
}

pub fn get_game_name_by_app_id(app_id: AppId_t) -> String {
    if let Some(cached) = GAME_NAME_CACHE.lock().get(&app_id).cloned() {
        return cached;
    }

    // Only cache successful lookups. If the capture has not fired yet,
    // return empty without caching so the next call retries. This
    // prevents a transient early call from permanently poisoning the
    // cache with an empty name.
    if !capture_ready!(GetAppDataFromAppInfo) {
        LOG_MISC_DEBUG!(
            "GetGameNameByAppID({}) called before GetAppDataFromAppInfo capture",
            app_id,
        );
        return String::new();
    }

    let mut name = String::new();
    let mut buf = [0u8; 256];
    // "common/name" triggers auto-localization in the C++ helper:
    // prefix "common" (keyType=2) + key "name", then the helper tries
    // "name_localized/<current_lang>" before falling back to "name".
    // Returns strlen+1 on success, -1 on failure.
    let key = b"common/name\0";
    let p_this = unsafe { CAPTURED_GET_APP_DATA_FROM_APP_INFO.load() };
    let len = unsafe {
        o_GetAppDataFromAppInfo(
            p_this,
            app_id,
            key.as_ptr() as *const i8,
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    if len > 1 {
        let n = (len - 1) as usize;
        if n <= buf.len() {
            name = String::from_utf8_lossy(&buf[..n]).into_owned();
        }
    }

    LOG_MISC_DEBUG!("GetGameNameByAppID({}): {}", app_id, name);
    if !name.is_empty() {
        GAME_NAME_CACHE.lock().insert(app_id, name.clone());
    }
    name
}

pub fn install() -> bool {
    resolve_c!(CUtlBufferEnsureCapacity);

    arm_capture_c!(GetAppIDForCurrentPipe);
    arm_capture_c!(GetAppDataFromAppInfo);

    arm_int3_c!(SpawnProcess, true, on_spawn_process_hit, std::ptr::null_mut());

    let ok1 = install_hook_c!(BuildSpawnEnvBlock);
    let ok2 = install_hook_c!(OptedInMask);
    ok1 && ok2
}

pub fn uninstall() -> bool {
    let ok1 = uninstall_hook!(BuildSpawnEnvBlock);
    let ok2 = uninstall_hook!(OptedInMask);
    ok1 && ok2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_clean() {
        // Process-global state; no other test in this crate touches
        // these, so a bare read of the initial values is safe.
        assert_eq!(ONLINE_FIX_REAL_APPID.load(Ordering::Acquire), 0);
        assert!(!NETWORKING_SOCKETS_ACTIVE.load(Ordering::Acquire));
        assert!(!SUPPRESS_APPID_FLIP.load(Ordering::Acquire));
    }

    #[test]
    fn should_report_is_false_initially() {
        assert!(!should_report_onlinefix_app_id());
    }

    #[test]
    fn is_onlinefix_inactive_initially() {
        assert!(!is_onlinefix_active());
    }

    #[test]
    fn resolve_app_id_is_zero_before_capture() {
        // GetAppIDForCurrentPipe is not armed in tests, so `capture_ready!`
        // is false and the wrap returns 0 without touching any pointer.
        assert_eq!(get_app_id_for_current_pipe_wrap(), 0);
    }
}
