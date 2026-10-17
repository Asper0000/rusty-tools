//! `Hooks_SteamUI.cpp`. Hooks into `steamui.dll` to keep track of which
//! apps are actively downloading, to stamp PurchasedTime onto the app
//! overview, and to process the deferred removal queue on the UI thread.
//!
//! ## Known divergence: `BuildCompleteAppOverviewChange`
//!
//! The C++ body re-injects our `removed_appid` set into a freshly
//! rebuilt `CAppOverview_Change` message so that a full library rebuild
//! does not resurrect a removed game. That requires calling Steam's own
//! protobuf helper (`RepeatedFieldUint32_Add`) against a message
//! created by the C++ protobuf runtime. prost uses a different
//! in-memory layout for `repeated uint32` fields, so the helper cannot
//! be called against prost-generated types. The hook is installed and
//! passes through; see `STUBS.md` "Known behavioural divergences".

use std::collections::HashSet;
use std::ffi::c_void;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::steam::enums::{EAppChangeFlags, EAppOwnershipFlags, EAppState};
use crate::steam::structs::CSteamApp;
use crate::steam::types::AppId_t;
use crate::utils::config::lua_config;
use crate::{
    arm_capture_u, capture_ready, capture_this_fn, hook_fn,
    install_hook_u, uninstall_hook,
};

// ── Captured-this functions ─────────────────────────────────────────

capture_this_fn! {
    fn GetAppByID(
        p_this: *mut c_void,
        app_id: AppId_t,
        b_create: bool,
    ) -> *mut CSteamApp
}

capture_this_fn! {
    fn MarkAppChange(
        p_this: *mut c_void,
        app_id: AppId_t,
        change_flags: u32,
    ) -> *mut c_void
}

// ── Module state ────────────────────────────────────────────────────

/// Apps currently downloading/updating. Maintained on the UI thread,
/// read from any thread.
static G_ACTIVE_DL: Lazy<Mutex<HashSet<AppId_t>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

/// Apps to drop from the library: queued off-thread, drained on the UI
/// thread by the RunFrame hook.
static G_PENDING_REMOVALS: Lazy<Mutex<Vec<AppId_t>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Apps whose removal has already reached the UI thread.
static G_REMOVED_APP_IDS: Lazy<Mutex<HashSet<AppId_t>>> =
    Lazy::new(|| Mutex::new(HashSet::new()));

// ── Hooks ───────────────────────────────────────────────────────────

hook_fn! {
    fn FillInAppOverview(
        p_this: *mut c_void,
        p_app_overview: *mut c_void,
        p_app: *mut CSteamApp,
    ) -> *mut c_void {
        if !p_app.is_null() {
            // `CSteamApp` is `#[repr(C, packed)]`, so every field read or
            // write must go through `read_unaligned` / `write_unaligned`.
            let (app_id, active) = unsafe {
                let app_state: EAppState = core::ptr::read_unaligned(
                    core::ptr::addr_of!((*p_app).AppStateFlags),
                );
                let id: AppId_t = core::ptr::read_unaligned(
                    core::ptr::addr_of!((*p_app).nAppID),
                );
                // UpdateRunning|UpdateStarted = a download is actually
                // going, versus an idle scheduled retry.
                let active = app_state.intersects(
                    EAppState::UpdateRunning | EAppState::UpdateStarted,
                );
                (id, active)
            };

            {
                let mut set = G_ACTIVE_DL.lock();
                if active {
                    set.insert(app_id);
                } else {
                    set.remove(&app_id);
                }
            }

            if lua_config::has_depot(app_id, false) {
                let t = lua_config::get_purchase_time(app_id);
                if t != 0 {
                    unsafe {
                        core::ptr::write_unaligned(
                            core::ptr::addr_of_mut!((*p_app).PurchasedTime),
                            t,
                        );
                        let read_back: u32 = core::ptr::read_unaligned(
                            core::ptr::addr_of!((*p_app).PurchasedTime),
                        );
                        LOG_STEAMUI_TRACE!(
                            "FillInAppOverview: set PurchasedTime={} for appId={}",
                            read_back,
                            app_id,
                        );
                    }
                }
            }
        }
        o_FillInAppOverview(p_this, p_app_overview, p_app)
    }
}

