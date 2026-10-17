//! `Hooks_Package.cpp`. Resolves `MarkLicenseAsChanged` /
//! `ProcessPendingLicenseUpdates` and hooks `CheckAppOwnership` to fake
//! ownership of Lua-declared apps. Enumerates the account's real owned
//! depots from the license list Steam sends at logon.
//!
//! ## Why the license list is parked
//!
//! `CMsgClientLicenseList` arrives once at logon and is the only
//! enumeration of owned packages available. Resolving each package to
//! its depots needs `GetPackageInfo`, whose `this` pointer is captured
//! from Steam's own first call — which normally has not happened by the
//! time the list lands. So the list is parked here and drained on the
//! next `CheckAppOwnership`.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::hook::steamui;
use crate::steam::enums::{EAppReleaseState, EPackageStatus};
use crate::steam::structs::{AppOwnership, CUtlVector, PackageInfo};
use crate::steam::types::{AppId_t, PackageId_t};
use crate::utils::config::lua_config;
use crate::{
    arm_capture_c, capture_ready, capture_this_fn, hook_fn, install_hook_c,
    resolve_c, resolve_fn, uninstall_hook,
};

// ── Resolve-only dependencies ───────────────────────────────────────

resolve_fn! {
    fn CUtlMemoryGrow(p_vec: *mut CUtlVector<AppId_t>, grow_size: i32) -> *mut c_void
}

resolve_fn! {
    fn MarkLicenseAsChanged(p_user: *mut c_void, package_id: u32, changed: bool) -> i64
}

resolve_fn! {
    fn ProcessPendingLicenseUpdates(p_user: *mut c_void) -> bool
}

// ── Captured-this function ──────────────────────────────────────────

capture_this_fn! {
    fn GetPackageInfo(
        p_this: *mut c_void,
        package_id: u32,
        access_token: u64,
    ) -> *mut PackageInfo
}

// ── Constants ───────────────────────────────────────────────────────

/// The injected package id. The C++ uses 0.
pub const K_INJECTED_PACKAGE_ID: PackageId_t = 0;
/// The access token the injected package's `GetPackageInfo` lookup uses.
pub const K_INJECTED_PKG_ACCESS_TOKEN: u64 = 10_660_652_434_190_618_804;

// ── State ───────────────────────────────────────────────────────────

/// Captured `CUser*` from the first `CheckAppOwnership` call. Needed by
/// `MarkLicenseAsChanged` / `ProcessPendingLicenseUpdates`.
static G_PCUSER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

/// The injected `PackageInfo*` returned by the first
/// `GetPackageInfo(0, TOKEN)` call. Mutated in place by
/// `notify_license_changed`.
static G_PINJECTED_PACKAGE_INFO: AtomicPtr<PackageInfo> =
    AtomicPtr::new(std::ptr::null_mut());

/// True once `InitFakeLicenseOnce` has run.
static LICENSE_INITIALIZED: AtomicBool = AtomicBool::new(false);
/// True when a refresh has been requested and not yet applied.
static LICENSE_REFRESH_PENDING: AtomicBool = AtomicBool::new(false);

/// Licence list parked from `CMsgClientLicenseList`, drained on the
/// next `CheckAppOwnership`.
static G_PENDING_LICENSES: Lazy<Mutex<Vec<License>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// True after a successful owned-depot dump. Guarded so a re-entrant
/// `CheckAppOwnership` during `DumpOwnedDepots` does not start a second
/// dump.
static G_OWNED_DEPOTS_DUMPED: AtomicBool = AtomicBool::new(false);

/// The deduped list of depots reachable through the account's
/// licenses. `ready` distinguishes "not resolved yet" from "owns
/// nothing".
struct OwnedDepots {
    ready: bool,
    depots: Vec<u32>,
}

static G_OWNED_DEPOTS: Lazy<Mutex<OwnedDepots>> = Lazy::new(|| {
    Mutex::new(OwnedDepots { ready: false, depots: Vec::new() })
});

/// One licence entry parsed from `CMsgClientLicenseList` by
/// `Hooks_NetPacket::Hooks_NetPacket_Licenses`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct License {
    pub package_id: PackageId_t,
    pub access_token: u64,
}

