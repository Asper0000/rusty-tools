//! Cross-module bridges. These functions delegate to the module that
//! now owns the real implementation.

#![allow(unused)]

// ── ManifestClient bridge ───────────────────────────────────────────

/// Delegates to `utils::steam_metadata::manifest_client::set_provider`.
pub fn manifest_set_provider(name: &str) -> bool {
    crate::utils::steam_metadata::manifest_client::set_provider(name)
}

pub fn manifest_active_provider_name() -> String {
    crate::utils::steam_metadata::manifest_client::active_provider_name().to_string()
}

// ── StatsClient bridge ──────────────────────────────────────────────

/// Delegates to `utils::steam_metadata::stats_client::fetch_stat_steam_id`.
pub fn stats_fetch_stat_steam_id(app_id: u32) -> Option<u64> {
    crate::utils::steam_metadata::stats_client::fetch_stat_steam_id(app_id)
}

// â”€â”€ AppTicket bridge â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€


// ── Hooks bridge ────────────────────────────────────────────────────

/// Delegates to `hook::package::notify_license_changed`.
pub fn hooks_notify_license_changed() {
    crate::hook::package::notify_license_changed();
}

/// Delegates to `hook::netpacket::probe_manifest`.
pub fn hooks_probe_manifest(depot_id: u32) -> bool {
    crate::hook::netpacket::probe_manifest(depot_id)
}

/// Returns `(ready, owned_depots)`. Delegates to
/// `hook::package::get_owned_depots`.
pub fn hooks_get_owned_depots() -> (bool, Vec<u32>) {
    crate::hook::package::get_owned_depots()
}

/// Returns `Some((app_id, gid))` for a depot that has passed through
/// `BuildDepotDependency` this session. Delegates to
/// `hook::manifest::lookup_depot`.
pub fn hooks_lookup_depot(depot_id: u32) -> Option<(u32, u64)> {
    crate::hook::manifest::lookup_depot(depot_id)
}

/// Originate a `GetManifestRequestCode` request as the signed-in user.
/// Delegates to `hook::netpacket::request_manifest_code`.
pub fn hooks_request_manifest_code(app_id: u32, depot_id: u32, gid: u64) -> u64 {
    crate::hook::netpacket::request_manifest_code(app_id, depot_id, gid)
}

/// Same as [`hooks_request_manifest_code`] with a caller-supplied
/// timeout. The donor worker uses a shorter timeout so a stalled
/// reply cannot block a mint cycle for 30 seconds per depot.
pub fn hooks_request_manifest_code_with_timeout(
    app_id: u32, depot_id: u32, gid: u64, timeout_ms: u32,
) -> u64 {
    crate::hook::netpacket::request_manifest_code_with_timeout(
        app_id, depot_id, gid,
        std::time::Duration::from_millis(timeout_ms as u64),
    )
}

// ---- Pipe bridge (real since batch 06f) ---------------------------

/// Called from `Hooks_IPC::handle_handshake`. Delegates to
/// `pipe::pipe_manager::on_handshake`.
///
/// # Safety
/// `pipe`, if non-null, must point at a live `CPipeClient`.
pub unsafe fn pipe_manager_on_handshake(pipe: *mut crate::steam::structs::CPipeClient) {
    crate::pipe::pipe_manager::on_handshake(pipe);
}

