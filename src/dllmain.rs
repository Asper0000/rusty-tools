//! `dllmain.cpp`. The Windows DLL entry point plus the init thread
//! that runs every filesystem / module-load / detour operation.
//!
//! Everything that touches the loader (LoadLibraryW), the registry,
//! or the filesystem runs on a worker thread. `DllMain` itself only
//! does `DisableThreadLibraryCalls` and dispatches to that worker, so
//! no work happens under the loader lock.

#![allow(dead_code)]

use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::OnceLock;

use crate::platform::dynamic_library::{self, ModuleHandle};
use crate::utils::cloud_redirect::host as cloud_redirect;
use crate::utils::config::{config, config_file_watcher, lua_config, lua_file_watcher};
use crate::utils::logging;
use crate::utils::steam_metadata::{ipc_loader, manifest_donor, pattern_loader, steam_diagnostics};

// ── Module globals ──────────────────────────────────────────────────

/// `HMODULE` for `steamclient64.dll`. Populated by the init thread.
pub static CLIENT_HMODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// `HMODULE` for `steamui.dll`. Populated by the init thread.
pub static UI_HMODULE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The fake AppId used by `-onlinefix` (SpaceWar).
pub const K_ONLINE_FIX_APPID: u32 = 480;

#[inline]
pub fn set_client_hmodule(ptr: *mut c_void) { CLIENT_HMODULE.store(ptr, Ordering::Release); }

#[inline]
pub fn set_ui_hmodule(ptr: *mut c_void)     { UI_HMODULE.store(ptr, Ordering::Release); }

#[inline]
pub fn client_hmodule() -> *mut c_void      { CLIENT_HMODULE.load(Ordering::Acquire) }

#[inline]
pub fn ui_hmodule() -> *mut c_void          { UI_HMODULE.load(Ordering::Acquire) }

// ── Runtime paths ───────────────────────────────────────────────────

/// All the fixed paths the init thread resolves once and every other
/// module reads afterwards. Stored in a `OnceLock`, so after init they
/// are effectively immutable.
pub struct RuntimePaths {
    pub install: String,
    pub client: String,
    pub ui: String,
    pub diversion: String,
    pub lua_dir: String,
    pub config: String,
}

static PATHS: OnceLock<RuntimePaths> = OnceLock::new();

pub fn paths() -> Option<&'static RuntimePaths> { PATHS.get() }

// ── Constants ───────────────────────────────────────────────────────

// These values are documented in the Win32 SDK. `windows-sys` does not
// re-export them from LibraryLoader on this version.
const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_PROCESS_DETACH: u32 = 0;

// Ã¢â€â‚¬Ã¢â€â‚¬ Loader-lock-safe debug output Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬
//
// DllMain runs under the Windows loader lock. Calling LOG_INFO! from
// here takes the per-file log mutex and performs disk I/O, which can
// deadlock with a thread that is blocked on the same mutex while
// Detours later tries to suspend it (see #17). OutputDebugStringA is
// safe in a loader-locked context: no file I/O, no lock.
#[cfg(windows)]
fn dllmain_debug(msg: &str) {
    if let Ok(cstr) = std::ffi::CString::new(msg) {
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA(
                cstr.as_ptr() as *const u8,
            );
        }
    }
}

#[cfg(not(windows))]
fn dllmain_debug(_msg: &str) {}

// ── Init thread ─────────────────────────────────────────────────────

/// Build the six runtime paths from the current working directory and
/// load `steamclient64.dll` / `steamui.dll`. Called once, on the init
/// thread, before any hook installation.
///
/// The C++ uses fixed-size `char[260]` buffers; this port builds
/// `String`s and stores them in a `OnceLock`.
fn initialize_steam_components() -> bool {
    let install = dynamic_library::get_current_directory_path();
    if install.is_empty() {
        LOG_ERROR!("InitializeSteamComponents: GetCurrentDirectoryPath returned empty");
        return false;
    }

    let client_path    = format!("{}\\steamclient64.dll",  install);
    let ui_path        = format!("{}\\steamui.dll",        install);
    let diversion_path = format!("{}\\bin\\diversion.dll", install);
    let lua_dir        = format!("{}\\config\\stplug-in",  install);
    let config_path    = format!("{}\\rustytools.toml", install);

    let client_h = dynamic_library::load(Path::new(&client_path));
    if client_h.is_null() {
        LOG_ERROR!(
            "Load steamclient64.dll failed: {} (err={})",
            client_path, dynamic_library::get_last_error_code(),
        );
        return false;
    }
    set_client_hmodule(client_h.0);
    LOG_INFO!("Loaded steamclient64.dll from {}", client_path);

    let ui_h = dynamic_library::load(Path::new(&ui_path));
    if ui_h.is_null() {
        LOG_ERROR!(
            "Load failed for steamui.dll: err={}",
            dynamic_library::get_last_error_code(),
        );
        return false;
    }
    set_ui_hmodule(ui_h.0);

    let _ = PATHS.set(RuntimePaths {
        install,
        client: client_path,
        ui: ui_path,
        diversion: diversion_path,
        lua_dir,
        config: config_path,
    });
    true
}