// ── Internal helpers ────────────────────────────────────────────────

unsafe fn cutl_memory_grow_wrap(
    p_vec: *mut CUtlVector<AppId_t>,
    grow_size: i32,
) -> bool {
    if O_C_UTL_MEMORY_GROW.is_null() {
        LOG_PACKAGE_WARN!("CUtlMemoryGrow: oCUtlMemoryGrow not ready, cannot grow");
        return false;
    }
    !o_CUtlMemoryGrow(p_vec, grow_size).is_null()
}

unsafe fn mark_license_as_changed_and_process_updates() -> bool {
    let user = G_PCUSER.load(Ordering::Acquire);
    if user.is_null()
        || O_MARK_LICENSE_AS_CHANGED.is_null()
        || O_PROCESS_PENDING_LICENSE_UPDATES.is_null()
    {
        LOG_PACKAGE_WARN!(
            "MarkLicenseAsChangedAndProcessUpdates: dependencies not ready, skipping"
        );
        return false;
    }
    let _ = o_MarkLicenseAsChanged(user, K_INJECTED_PACKAGE_ID, true);
    let _ = o_ProcessPendingLicenseUpdates(user);
    LOG_PACKAGE_DEBUG!(
        "MarkLicenseAsChangedAndProcessUpdates: marked package {} as changed and processed updates",
        K_INJECTED_PACKAGE_ID
    );
    true
}

unsafe fn try_process_pending_license_refresh() {
    if !LICENSE_REFRESH_PENDING.load(Ordering::Acquire) {
        return;
    }
    if mark_license_as_changed_and_process_updates() {
        LICENSE_REFRESH_PENDING.store(false, Ordering::Release);
    }
}

unsafe fn init_fake_license_once(p_pkg: *mut PackageInfo) -> bool {
    if p_pkg.is_null() {
        return false;
    }
    if (*p_pkg).Status != EPackageStatus::Available {
        LOG_PACKAGE_WARN!(
            "InitFakeLicenseOnce: package status is not Available ({:?}), skipping injection",
            (*p_pkg).Status,
        );
        return false;
    }

    // The C++ names this "depots" but the values are app ids. Preserved.
    let app_ids = lua_config::get_all_depot_ids();
    if !app_ids.is_empty() {
        let old_size = (*p_pkg).AppIdVec.m_Size;
        let num_to_add = app_ids.len() as u32;
        LOG_PACKAGE_INFO!(
            "InitFakeLicense(PackageId={}): adding {} apps, oldSize={}",
            K_INJECTED_PACKAGE_ID,
            num_to_add,
            old_size,
        );
        let vec_ptr = core::ptr::addr_of_mut!((*p_pkg).AppIdVec);
        if !cutl_memory_grow_wrap(vec_ptr, num_to_add as i32) {
            LOG_PACKAGE_WARN!(
                "InitFakeLicense(PackageId={}): failed to grow AppId vector",
                K_INJECTED_PACKAGE_ID,
            );
            return false;
        }
        let mem = (*p_pkg).AppIdVec.m_Memory.m_pMemory;
        for (i, id) in app_ids.iter().enumerate() {
            *mem.add(old_size as usize + i) = *id;
        }
    }

    LICENSE_INITIALIZED.store(true, Ordering::Release);
    LICENSE_REFRESH_PENDING.store(true, Ordering::Release);
    try_process_pending_license_refresh();
    true
}

unsafe fn try_init_fake_license_once() -> bool {
    if LICENSE_INITIALIZED.load(Ordering::Acquire) {
        return true;
    }
    if !(capture_ready!(GetPackageInfo)) {
        return false;
    }
    let p_this = CAPTURED_GET_PACKAGE_INFO.load();
    let p_pkg = o_GetPackageInfo(
        p_this,
        K_INJECTED_PACKAGE_ID,
        K_INJECTED_PKG_ACCESS_TOKEN,
    );
    if p_pkg.is_null() {
        LOG_PACKAGE_WARN!(
            "TryInitFakeLicenseOnce: GetPackageInfo returned null for injected package"
        );
        return false;
    }
    if G_PINJECTED_PACKAGE_INFO.load(Ordering::Acquire).is_null() {
        G_PINJECTED_PACKAGE_INFO.store(p_pkg, Ordering::Release);
    }
    init_fake_license_once(p_pkg)
}

