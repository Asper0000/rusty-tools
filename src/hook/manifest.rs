//! `Hooks_Manifest.cpp`. Records depot<->app<->gid mappings as Steam
//! builds depot configs (`BuildDepotDependency`) and pre-seeds the
//! manifest cache on per-manifest acquire (`YldLoadDepotManifest`).
//!
//! The `BuildDepotDependency` hook also guards against an appinfo
//! mid-refresh: Steam reports zero depots for an app during the window
//! where its appinfo is being swapped in, and acting on that transient
//! writes an empty depot config. Returning `false` makes Steam keep its
//! previous config so the next pass builds it properly.

use std::collections::{BTreeSet, HashMap};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::platform::{dialog, thread};
use crate::steam::structs::{CUtlVector, DepotEntry};
use crate::steam::types::AppId_t;
use crate::utils::config::lua_config;
use crate::utils::steam_metadata::manifest_cache;
use crate::{hook_fn, install_hook_c, uninstall_hook};

/// Per-fetch timeout for the on-demand pre-seed in
/// `YldLoadDepotManifest`.
const K_PRESEED_FETCH_TIMEOUT_MS: u32 = 5000;

/// Quiet window before a missing-manifest popup is surfaced.
const K_MISS_NOTIFY_DEBOUNCE_MS: u64 = 3000;

/// Cap on the number of depots shown in the popup.
const K_MISS_NOTIFY_MAX_LINES: usize = 20;

// ── Module state ────────────────────────────────────────────────────

struct MissingState {
    /// Pending batch, depot->gid.
    missing: BTreeSet<(u32, u64)>,
    /// Already boxed this session.
    notified: BTreeSet<(u32, u64)>,
}

static G_MISSING_STATE: Lazy<Mutex<MissingState>> = Lazy::new(|| {
    Mutex::new(MissingState {
        missing: BTreeSet::new(),
        notified: BTreeSet::new(),
    })
});

/// Debounce generation counter.
static G_MISS_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug)]
struct DepotSeen {
    app_id: AppId_t,
    gid: u64,
}

/// depotId -> (appId, Steam's own manifest GID). Recorded before the
/// override pass, so this is what Steam believes rather than what we
/// told it.
static G_DEPOTS_SEEN: Lazy<Mutex<HashMap<u32, DepotSeen>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Internal helpers ────────────────────────────────────────────────

fn depot_entry_debug(e: &DepotEntry) -> String {
    format!(
        "DepotId={} AppId={} Gid={} Size={} Dlc={} Lcs={} Carry={} Shared={}",
        e.DepotId,
        e.AppId,
        e.ManifestGid,
        e.ManifestSize,
        e.DlcAppId,
        e.LcsRequired as i32,
        e.bNotNewTarget as i32,
        e.SharedInstall as i32,
    )
}

unsafe fn record_depots(vec: *const CUtlVector<DepotEntry>) {
    if vec.is_null() {
        return;
    }
    let mut seen = G_DEPOTS_SEEN.lock();
    let size = (*vec).m_Size;
    let mem = (*vec).m_Memory.m_pMemory;
    for i in 0..size {
        let e = &*mem.add(i as usize);
        if e.DepotId == 0 || e.ManifestGid == 0 {
            continue;
        }
        seen.insert(e.DepotId, DepotSeen { app_id: e.AppId, gid: e.ManifestGid });
    }
}

