//! Cloud save redirection host. Source: `CloudRedirectHost.cpp`.
//!
//! Loads `cloud_redirect.dll` from the Steam directory, resolves its
//! `CR_*` exports, and exposes wrappers the NetPacket hook will use to
//! answer `Cloud.*` RPCs locally. All entry points are no-ops unless
//! `[cloud].enabled` is set and the DLL loaded successfully.
//!
//! The `CR_*` ABI is declared locally, matching the C++ which declared
//! it inline rather than pulling in CloudRedirect's headers.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use crate::platform::dynamic_library::{self, ModuleHandle};
use crate::utils::config::{config, lua_config};

// ── CR_* ABI ────────────────────────────────────────────────────────

type CrNotifyFn = extern "C" fn(i32, *const i8, *const i8);

type CrInitCloudSaveFn = extern "C" fn(*const i8, CrNotifyFn) -> bool;
type CrHandleCloudRpcFn = extern "C" fn(
    *const i8, u32, u32,
    *const u8, u32,
    *mut u8, u32,
    *mut u32, *mut i32,
) -> bool;
type CrSetAppsFn = extern "C" fn(*const u32, u32);
type CrIsAppFn = extern "C" fn(u32) -> bool;
type CrShutdownFn = extern "C" fn();
type CrEnableStatsSyncFn = extern "C" fn(bool, bool);
type CrSetAccountIdFn = extern "C" fn(u32);
type CrNotifyAppRunningFn = extern "C" fn(u32, bool);
type CrNotifyStatsStoredFn = extern "C" fn(u32);

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct AchievementBlock {
    pub stat_id: u32,
    pub bits: u32,
    pub unlock_times: [u32; 32],
}

type CrGetAchievementsFn = extern "C" fn(u32, *mut AchievementBlock, u32) -> u32;
type CrInstallVtableHooksFn = extern "C" fn() -> bool;

// ── Globals ─────────────────────────────────────────────────────────