/// Format up to `max_shown` ids from a raw `u32` array as "a, b, c",
/// appending " … (+N more)" when truncated. Used only for logging.
fn join_ids(p_mem: *const u32, size: u32, max_shown: u32) -> String {
    if p_mem.is_null() || size == 0 {
        return String::new();
    }
    let shown = size.min(max_shown);
    let mut out = String::new();
    for i in 0..shown {
        if i > 0 {
            out.push_str(", ");
        }
        let v = unsafe { *p_mem.add(i as usize) };
        out.push_str(&v.to_string());
    }
    if size > shown {
        out.push_str(&format!(" ... (+{} more)", size - shown));
    }
    out
}

unsafe fn dump_owned_depots() {
    let pending: Vec<License> = {
        let g = G_PENDING_LICENSES.lock();
        g.clone()
    };

    let p_this = CAPTURED_GET_PACKAGE_INFO.load();
    let mut resolved: u64 = 0;
    let mut unresolved: u64 = 0;
    let mut total_depots: u64 = 0;
    let mut owned: Vec<u32> = Vec::new();

    for lic in &pending {
        let p_pkg = o_GetPackageInfo(p_this, lic.package_id, lic.access_token);
        if p_pkg.is_null() {
            // Usually a package whose info has not been cached yet rather
            // than a real failure; logged at debug so it does not drown
            // the useful lines.
            unresolved += 1;
            LOG_PACKAGE_DEBUG!(
                "OwnedDepots: package {} did not resolve (token={})",
                lic.package_id,
                lic.access_token,
            );
            continue;
        }

        resolved += 1;
        let depot_count = (*p_pkg).DepotIdVec.m_Size;
        total_depots += depot_count as u64;
        let depot_mem = (*p_pkg).DepotIdVec.m_Memory.m_pMemory;
        for i in 0..depot_count {
            let depot_id = *depot_mem.add(i as usize);
            if depot_id != 0 {
                owned.push(depot_id);
            }
        }

        let apps_mem = (*p_pkg).AppIdVec.m_Memory.m_pMemory;
        LOG_PACKAGE_INFO!(
            "OwnedDepots: package {} apps={} depots={} | appids=[{}] | depotids=[{}]",
            lic.package_id,
            (*p_pkg).AppIdVec.m_Size,
            depot_count,
            join_ids(apps_mem, (*p_pkg).AppIdVec.m_Size, 64),
            join_ids(depot_mem, depot_count, 64),
        );
    }

    // Packages overlap heavily — the same depot arrives via several
    // licenses — so dedupe before anyone intersects against this.
    owned.sort_unstable();
    owned.dedup();

    {
        let mut g = G_OWNED_DEPOTS.lock();
        g.depots = owned.clone();
        g.ready = true;
    }

    LOG_PACKAGE_INFO!(
        "OwnedDepots: done — {} package(s) resolved, {} unresolved, {} depot entr(ies), {} unique",
        resolved, unresolved, total_depots, owned.len(),
    );

    // Chunked debug dump: this is the set the donor actually intersects
    // a wanted list against, and deciding what gets disclosed off a
    // list nobody can read back is not something to leave to inference.
    const PER_LINE: usize = 64;
    let mut i = 0;
    while i < owned.len() {
        let end = (i + PER_LINE).min(owned.len());
        let chunk: Vec<String> = owned[i..end].iter().map(|x| x.to_string()).collect();
        LOG_PACKAGE_DEBUG!("OwnedDepots[{}-{}]: {}", i, end - 1, chunk.join(", "));
        i = end;
    }
}

// ── Hook ────────────────────────────────────────────────────────────