/// Record a depot whose manifest is not archived yet (404) and, after a
/// short quiet window, show a single MessageBox listing everything
/// collected. The debounce (a generation counter) coalesces the burst
/// of a whole install — including the separate `BuildDepotDependency`
/// calls for a game's DLC apps — into one prompt. The box is shown on
/// the detached waiter, never on Steam's own thread.
fn report_missing(depot: u32, gid: u64) {
    {
        let mut g = G_MISSING_STATE.lock();
        // Once per depot per session: an active-but-stuck download
        // re-requests the code every ~30 s, but the user only needs to
        // be told once.
        if !g.notified.insert((depot, gid)) {
            return;
        }
        g.missing.insert((depot, gid));
    }
    let gen = G_MISS_GEN.fetch_add(1, Ordering::AcqRel) + 1;

    let _ = thread::start_detached(move || {
        std::thread::sleep(Duration::from_millis(K_MISS_NOTIFY_DEBOUNCE_MS));
        // A newer miss arrived during the wait — let its waiter show the
        // box.
        if G_MISS_GEN.load(Ordering::Acquire) != gen {
            return 0;
        }

        let snapshot: Vec<(u32, u64)> = {
            let mut g = G_MISSING_STATE.lock();
            let s: Vec<(u32, u64)> = g.missing.iter().copied().collect();
            g.missing.clear();
            s
        };
        if snapshot.is_empty() {
            return 0;
        }

        let mut body = format!(
            "Missing {} manifest(s) from the cache - not archived yet.\n\
             They've been queued; try the download again in a little while.\n\n\
             depot;manifestid\n",
            snapshot.len(),
        );

        let mut shown = 0;
        for (depot, gid) in &snapshot {
            if shown >= K_MISS_NOTIFY_MAX_LINES {
                body.push_str(&format!("...and {} more", snapshot.len() - shown));
                break;
            }
            body.push_str(&format!("{};{}\n", depot, gid));
            shown += 1;
        }

        dialog::show_warning("OpenSteamTool - manifests not ready", &body);
        0
    });
}

// ── Hooks ───────────────────────────────────────────────────────────

hook_fn! {
    fn BuildDepotDependency(
        p_user_app_mgr: *mut c_void,
        app_id: AppId_t,
        p_user_config: *mut c_void,
        p_depot_info: *mut CUtlVector<DepotEntry>,
        p_shared_depot_info: *mut CUtlVector<DepotEntry>,
        p_steam_app: *mut c_void,
        p_build_id: *mut u32,
        p_beta_fallback: *mut bool,
    ) -> bool {
        let result = o_BuildDepotDependency(
            p_user_app_mgr,
            app_id,
            p_user_config,
            p_depot_info,
            p_shared_depot_info,
            p_steam_app,
            p_build_id,
            p_beta_fallback,
        );

        // Trace logging of the returned depot entries, when both lists
        // are present. Kept at TRACE so the common path is silent.
        if !p_depot_info.is_null() {
            let size = (*p_depot_info).m_Size;
            LOG_MANIFEST_TRACE!("pDepotInfo->nCount={}", size);
            if size > 0 {
                let mem = (*p_depot_info).m_Memory.m_pMemory;
                for i in 0..size {
                    LOG_MANIFEST_TRACE!("  [{}] {}", i, depot_entry_debug(&*mem.add(i as usize)));
                }
            }
        }
        if !p_shared_depot_info.is_null() {
            let size = (*p_shared_depot_info).m_Size;
            LOG_MANIFEST_TRACE!("pSharedDepotInfo->nCount={}", size);
            if size > 0 {
                let mem = (*p_shared_depot_info).m_Memory.m_pMemory;
                for i in 0..size {
                    LOG_MANIFEST_TRACE!(
                        "  shared[{}] {}",
                        i,
                        depot_entry_debug(&*mem.add(i as usize)),
                    );
                }
            }
        }

        if !result {
            return result;
        }

        // Steam reports zero depots for an app during the window where
        // its appinfo is being swapped in by a PICS refresh — the old
        // depot set is dropped before the new one lands. The caller
        // stores whatever comes back, so acting on the transient writes
        // an EMPTY depot config; the update job then reads that, logs
        // "0 active: 0 target", commits nothing, and still stamps the
        // app Fully Installed at the new BuildID — an install that
        // silently "completes" having downloaded nothing.
        //
        // Returning false makes the caller bail on its clean failure
        // path and keep the app's previous depot config.
        //
        // Only pDepotInfo is tested. pSharedDepotInfo holds depots
        // belonging to OTHER apps (sharedinstall / depotfromapp
        // redirects), so it says nothing about whether this app's own
        // config is sound.
        if app_id != 0
            && (p_depot_info.is_null() || unsafe { (*p_depot_info).m_Size } == 0)
            && lua_config::has_depot(app_id, false)
        {
            LOG_MANIFEST_WARN!(
                "BuildDepotDependency: app {} returned 0 depots (appinfo mid-refresh) — \
                 failing the call so Steam keeps its existing depot config instead of \
                 storing an empty one",
                app_id,
            );
            return false;
        }

        // Before the override pass, so what is cached is Steam's GID.
        unsafe {
            record_depots(p_depot_info);
            record_depots(p_shared_depot_info);
        }

        let overrides = lua_config::get_manifest_overrides();
        if !overrides.is_empty() && !p_depot_info.is_null() {
            let size = unsafe { (*p_depot_info).m_Size };
            if size > 0 {
                let mem = unsafe { (*p_depot_info).m_Memory.m_pMemory };
                for i in 0..size {
                    let e = unsafe { &mut *mem.add(i as usize) };
                    if let Some(ov) = overrides.get(&(e.DepotId as u64)) {
                        // If size=0 in the override, keep the original
                        // size (affects download display but not the
                        // actual download).
                        let new_size = if ov.size != 0 { ov.size } else { e.ManifestSize };
                        LOG_MANIFEST_INFO!(
                            "BuildDepotDependency: patching depot {} gid={}->{} size={}->{}",
                            e.DepotId,
                            e.ManifestGid,
                            ov.gid,
                            e.ManifestSize,
                            new_size,
                        );
                        e.ManifestGid  = ov.gid;
                        e.ManifestSize = new_size;
                    }
                }
            }
        }

        result
    }
}