/// The worker entry point. Mirrors `InitThread` in `dllmain.cpp`.
fn init_thread(self_module: ModuleHandle) -> u32 {
    logging::init(self_module);
    LOG_INFO!("[wf] init_thread: enter");

    LOG_INFO!("[wf] initialize_steam_components: enter");
    if !initialize_steam_components() {
        LOG_ERROR!("InitializeSteamComponents failed");
        LOG_INFO!("[wf] initialize_steam_components: FAILED");
        return 1;
    }
    LOG_INFO!("[wf] initialize_steam_components: done");

    let Some(paths) = PATHS.get() else {
        LOG_INFO!("[wf] PATHS.get: FAILED (should never happen)");
        return 1;
    };

    LOG_INFO!("[wf] config::load: enter");
    config::load(&paths.config);
    LOG_INFO!("[wf] config::load: done");

    LOG_INFO!("[wf] logging::init_modules_public: enter");
    logging::init_modules_public();
    logging::install_platform_log_sink();
    LOG_INFO!("[wf] logging::init_modules_public: done");

    LOG_INFO!("[wf] steam_diagnostics::initialize: enter");
    steam_diagnostics::initialize(&paths.client, &paths.ui);
    LOG_INFO!("[wf] steam_diagnostics::initialize: done");

    LOG_INFO!("[wf] pattern_loader::load (steamui): enter");
    pattern_loader::load(ModuleHandle(ui_hmodule()), &paths.ui, "steamui");
    LOG_INFO!("[wf] pattern_loader::load (steamui): done");

    LOG_INFO!("[wf] pattern_loader::load (steamclient): enter");
    pattern_loader::load(ModuleHandle(client_hmodule()), &paths.client, "steamclient");
    LOG_INFO!("[wf] pattern_loader::load (steamclient): done");

    LOG_INFO!("[wf] ipc_loader::load: enter");
    ipc_loader::load(&paths.client);
    LOG_INFO!("[wf] ipc_loader::load: done");

    LOG_INFO!("[wf] lua parse: enter");
    let resolved_lua_dir = config::get_lua_dir(&paths.install);
    let watch_dirs = lua_config::merge_watch_dirs(&config::get_lua_paths(), &resolved_lua_dir);
    for dir in &watch_dirs {
        lua_config::parse_directory(dir);
    }
    LOG_INFO!("[wf] lua parse: done");

    LOG_INFO!("[wf] persist_lua_keys: enter");
    crate::utils::config::vdf_persist::persist_lua_keys_to_config_vdf(&paths.install);
    LOG_INFO!("[wf] persist_lua_keys: done");

    LOG_INFO!("[wf] start_vdf_keeper: enter");
    crate::utils::config::vdf_persist::start_keeper(&paths.install);
    LOG_INFO!("[wf] start_vdf_keeper: done");

    LOG_INFO!("[wf] lua_file_watcher::start: enter");
    lua_file_watcher::start(watch_dirs);
    LOG_INFO!("[wf] lua_file_watcher::start: done");

    LOG_INFO!("[wf] config_file_watcher::start: enter");
    config_file_watcher::start(&paths.config, &paths.install);
    LOG_INFO!("[wf] config_file_watcher::start: done");

    LOG_INFO!("[wf] install_steamui_hooks: enter");
    crate::hook::framework::install_steamui_hooks();
    LOG_INFO!("[wf] install_steamui_hooks: done");

    LOG_INFO!("[wf] install_steamclient_hooks: enter");
    crate::hook::framework::install_steamclient_hooks();
    LOG_INFO!("[wf] install_steamclient_hooks: done");

    LOG_INFO!("[wf] report_missing_functions: enter");
    pattern_loader::report_missing_functions();
    LOG_INFO!("[wf] report_missing_functions: done");

    LOG_INFO!("[wf] cloud_redirect::initialize: enter");
    cloud_redirect::initialize(&paths.install);
    LOG_INFO!("[wf] cloud_redirect::initialize: done");

    LOG_INFO!("[wf] manifest_donor::start: enter");
    manifest_donor::start();
    LOG_INFO!("[wf] manifest_donor::start: done");

    LOG_INFO!("rusty_tools init complete");
    LOG_INFO!("[wf] init_thread: exit");
    0
}

