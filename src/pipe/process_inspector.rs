//! Process snapshot + Steam environment query. Source:
//! `Pipe/ProcessInspector.cpp`.

use crate::platform::numbers;
use crate::platform::process;
use crate::platform::stopwatch::Stopwatch;
use crate::steam::types::AppId_t;

pub const K_STEAM_PROCESS_NAMES: [&str; 6] = [
    "steam.exe",
    "steamwebhelper.exe",
    "steamservice.exe",
    "steamerrorreporter.exe",
    "gameoverlayui.exe",
    "gameoverlayui64.exe",
];

#[derive(Clone, Debug, Default)]
pub struct ProcessEnvironment {
    pub steam_overlay_game_id_app_id: Option<AppId_t>,
    pub steam_game_id_app_id: Option<AppId_t>,
    pub steam_app_id: Option<AppId_t>,
}

impl ProcessEnvironment {
    pub fn resolve_app_id(&self) -> AppId_t {
        if let Some(id) = self.steam_overlay_game_id_app_id { return id; }
        if let Some(id) = self.steam_game_id_app_id { return id; }
        self.steam_app_id.unwrap_or(0)
    }

    pub fn has_steam_app_environment(&self) -> bool {
        self.steam_app_id.is_some()
            || self.steam_game_id_app_id.is_some()
            || self.steam_overlay_game_id_app_id.is_some()
    }

    pub fn debug_string(&self) -> String {
        format!(
            "SteamAppId={} SteamGameId={} SteamOverlayGameId={} resolvedAppId={}",
            self.steam_app_id.unwrap_or(0),
            self.steam_game_id_app_id.unwrap_or(0),
            self.steam_overlay_game_id_app_id.unwrap_or(0),
            self.resolve_app_id(),
        )
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub creation_time: u64,
    pub image_path: String,
    pub image_name: String,
    pub steam_client_process: bool,
    pub likely_game_process: bool,
    pub environment: ProcessEnvironment,
}

impl ProcessSnapshot {
    pub fn resolve_app_id(&self) -> AppId_t {
        self.environment.resolve_app_id()
    }

    pub fn debug_string(&self) -> String {
        format!(
            "pid={} creation={} image={} steamClientProcess={} likelyGameProcess={} env=[{}]",
            self.pid,
            format!(
                "{} filetime={}",
                process::format_creation_time(self.creation_time),
                self.creation_time,
            ),
            if self.image_name.is_empty() { "-" } else { &self.image_name },
            self.steam_client_process,
            self.likely_game_process,
            self.environment.debug_string(),
        )
    }
}

fn base_name_from_path(path: &str) -> String {
    match path.rfind(|c| c == '\\' || c == '/') {
        Some(i) => path[i + 1..].to_string(),
        None => path.to_string(),
    }
}

fn app_id_from_game_id_string(value: &str) -> Option<AppId_t> {
    let parsed = numbers::parse_u64(value)?;
    let app_id = (parsed & 0x00FF_FFFF) as u32;
    if app_id == 0 { None } else { Some(app_id) }
}

fn app_id_from_appid_string(value: &str) -> Option<AppId_t> {
    let parsed = numbers::parse_u32(value)?;
    if parsed == 0 { None } else { Some(parsed) }
}

fn wide(s: &str) -> Vec<u16> { s.encode_utf16().collect() }

pub fn get_process_creation_time(pid: u32) -> Option<u64> {
    process::get_creation_time(pid)
}

pub fn is_steam_process_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    K_STEAM_PROCESS_NAMES.iter().any(|n| *n == lower)
}

pub fn read_steam_environment(pid: u32) -> ProcessEnvironment {
    let mut env = ProcessEnvironment::default();
    if let Some(v) = process::get_environment_variable_value(pid, &wide("SteamAppId")) {
        env.steam_app_id = app_id_from_appid_string(&v);
    }
    if let Some(v) = process::get_environment_variable_value(pid, &wide("SteamGameId")) {
        env.steam_game_id_app_id = app_id_from_game_id_string(&v);
    }
    if let Some(v) = process::get_environment_variable_value(pid, &wide("SteamOverlayGameId")) {
        env.steam_overlay_game_id_app_id = app_id_from_game_id_string(&v);
    }
    LOG_PIPE_DEBUG!("ProcessInspector: pid={} steam env {}", pid, env.debug_string());
    env
}

pub fn inspect_process(pid: u32) -> ProcessSnapshot {
    let timer = Stopwatch::new();
    let mut snapshot = ProcessSnapshot {
        pid,
        ..Default::default()
    };
    snapshot.creation_time = get_process_creation_time(pid).unwrap_or(0);

    if let Some(p) = process::get_image_path(pid) {
        snapshot.image_path = p;
        snapshot.image_name = base_name_from_path(&snapshot.image_path);
    }
    snapshot.steam_client_process = is_steam_process_name(&snapshot.image_name);
    snapshot.environment = read_steam_environment(pid);
    snapshot.likely_game_process =
        !snapshot.steam_client_process && snapshot.environment.has_steam_app_environment();

    LOG_PIPE_INFO!(
        "ProcessInspector: inspected {} elapsed_ms={:.3}",
        snapshot.debug_string(),
        timer.elapsed_ms(),
    );
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_name_variants() {
        assert_eq!(base_name_from_path("C:\\a\\b\\c.exe"), "c.exe");
        assert_eq!(base_name_from_path("c.exe"), "c.exe");
        assert_eq!(base_name_from_path("C:/a/b/c.exe"), "c.exe");
    }

    #[test]
    fn app_id_from_game_id_masks_24_bits() {
        // Mask keeps 24 bits. 730 = 0x2DA, well within range.
        assert_eq!(app_id_from_game_id_string("730"), Some(730));
        // 0x1100001000000042 -> masked to 0x42. Use the literal here
        // rather than a decimal transcription: the equivalence between
        // the two representations is exactly what a hand-typed decimal
        // gets wrong.
        let big: u64 = 0x1100_0010_0000_0042;
        assert_eq!(app_id_from_game_id_string(&big.to_string()), Some(0x42));
        // Zero masked -> None
        assert_eq!(app_id_from_game_id_string("0"), None);
        // Non-numeric
        assert_eq!(app_id_from_game_id_string("xyz"), None);
    }

    #[test]
    fn app_id_from_appid_rejects_zero() {
        assert_eq!(app_id_from_appid_string("730"), Some(730));
        assert_eq!(app_id_from_appid_string("0"), None);
        assert_eq!(app_id_from_appid_string("xyz"), None);
    }

    #[test]
    fn steam_process_name_case_insensitive() {
        assert!(is_steam_process_name("steam.exe"));
        assert!(is_steam_process_name("STEAM.EXE"));
        assert!(is_steam_process_name("SteamWebHelper.exe"));
        assert!(!is_steam_process_name("notsteam.exe"));
    }

    #[test]
    fn environment_resolve_prefers_overlay_then_game_then_app() {
        let mut env = ProcessEnvironment::default();
        assert_eq!(env.resolve_app_id(), 0);
        assert!(!env.has_steam_app_environment());

        env.steam_app_id = Some(100);
        assert_eq!(env.resolve_app_id(), 100);

        env.steam_game_id_app_id = Some(200);
        assert_eq!(env.resolve_app_id(), 200);

        env.steam_overlay_game_id_app_id = Some(300);
        assert_eq!(env.resolve_app_id(), 300);

        assert!(env.has_steam_app_environment());
    }
}