hook_fn! {
    fn YldLoadDepotManifest(
        a1: *mut c_void,
        a2: *mut c_void,
        app_id: i32,
        depot_id: u32,
        manifest_gid: u64,
        branch: *const i8,
    ) -> i64 {
        // Only depots OST unlocks and does not own (lua-added, incl.
        // workshop apps). A bounded blocking fetch: the original checks
        // disk on the very next line, so an archive hit converts into a
        // first-attempt success; a miss just returns and the original
        // takes its normal request-code path.
        if depot_id != 0 && manifest_gid != 0 && lua_config::has_depot(depot_id, false) {
            let branch_str = if branch.is_null() {
                ""
            } else {
                unsafe { std::ffi::CStr::from_ptr(branch) }.to_str().unwrap_or("")
            };
            LOG_MANIFEST_DEBUG!(
                "YldLoadDepotManifest: pre-seed app={} depot={} gid={} branch={}",
                app_id, depot_id, manifest_gid, branch_str,
            );
            let _ = manifest_cache::ensure_cached(
                app_id as u32,
                depot_id,
                manifest_gid,
                K_PRESEED_FETCH_TIMEOUT_MS,
                false,
            );
        }
        o_YldLoadDepotManifest(a1, a2, app_id, depot_id, manifest_gid, branch)
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Look up the app id and manifest GID last seen for a depot.
/// Populated from `BuildDepotDependency`, which is the only place a
/// depot's current GID passes through. Returns `None` if the depot has
/// not been seen.
pub fn lookup_depot(depot_id: u32) -> Option<(AppId_t, u64)> {
    let g = G_DEPOTS_SEEN.lock();
    g.get(&depot_id).map(|s| (s.app_id, s.gid))
}

/// Record a depot whose manifest is not archived yet (a definitive
/// 404) and, after a short quiet window, surface one MessageBox listing
/// everything collected. Safe to call from any thread. Called from the
/// real-download path so the prompt only appears when a download
/// actually needs a manifest that isn't ready — never during the
/// startup depot sweep.
pub fn report_missing_manifest(depot_id: u32, gid: u64) {
    report_missing(depot_id, gid);
}

pub fn install() -> bool {
    let ok1 = install_hook_c!(BuildDepotDependency);
    let ok2 = install_hook_c!(YldLoadDepotManifest);
    ok1 && ok2
}

pub fn uninstall() -> bool {
    let ok1 = uninstall_hook!(BuildDepotDependency);
    let ok2 = uninstall_hook!(YldLoadDepotManifest);
    ok1 && ok2
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_missing_depot_returns_none() {
        assert!(lookup_depot(999_999_999).is_none());
    }

    #[test]
    fn record_and_lookup_round_trip() {
        // The public path exercises the map; a full raw-pointer round
        // trip goes through `record_depots` which needs a live
        // `CUtlVector`. Test via insertion through the same map.
        let depot = 4_242_424_242u32;
        assert!(lookup_depot(depot).is_none());
        {
            let mut g = G_DEPOTS_SEEN.lock();
            g.insert(depot, DepotSeen { app_id: 12_345, gid: 0xDEAD_BEEF });
        }
        assert_eq!(lookup_depot(depot), Some((12_345, 0xDEAD_BEEF)));
        // Clean up so other tests see a clean registry.
        {
            let mut g = G_DEPOTS_SEEN.lock();
            g.remove(&depot);
        }
    }
}