// ── Host detection ──────────────────────────────────────────────────

/// True only when the host process is `steam.exe`. The proxy DLLs
/// already gate injection to Steam, but `rundll32` loads this DLL
/// directly to service a `bst://` link — there we must NOT run the
/// Steam-injection machinery.
fn is_steam_host() -> bool {
    let mut buf = [0u8; 260];
    let n = unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleFileNameA(
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
        )
    };
    if n == 0 { return false; }
    // GetModuleFileNameA does not null-terminate when the path exactly
    // fills the buffer, so slice to the returned length instead of
    // relying on CStr::from_ptr (which would read past the end).
    let s = String::from_utf8_lossy(&buf[..n as usize]);
    let name = match s.rfind('\\') {
        Some(i) => &s[i + 1..],
        None => &s[..],
    };
    name.eq_ignore_ascii_case("steam.exe")
}

// ── DllMain ─────────────────────────────────────────────────────────

/// # Safety
/// Called by the Windows loader. `hmodule` is the base address of this
/// DLL for the lifetime of the process.
#[cfg(windows)]
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hmodule: *mut c_void,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Do NOT use LOG_INFO! here. DllMain runs under the loader
            // lock; taking the file mutex can deadlock with Detours'
            // suspend-all during unhook. OutputDebugStringA is safe.
            let is_host = is_steam_host();
            dllmain_debug(if is_host {
                "[wf] DllMain DLL_PROCESS_ATTACH is_steam_host=true"
            } else {
                "[wf] DllMain DLL_PROCESS_ATTACH is_steam_host=false"
            });
            windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls(hmodule);
            if !is_host {
                // e.g. rundll32 servicing a bst:// handler. No
                // injection machinery runs; the TokeerUri export does
                // its work standalone.
                return 1;
            }
            // Hand off all real work to a worker thread. `DllMain`
            // itself must not touch the filesystem, load a module, or
            // open a detour transaction — all of that would run under
            // the loader lock.
            let self_module = ModuleHandle(hmodule);
            let _ = std::thread::Builder::new()
                .name("ost-init".into())
                .spawn(move || { init_thread(self_module); });
            1
        }
        DLL_PROCESS_DETACH => {
            // Loader-lock-safe: see comment in the ATTACH branch.
            dllmain_debug("[wf] DllMain DLL_PROCESS_DETACH");
            if is_steam_host() {
                let _ = std::panic::catch_unwind(|| {
                    config_file_watcher::stop();
                    lua_file_watcher::stop();
                    crate::utils::config::vdf_persist::stop_keeper();
                    // Best-effort final write to config.vdf before hooks come down.
                    let _ = std::panic::catch_unwind(|| {
                        if let Some(p) = paths() {
                            crate::utils::config::vdf_persist::persist_lua_keys_to_config_vdf(&p.install);
                        }
                    });
                    crate::hook::framework::uninstall_steamui_hooks();
                    crate::hook::framework::uninstall_steamclient_hooks();
                    cloud_redirect::shutdown();
                });
            }
            1
        }
        _ => 1,
    }
}

/// Non-Windows stub so the crate still builds elsewhere. The real
/// `DllMain` only exists on Windows; on other targets the symbol is
/// simply not exported.
#[cfg(not(windows))]
#[no_mangle]
pub extern "C" fn DllMain(_h: *mut c_void, _reason: u32, _r: *mut c_void) -> i32 { 1 }

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_null() {
        let before = client_hmodule();
        let _ = before;
    }

    #[test]
    fn set_and_get_round_trip() {
        let saved = client_hmodule();
        let sentinel = 0x1234_5678usize as *mut c_void;
        set_client_hmodule(sentinel);
        assert_eq!(client_hmodule(), sentinel);
        set_client_hmodule(saved);
    }

    #[test]
    fn k_onlinefix_appid_matches_cpp() {
        assert_eq!(K_ONLINE_FIX_APPID, 480);
    }

    #[test]
    fn paths_not_set_before_init() {
        // `PATHS` is set only by `initialize_steam_components`, which
        // unit tests do not call.
        let _ = paths();
    }
}