hook_fn! {
    fn CheckAppOwnership(
        p_obj: *mut c_void,
        app_id: AppId_t,
        p_own: *mut AppOwnership,
    ) -> bool {
        if G_PCUSER.load(Ordering::Acquire).is_null() {
            G_PCUSER.store(p_obj, Ordering::Release);
            LOG_PACKAGE_DEBUG!("CheckAppOwnership: captured CUser {:p}", p_obj);
        }

        let result = o_CheckAppOwnership(p_obj, app_id, p_own);
        let _ = try_init_fake_license_once();
        try_dump_owned_depots();

        if lua_config::has_depot(app_id, false) && !p_own.is_null() {
            if result && (*p_own).ExistInPackageNums > 1 {
                // Actually owned — record so HasDepot excludes it going
                // forward.
                lua_config::mark_owned(app_id);
                (*p_own).ReleaseState = EAppReleaseState::Released;
            } else {
                (*p_own).PackageId = K_INJECTED_PACKAGE_ID;
                (*p_own).ReleaseState = EAppReleaseState::Released;
                // Forces DLCs on Steam family-shared games that we don't
                // own when their appid is added via .lua.
                (*p_own).bOwnsLicense = true;
                // Setting this free flag to false hides the app from the
                // library UI.
                (*p_own).bFreeLicense = false;
                return true;
            }
        }
        result
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Store a new license list. Called from the `CMsgClientLicenseList`
/// hook (batch 6e).
pub fn on_license_list(licenses: Vec<License>) {
    {
        let mut g = G_PENDING_LICENSES.lock();
        *g = licenses;
    }
    G_OWNED_DEPOTS_DUMPED.store(false, Ordering::Release);
}

/// Try to resolve the parked licenses to their depots. Safe to call
/// repeatedly and from anywhere: no-ops unless licenses are pending and
/// the `GetPackageInfo` capture is ready. Driven from
/// `CheckAppOwnership`, which runs often enough to guarantee both
/// eventually.
pub fn try_dump_owned_depots() {
    if G_OWNED_DEPOTS_DUMPED.load(Ordering::Acquire) {
        return;
    }
    {
        let g = G_PENDING_LICENSES.lock();
        if g.is_empty() {
            return;
        }
    }
    if !(capture_ready!(GetPackageInfo)) {
        return;
    }
    // Set before dumping, not after: `dump_owned_depots` calls back into
    // steamclient, and `CheckAppOwnership` is what drives us — without
    // this a re-entrant call would start a second dump.
    G_OWNED_DEPOTS_DUMPED.store(true, Ordering::Release);
    unsafe { dump_owned_depots(); }
}

/// Returns `(ready, owned_depots)`. `ready = false` means the license
/// list has not yet been resolved.
pub fn get_owned_depots() -> (bool, Vec<u32>) {
    let g = G_OWNED_DEPOTS.lock();
    (g.ready, g.depots.clone())
}

/// Mark package 0 as changed and trigger
/// `CClientAppManager_ProcessPendingLicenseUpdates`. Called by the Lua
/// config watcher whenever the set of managed depots changes.
pub fn notify_license_changed() {
    let p_pkg = G_PINJECTED_PACKAGE_INFO.load(Ordering::Acquire);
    if p_pkg.is_null() {
        LOG_PACKAGE_WARN!(
            "NotifyLicenseChanged: injected PackageInfo not ready, cannot notify"
        );
        return;
    }
    // Also require that G_PCUSER is captured, since the eventual
    // MarkLicenseAsChanged/ProcessPendingLicenseUpdates call needs it.
    // Without this guard, a very early notify_license_changed() would
    // take() the pending lists and then fail to apply them, silently
    // discarding the user's Lua changes. CheckAppOwnership captures
    // G_PCUSER on its first call, which is well before any realistic
    // file-watch event, but the guard is cheap.
    if G_PCUSER.load(Ordering::Acquire).is_null() {
        LOG_PACKAGE_DEBUG!(
            "NotifyLicenseChanged: CUser not captured yet, leaving pending queued"
        );
        return;
    }

    // ── Remove depots that were unloaded ────────────────────────────
    let removals = lua_config::take_pending_removals();
    LOG_PACKAGE_DEBUG!(
        "NotifyLicenseChanged: processing {} removals",
        removals.len()
    );
    let mut removed_count: u32 = 0;
    for id in &removals {
        // SAFETY: `p_pkg` is the injected PackageInfo captured earlier;
        // its lifetime is the process, and the C++ mutates it in place.
        let ok = unsafe {
            let v = &mut (*p_pkg).AppIdVec;
            v.find_and_fast_remove(id)
        };
        if ok {
            removed_count += 1;
            LOG_PACKAGE_DEBUG!("NotifyLicenseChanged: removed AppId {}", id);
        } else {
            LOG_PACKAGE_WARN!(
                "NotifyLicenseChanged: AppId {} not found in package AppIdVec during removal",
                id
            );
        }
    }

    // ── Add depots that are newly loaded ────────────────────────────
    let additions = lua_config::take_pending_additions();
    let mut added_ids: HashSet<AppId_t> = HashSet::new();
    LOG_PACKAGE_DEBUG!(
        "NotifyLicenseChanged: processing {} additions",
        additions.len()
    );
    if !additions.is_empty() {
        unsafe {
            let old_size = (*p_pkg).AppIdVec.m_Size;
            let vec_ptr = core::ptr::addr_of_mut!((*p_pkg).AppIdVec);
            if cutl_memory_grow_wrap(vec_ptr, additions.len() as i32) {
                // An applied addition invalidates any UI removal that has
                // not reached the UI thread yet.
                for id in &additions {
                    steamui::cancel_removal(*id);
                }
                let mem = (*p_pkg).AppIdVec.m_Memory.m_pMemory;
                for (i, id) in additions.iter().enumerate() {
                    *mem.add(old_size as usize + i) = *id;
                    added_ids.insert(*id);
                    LOG_PACKAGE_DEBUG!(
                        "NotifyLicenseChanged: inserted AppId {} at [{}]",
                        id,
                        old_size as usize + i,
                    );
                }
            } else {
                LOG_PACKAGE_WARN!(
                    "NotifyLicenseChanged: failed to grow AppId vector for additions"
                );
            }
        }
    }

    if added_ids.is_empty() && removed_count == 0 {
        LOG_PACKAGE_DEBUG!("NotifyLicenseChanged: no changes");
        return;
    }

    // Mark package 0 as changed and trigger library refresh.
    let ok = unsafe { mark_license_as_changed_and_process_updates() };
    if !ok {
        LOG_PACKAGE_WARN!("NotifyLicenseChanged: failed to mark license as changed");
        return;
    }
    LOG_PACKAGE_INFO!(
        "NotifyLicenseChanged: {} added, {} removed",
        added_ids.len(),
        removed_count,
    );

    // Queue UI removals for the main-thread RunFrame hook to drain.
    // Never touch MarkAppChange from this (FileWatcher) thread.
    let mut queued_removal_count = 0;
    for id in &removals {
        // ParseFile unloads the old file before parsing the replacement.
        // Do not queue that transient removal when the id was added again.
        if !added_ids.contains(id) {
            steamui::queue_removal(*id);
            queued_removal_count += 1;
        }
    }
    LOG_PACKAGE_DEBUG!(
        "NotifyLicenseChanged: queued {} UI removals, skipped {} transient removals",
        queued_removal_count,
        removals.len() - queued_removal_count,
    );
}

pub fn install() -> bool {
    resolve_c!(CUtlMemoryGrow);
    resolve_c!(MarkLicenseAsChanged);
    resolve_c!(ProcessPendingLicenseUpdates);

    arm_capture_c!(GetPackageInfo);

    install_hook_c!(CheckAppOwnership)
}

pub fn uninstall() -> bool {
    uninstall_hook!(CheckAppOwnership)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_cpp() {
        assert_eq!(K_INJECTED_PACKAGE_ID, 0);
        assert_eq!(K_INJECTED_PKG_ACCESS_TOKEN, 10_660_652_434_190_618_804);
    }

    #[test]
    fn owned_depots_starts_not_ready() {
        let (ready, depots) = get_owned_depots();
        assert!(!ready);
        assert!(depots.is_empty());
    }

    #[test]
    fn license_round_trip() {
        let l = License {
            package_id: 12345,
            access_token: 0xDEAD_BEEF_CAFE_BABE,
        };
        assert_eq!(l.package_id, 12345);
        assert_eq!(l.access_token, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[test]
    fn join_ids_empty_returns_empty() {
        assert_eq!(join_ids(std::ptr::null(), 0, 64), "");
    }
}