struct State {
    active: bool,
    module: ModuleHandle,
    init_cloud_save: Option<CrInitCloudSaveFn>,
    handle_cloud_rpc: Option<CrHandleCloudRpcFn>,
    set_apps: Option<CrSetAppsFn>,
    is_app: Option<CrIsAppFn>,
    shutdown: Option<CrShutdownFn>,
    enable_stats_sync: Option<CrEnableStatsSyncFn>,
    set_account_id: Option<CrSetAccountIdFn>,
    notify_app_running: Option<CrNotifyAppRunningFn>,
    notify_stats_stored: Option<CrNotifyStatsStoredFn>,
    get_achievements: Option<CrGetAchievementsFn>,
    install_vtable_hooks: Option<CrInstallVtableHooksFn>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn with_state<R>(f: impl FnOnce(&State) -> R) -> Option<R> {
    let g = STATE.lock().expect("poisoned");
    g.as_ref().map(f)
}

// ── Notify shim ─────────────────────────────────────────────────────

extern "C" fn cloud_notify(level: i32, title: *const i8, message: *const i8) {
    let t = if title.is_null() { "CloudRedirect".to_string() }
            else { unsafe { std::ffi::CStr::from_ptr(title) }.to_string_lossy().into_owned() };
    let m = if message.is_null() { String::new() }
            else { unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy().into_owned() };
    match level {
        2 => LOG_ERROR!("[CloudRedirect] {}: {}", t, m),
        1 => LOG_WARN!("[CloudRedirect] {}: {}", t, m),
        _ => LOG_INFO!("[CloudRedirect] {}: {}", t, m),
    }
}

// ── Symbol resolution ───────────────────────────────────────────────

fn resolve_symbol<T: Copy>(module: ModuleHandle, name: &str, out: &mut Option<T>) -> bool {
    let ptr = unsafe { dynamic_library::get_symbol(module, name) };
    if ptr.is_null() {
        LOG_WARN!("CloudRedirect: export {} not found in cloud_redirect.dll", name);
        return false;
    }
    *out = Some(unsafe { std::mem::transmute_copy(&ptr) });
    true
}

fn resolve_optional<T: Copy>(module: ModuleHandle, name: &str, out: &mut Option<T>) {
    let ptr = unsafe { dynamic_library::get_symbol(module, name) };
    if !ptr.is_null() {
        *out = Some(unsafe { std::mem::transmute_copy(&ptr) });
    }
}

fn resolve_library_path(steam_root: &str, configured: &str) -> PathBuf {
    if configured.is_empty() {
        return PathBuf::from(steam_root).join("cloud_redirect.dll");
    }
    let p = Path::new(configured);
    if p.is_absolute() { p.to_path_buf() } else { PathBuf::from(steam_root).join(p) }
}

// ── Public API ──────────────────────────────────────────────────────

pub fn initialize(steam_install_path: &str) {
    let cloud = config::get_cloud_settings();
    if !cloud.enabled {
        LOG_INFO!("CloudRedirect: [cloud].enabled is false, cloud save redirection disabled");
        return;
    }
    if steam_install_path.is_empty() {
        LOG_WARN!("CloudRedirect: empty Steam install path, cannot initialise");
        return;
    }

    if ACTIVE.load(Ordering::Acquire) { return; }

    let lib_path = resolve_library_path(steam_install_path, &cloud.library);
    if !lib_path.exists() {
        LOG_WARN!("CloudRedirect: cloud_redirect.dll not found at {}", lib_path.display());
        return;
    }

    let module = dynamic_library::load(&lib_path);
    if module.is_null() {
        LOG_WARN!("CloudRedirect: failed to load {} (err={})",
                  lib_path.display(), dynamic_library::get_last_error_code());
        return;
    }

    let mut state = State {
        active: false,
        module,
        init_cloud_save: None,
        handle_cloud_rpc: None,
        set_apps: None,
        is_app: None,
        shutdown: None,
        enable_stats_sync: None,
        set_account_id: None,
        notify_app_running: None,
        notify_stats_stored: None,
        get_achievements: None,
        install_vtable_hooks: None,
    };

    let mut ok = true;
    ok &= resolve_symbol(module, "CR_InitCloudSave",  &mut state.init_cloud_save);
    ok &= resolve_symbol(module, "CR_HandleCloudRpc", &mut state.handle_cloud_rpc);
    ok &= resolve_symbol(module, "CR_SetApps",        &mut state.set_apps);
    ok &= resolve_symbol(module, "CR_IsApp",          &mut state.is_app);
    ok &= resolve_symbol(module, "CR_Shutdown",       &mut state.shutdown);
    if !ok {
        LOG_WARN!("CloudRedirect: cloud_redirect.dll is missing required exports, disabling");
        return;
    }

    resolve_optional(module, "CR_EnableStatsSync",    &mut state.enable_stats_sync);
    resolve_optional(module, "CR_SetAccountId",       &mut state.set_account_id);
    resolve_optional(module, "CR_NotifyAppRunning",   &mut state.notify_app_running);
    resolve_optional(module, "CR_NotifyStatsStored",  &mut state.notify_stats_stored);
    resolve_optional(module, "CR_GetAchievements",    &mut state.get_achievements);
    resolve_optional(module, "CR_InstallVtableHooks", &mut state.install_vtable_hooks);

    let steam_path_c = match std::ffi::CString::new(steam_install_path) {
        Ok(c) => c,
        Err(_) => { LOG_WARN!("CloudRedirect: Steam path has interior NUL"); return; }
    };

    let init = state.init_cloud_save.unwrap();
    if !init(steam_path_c.as_ptr() as *const i8, cloud_notify) {
        LOG_WARN!("CloudRedirect: CR_InitCloudSave failed, disabling cloud save redirection");
        return;
    }

    state.active = true;
    LOG_INFO!("CloudRedirect: loaded {} and initialised cloud save redirection",
              lib_path.display());

    if let Some(enable) = state.enable_stats_sync {
        enable(true, true);
        LOG_INFO!("CloudRedirect: stats sync registered");
    }

    let depots = lua_config::get_all_depot_ids();
    let set_apps = state.set_apps.unwrap();
    set_apps(
        if depots.is_empty() { std::ptr::null() } else { depots.as_ptr() },
        depots.len() as u32,
    );
    LOG_INFO!("CloudRedirect: registered {} redirected app(s)", depots.len());

    if let Some(install) = state.install_vtable_hooks {
        if install() {
            LOG_INFO!("CloudRedirect: vtable hooks installed");
        } else {
            LOG_WARN!("CloudRedirect: vtable hook install failed, using packet-layer path");
        }
    }

    *STATE.lock().expect("poisoned") = Some(state);
    ACTIVE.store(true, Ordering::Release);
}

pub fn sync_app_set() {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    let depots = lua_config::get_all_depot_ids();
    with_state(|s| {
        if let Some(set_apps) = s.set_apps {
            set_apps(
                if depots.is_empty() { std::ptr::null() } else { depots.as_ptr() },
                depots.len() as u32,
            );
            LOG_DEBUG!("CloudRedirect: re-synced redirected app set ({} app(s))", depots.len());
        }
    });
}

pub fn is_active() -> bool { ACTIVE.load(Ordering::Acquire) }

pub fn is_app(app_id: u32) -> bool {
    if !ACTIVE.load(Ordering::Acquire) { return false; }
    with_state(|s| s.is_app.map(|f| f(app_id)).unwrap_or(false)).unwrap_or(false)
}

pub fn handle_cloud_rpc(
    method: &str,
    app_id: u32,
    account_id: u32,
    req_body: &[u8],
    resp_buf: &mut [u8],
) -> Option<(usize, i32)> {
    if !ACTIVE.load(Ordering::Acquire) { return None; }
    let method_c = std::ffi::CString::new(method).ok()?;
    with_state(|s| {
        let f = s.handle_cloud_rpc?;
        let mut resp_len: u32 = 0;
        let mut eresult: i32 = 2; // k_EResultFail default
        let ok = f(
            method_c.as_ptr() as *const i8,
            app_id,
            account_id,
            req_body.as_ptr(),
            req_body.len() as u32,
            resp_buf.as_mut_ptr(),
            resp_buf.len() as u32,
            &mut resp_len,
            &mut eresult,
        );
        if ok { Some((resp_len as usize, eresult)) } else { None }
    }).flatten()
}

pub fn set_account_id(account_id: u32) {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    with_state(|s| { if let Some(f) = s.set_account_id { f(account_id); } });
}

pub fn notify_app_running(app_id: u32, running: bool) {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    with_state(|s| { if let Some(f) = s.notify_app_running { f(app_id, running); } });
}

pub fn notify_stats_stored(app_id: u32) {
    if !ACTIVE.load(Ordering::Acquire) { return; }
    with_state(|s| { if let Some(f) = s.notify_stats_stored { f(app_id); } });
}

pub fn get_achievements(app_id: u32, out: &mut [AchievementBlock]) -> usize {
    if !ACTIVE.load(Ordering::Acquire) { return 0; }
    with_state(|s| {
        s.get_achievements
            .map(|f| f(app_id, out.as_mut_ptr(), out.len() as u32) as usize)
            .unwrap_or(0)
    }).unwrap_or(0)
}

pub fn shutdown() {
    if !ACTIVE.swap(false, Ordering::AcqRel) { return; }
    let taken = STATE.lock().expect("poisoned").take();
    if let Some(s) = taken {
        if let Some(f) = s.shutdown { f(); }
        LOG_INFO!("CloudRedirect: shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_library_path_bare_name() {
        let p = resolve_library_path("C:\\Steam", "");
        assert_eq!(p, PathBuf::from("C:\\Steam").join("cloud_redirect.dll"));
    }

    #[test]
    fn resolve_library_path_absolute_wins() {
        let p = resolve_library_path("C:\\Steam", "D:\\other\\cr.dll");
        assert_eq!(p, PathBuf::from("D:\\other\\cr.dll"));
    }

    #[test]
    fn resolve_library_path_relative_joins() {
        let p = resolve_library_path("C:\\Steam", "bin\\cr.dll");
        assert_eq!(p, PathBuf::from("C:\\Steam").join("bin\\cr.dll"));
    }

    #[test]
    fn inactive_is_app_is_false() {
        // Global state may have been initialized by another test in a
        // shared-process run. Guard with the active check.
        if !is_active() {
            assert!(!is_app(12345));
        }
    }
}