hook_fn! {
    fn BuildCompleteAppOverviewChange(
        p_controller: *mut c_void,
        p_change: *mut c_void,
        optional_callback_slot: *mut c_void,
    ) -> () {
        // DIVERGENCE FROM C++: the C++ body re-injects the
        // `removed_appid` entries into the freshly-rebuilt message so a
        // full library rebuild does not resurrect a removed game. That
        // requires calling Steam's own protobuf helper against a message
        // created by the C++ protobuf runtime, which prost cannot do (see
        // the module doc). The hook installs and passes through.
        //
        // User-visible consequence: a removed game can reappear in the
        // library after a full rebuild (rare - typically only on profile
        // corruption, cache clear, or fresh login). Incremental updates
        // are unaffected.
        o_BuildCompleteAppOverviewChange(p_controller, p_change, optional_callback_slot)
    }
}

hook_fn! {
    fn CSteamUIAppControllerRunFrame(p_controller: *mut c_void) -> *mut c_void {
        if capture_ready!(GetAppByID) && capture_ready!(MarkAppChange) {
            let draining: Vec<AppId_t> = {
                let mut pending = G_PENDING_REMOVALS.lock();
                std::mem::take(&mut *pending)
            };
            for app_id in draining {
                if lua_config::is_owned(app_id) {
                    LOG_STEAMUI_DEBUG!(
                        "RunFrame: appId {} is owned again, skipping removal",
                        app_id
                    );
                    continue;
                }
                let get_this = unsafe { CAPTURED_GET_APP_BY_I_D.load() };
                let app = unsafe { o_GetAppByID(get_this, app_id, false) };
                if !app.is_null() {
                    let should_mark = unsafe {
                        core::ptr::write_unaligned(
                            core::ptr::addr_of_mut!((*app).OwnershipFlags),
                            EAppOwnershipFlags::None,
                        );
                        let state: EAppState = core::ptr::read_unaligned(
                            core::ptr::addr_of!((*app).AppStateFlags),
                        );
                        state == EAppState::Uninstalled
                    };
                    if should_mark {
                        let mut set = G_REMOVED_APP_IDS.lock();
                        set.insert(app_id);
                    }
                }
                let mark_this = unsafe { CAPTURED_MARK_APP_CHANGE.load() };
                unsafe {
                    o_MarkAppChange(
                        mark_this,
                        app_id,
                        EAppChangeFlags::AppInfoOrConfig.bits(),
                    );
                }
            }
        }
        o_CSteamUIAppControllerRunFrame(p_controller)
    }
}

// ── Public API ──────────────────────────────────────────────────────

pub fn queue_removal(app_id: AppId_t) {
    let mut pending = G_PENDING_REMOVALS.lock();
    pending.push(app_id);
}

pub fn cancel_removal(app_id: AppId_t) {
    {
        let mut pending = G_PENDING_REMOVALS.lock();
        pending.retain(|&x| x != app_id);
    }
    {
        let mut removed = G_REMOVED_APP_IDS.lock();
        removed.remove(&app_id);
    }
}

pub fn active_download_count() -> usize {
    G_ACTIVE_DL.lock().len()
}

pub fn install() -> bool {
    arm_capture_u!(GetAppByID);
    arm_capture_u!(MarkAppChange);

    // `RepeatedFieldUint32_Add` is deliberately not resolved: the C++
    // calls it with a `RepeatedField<uint32>` pointer that prost does
    // not produce. See the module doc and `STUBS.md`.

    let ok1 = install_hook_u!(FillInAppOverview);
    let ok2 = install_hook_u!(BuildCompleteAppOverviewChange);
    let ok3 = install_hook_u!(CSteamUIAppControllerRunFrame);
    ok1 && ok2 && ok3
}

pub fn uninstall() -> bool {
    let ok1 = uninstall_hook!(FillInAppOverview);
    let ok2 = uninstall_hook!(BuildCompleteAppOverviewChange);
    let ok3 = uninstall_hook!(CSteamUIAppControllerRunFrame);
    ok1 && ok2 && ok3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_and_cancel_removal_round_trip() {
        {
            let mut p = G_PENDING_REMOVALS.lock();
            p.clear();
        }
        {
            let mut r = G_REMOVED_APP_IDS.lock();
            r.clear();
        }

        queue_removal(100);
        queue_removal(200);
        {
            let p = G_PENDING_REMOVALS.lock();
            assert_eq!(p.len(), 2);
        }

        cancel_removal(100);
        {
            let p = G_PENDING_REMOVALS.lock();
            assert_eq!(*p, vec![200]);
        }

        cancel_removal(999);
        {
            let p = G_PENDING_REMOVALS.lock();
            assert_eq!(*p, vec![200]);
        }
    }

    #[test]
    fn active_download_count_starts_zero() {
        {
            let mut s = G_ACTIVE_DL.lock();
            s.clear();
        }
        assert_eq!(active_download_count(), 0);
    }
}
