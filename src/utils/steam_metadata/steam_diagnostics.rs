//! Steam client diagnostics captured at init. Source: `SteamDiagnostics.cpp`.

use std::path::Path;
use std::sync::Mutex;

use crate::platform::dynamic_library;
use crate::platform::hash;

#[derive(Clone, Debug)]
struct Snapshot {
    opensteamtool_version: String,
    build_id: String,
    steamclient_path: String,
    steamclient_sha256: String,
    steamui_path: String,
    steamui_sha256: String,
}

static SNAPSHOT: Mutex<Option<Snapshot>> = Mutex::new(None);

const UNAVAILABLE: &str = "(unavailable)";

fn detect_steam_build_id() -> String {
    let steam = dynamic_library::get_loaded("steam.exe");
    if steam.is_null() { 
        LOG_WARN!("SteamDiagnostics: steam.exe module not loaded; build id unavailable");
        return UNAVAILABLE.into();
    }
    type GetBootstrapperVersionFn = unsafe extern "system" fn() -> i64;
    let f = unsafe { dynamic_library::get_symbol(steam, "GetBootstrapperVersion") };
    if f.is_null() {
        LOG_WARN!("SteamDiagnostics: steam.exe!GetBootstrapperVersion not exported");
        return UNAVAILABLE.into();
    }
    let f: GetBootstrapperVersionFn = unsafe { std::mem::transmute(f) };
    unsafe { f() }.to_string()
}

fn hash_or_unavailable(path: &str) -> String {
    let h = hash::sha256_of_file(Path::new(path));
    if h.is_empty() { UNAVAILABLE.into() } else { h }
}

pub fn initialize(steamclient_path: &str, steamui_path: &str) {
    let snap = Snapshot {
        opensteamtool_version: crate::VERSION.to_string(),
        build_id: detect_steam_build_id(),
        steamclient_path: steamclient_path.to_string(),
        steamclient_sha256: hash_or_unavailable(steamclient_path),
        steamui_path: steamui_path.to_string(),
        steamui_sha256: hash_or_unavailable(steamui_path),
    };
    LOG_INFO!(
        "SteamDiagnostics: ost.version={} build={} steamclient64.sha256={} steamui.sha256={}",
        snap.opensteamtool_version, snap.build_id,
        snap.steamclient_sha256, snap.steamui_sha256
    );
    *SNAPSHOT.lock().expect("snapshot mutex poisoned") = Some(snap);
}

pub fn sha256_of(path: &str) -> String {
    let g = SNAPSHOT.lock().expect("poisoned");
    if let Some(snap) = g.as_ref() {
        if path == snap.steamclient_path {
            return if snap.steamclient_sha256 == UNAVAILABLE { String::new() } else { snap.steamclient_sha256.clone() };
        }
        if path == snap.steamui_path {
            return if snap.steamui_sha256 == UNAVAILABLE { String::new() } else { snap.steamui_sha256.clone() };
        }
    }
    drop(g);
    hash::sha256_of_file(Path::new(path))
}

pub fn show_warning(title: String, message: String) {
    let full_message = {
        let g = SNAPSHOT.lock().expect("poisoned");
        let base = message;
        match g.as_ref() {
            Some(s) => format!(
                "{}\n\nSteam diagnostics:\n  rusty_tools version: {}\n  Build ID:              {}\n  steamclient64.dll SHA: {}\n  steamui.dll SHA:       {}",
                base, s.opensteamtool_version, s.build_id, s.steamclient_sha256, s.steamui_sha256
            ),
            None => base,
        }
    };
    std::thread::spawn(move || {
        crate::platform::dialog::show_warning(&title, &full_message);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_unknown_path_hashes_on_demand() {
        // A path we know isn't cached in the snapshot hashes on demand.
        let h = sha256_of("definitely-not-a-real-file-xyz");
        assert_eq!(h, "");
    }
}
