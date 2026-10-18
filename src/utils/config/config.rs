//! TOML configuration. Source: `Utils/Config/Config.cpp`, `Config.h`

use std::path::Path;
use std::sync::Mutex;

use serde::Deserialize;

use crate::bridge;

#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LogLevel { Trace, Debug, Info, Warn, Error }

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info  => "info",
            LogLevel::Warn  => "warn",
            LogLevel::Error => "error",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct ManifestTimeouts {
    pub resolve: u32, pub connect: u32, pub send: u32, pub recv: u32,
}

impl Default for ManifestTimeouts {
    fn default() -> Self { Self { resolve: 5000, connect: 5000, send: 10000, recv: 10000 } }
}

#[derive(Clone, Debug)]
pub struct InjectDll {
    pub path: String,
    pub when_cmdline: String,
    pub when_appids: std::collections::HashSet<u32>,
    pub all_games: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CloudSettings { pub enabled: bool, pub library: String }

#[derive(Clone, Debug)]
pub struct DonateSettings {
    pub enabled: bool, pub url: String,
    pub interval_secs: u32, pub max_mints_per_cycle: u32,
    pub min_mint_interval_ms: u32, pub max_mints_per_session: u32,
    pub wanted_refresh_secs: u32,
}

impl Default for DonateSettings {
    fn default() -> Self {
        Self { enabled: true, url: String::new(), interval_secs: 30,
               max_mints_per_cycle: 25, min_mint_interval_ms: 2000,
               max_mints_per_session: 0, wanted_refresh_secs: 300 }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct LoadResult { pub applied: bool, pub lua_paths_changed: bool }

#[derive(Clone, Debug)]
struct Snapshot {
    manifest_provider: String,
    manifest_timeouts: ManifestTimeouts,
    log_level: LogLevel,
    log_dir: String,
    lua_paths: Vec<String>,
    lua_dir: Option<String>,
    remote_url_templates: Vec<String>,
    stats_enable_api: bool,
    update_enabled: bool,
    donate: DonateSettings,
    inject_dlls: Vec<InjectDll>,
    tokeer_register_scheme: bool,
    cloud: CloudSettings,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            manifest_provider: "opensteamtool".into(),
            manifest_timeouts: ManifestTimeouts::default(),
            log_level: LogLevel::Debug,
            log_dir: String::new(),
            lua_paths: Vec::new(),
            lua_dir: None,
            remote_url_templates: Vec::new(),
            stats_enable_api: true,
            update_enabled: true,
            donate: DonateSettings::default(),
            inject_dlls: Vec::new(),
            cloud: CloudSettings::default(),
            tokeer_register_scheme: false,
        }
    }
}

struct State { snapshot: Snapshot, loaded_once: bool }

static STATE: Mutex<State> = Mutex::new(State {
    snapshot: Snapshot {
        manifest_provider: String::new(),
        manifest_timeouts: ManifestTimeouts { resolve: 5000, connect: 5000, send: 10000, recv: 10000 },
        log_level: LogLevel::Debug,
        log_dir: String::new(),
        lua_paths: Vec::new(),
        lua_dir: None,
        remote_url_templates: Vec::new(),
        stats_enable_api: true,
        update_enabled: true,
        donate: DonateSettings { enabled: true, url: String::new(), interval_secs: 30,
                                  max_mints_per_cycle: 25, min_mint_interval_ms: 2000,
                                  max_mints_per_session: 0, wanted_refresh_secs: 300 },
        inject_dlls: Vec::new(),
        cloud: CloudSettings { enabled: false, library: String::new() },
        tokeer_register_scheme: false,
    },
    loaded_once: false,
});

#[derive(Deserialize, Default)]
struct TomlManifest {
    url: Option<String>,
    timeout_resolve_ms: Option<i64>,
    timeout_connect_ms: Option<i64>,
    timeout_send_ms: Option<i64>,
    timeout_recv_ms: Option<i64>,
}
#[derive(Deserialize, Default)] struct TomlLog { level: Option<String> }
#[derive(Deserialize, Default)] struct TomlLua { paths: Option<Vec<String>>, dir: Option<String> }
#[derive(Deserialize, Default)] struct TomlRemote { url_template: Option<StringOrVec> }
#[derive(Deserialize)]
#[serde(untagged)]
enum StringOrVec { Single(String), Multi(Vec<String>) }
#[derive(Deserialize, Default)] struct TomlStats { enable_api: Option<bool> }
#[derive(Deserialize, Default)] struct TomlUpdate { enabled: Option<bool> }
#[derive(Deserialize, Default)]
struct TomlDonate {
    enabled: Option<bool>, url: Option<String>,
    interval_secs: Option<i64>, max_mints_per_cycle: Option<i64>,
    min_mint_interval_ms: Option<i64>, max_mints_per_session: Option<i64>,
    wanted_refresh_secs: Option<i64>,
}
#[derive(Deserialize, Default)]
struct TomlInject {
    path: Option<String>, when_cmdline: Option<String>,
    all_games: Option<bool>, when_appids: Option<Vec<i64>>,
}
#[derive(Deserialize, Default)] struct TomlCloud { enabled: Option<bool>, library: Option<String> }
#[derive(Deserialize, Default)] struct TomlTokeer { register_scheme: Option<bool> }

#[derive(Deserialize, Default)]
struct TomlConfig {
    #[serde(default)] manifest: TomlManifest,
    #[serde(default)] log: TomlLog,
    #[serde(default)] lua: TomlLua,
    #[serde(default)] remote: TomlRemote,
    #[serde(default)] stats: TomlStats,
    #[serde(default)] update: TomlUpdate,
    #[serde(default)] donate: TomlDonate,
    #[serde(default)] inject: Vec<TomlInject>,
    #[serde(default)] cloud: TomlCloud,
    #[serde(default)] tokeer: TomlTokeer,
}

fn parse_log_level(s: &str) -> Option<LogLevel> {
    match s {
        "trace" => Some(LogLevel::Trace), "debug" => Some(LogLevel::Debug),
        "info"  => Some(LogLevel::Info),  "warn"  => Some(LogLevel::Warn),
        "error" => Some(LogLevel::Error), _ => None,
    }
}

fn clamp_i64(v: Option<i64>, lo: u32, hi: u32, cur: u32, key: &str) -> u32 {
    let Some(v) = v else { return cur };
    if v < lo as i64 || v > hi as i64 {
        LOG_WARN!("[donate] {} = {} out of range [{}, {}], keeping {}", key, v, lo, hi, cur);
        return cur;
    }
    v as u32
}

fn snapshot_from_toml(toml_cfg: &TomlConfig, config_path: &Path) -> Snapshot {
    let mut s = Snapshot::default();
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    s.log_dir = parent.join("rustytools").to_string_lossy().into_owned();

    if let Some(v) = &toml_cfg.manifest.url { s.manifest_provider = v.clone(); }
    if let Some(v) = toml_cfg.manifest.timeout_resolve_ms { s.manifest_timeouts.resolve = v as u32; }
    if let Some(v) = toml_cfg.manifest.timeout_connect_ms { s.manifest_timeouts.connect = v as u32; }
    if let Some(v) = toml_cfg.manifest.timeout_send_ms    { s.manifest_timeouts.send    = v as u32; }
    if let Some(v) = toml_cfg.manifest.timeout_recv_ms    { s.manifest_timeouts.recv    = v as u32; }

    if let Some(v) = &toml_cfg.log.level {
        if let Some(lvl) = parse_log_level(v) { s.log_level = lvl; }
    }
    if let Some(v) = &toml_cfg.lua.paths { s.lua_paths = v.clone(); }
    if let Some(v) = &toml_cfg.lua.dir { s.lua_dir = Some(v.clone()); }
    if let Some(rt) = &toml_cfg.remote.url_template {
        match rt {
            StringOrVec::Single(x) => s.remote_url_templates.push(x.clone()),
            StringOrVec::Multi(xs) => s.remote_url_templates.extend(xs.iter().cloned()),
        }
    }
    if let Some(v) = toml_cfg.stats.enable_api { s.stats_enable_api = v; }
    if let Some(v) = toml_cfg.update.enabled   { s.update_enabled   = v; }

    let d = &toml_cfg.donate;
    if let Some(v) = d.enabled { s.donate.enabled = v; }
    if let Some(v) = &d.url   { s.donate.url = v.clone(); }
    s.donate.interval_secs        = clamp_i64(d.interval_secs,        30,  86400, s.donate.interval_secs,        "interval_secs");
    s.donate.max_mints_per_cycle  = clamp_i64(d.max_mints_per_cycle,   1,    500, s.donate.max_mints_per_cycle,  "max_mints_per_cycle");
    s.donate.min_mint_interval_ms = clamp_i64(d.min_mint_interval_ms,  0,  60000, s.donate.min_mint_interval_ms, "min_mint_interval_ms");
    s.donate.max_mints_per_session= clamp_i64(d.max_mints_per_session, 0, 100000, s.donate.max_mints_per_session,"max_mints_per_session");
    s.donate.wanted_refresh_secs  = clamp_i64(d.wanted_refresh_secs,  30,  86400, s.donate.wanted_refresh_secs,  "wanted_refresh_secs");

    let steam_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    for entry in &toml_cfg.inject {
        let Some(p) = &entry.path else { continue };
        if p.is_empty() { continue; }
        let full = { let fp = Path::new(p); if fp.is_relative() { steam_dir.join(fp) } else { fp.to_path_buf() } };
        if !full.exists() { LOG_WARN!("inject dll not found: {}", full.display()); continue; }
        let mut when = std::collections::HashSet::new();
        if let Some(ids) = &entry.when_appids { for &id in ids { when.insert(id as u32); } }
        s.inject_dlls.push(InjectDll {
            path: full.to_string_lossy().into_owned(),
            when_cmdline: entry.when_cmdline.clone().unwrap_or_default(),
            when_appids: when,
            all_games: entry.all_games.unwrap_or(false),
        });
    }
    if let Some(v) = toml_cfg.cloud.enabled { s.cloud.enabled = v; }
    if let Some(v) = &toml_cfg.cloud.library { s.cloud.library = v.clone(); }
    if let Some(v) = toml_cfg.tokeer.register_scheme { s.tokeer_register_scheme = v; }
    s
}

fn apply_snapshot(snapshot: &Snapshot) -> LoadResult {
    let mut g = STATE.lock().expect("config mutex poisoned");
    let lua_changed = g.snapshot.lua_paths != snapshot.lua_paths;
    g.snapshot = snapshot.clone();
    g.loaded_once = true;
    LoadResult { applied: true, lua_paths_changed: lua_changed }
}

fn apply_manifest_provider(name: &str) {
    if !bridge::manifest_set_provider(name) {
        LOG_WARN!("Unknown manifest.url \"{}\", keeping default", name);
        bridge::manifest_set_provider("opensteamtool");
    }
}

pub fn load(config_path: &str) -> LoadResult {
    let path = Path::new(config_path);
    let toml_cfg = if path.exists() {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<TomlConfig>(&text) {
                Ok(t) => t,
                Err(e) => {
                    LOG_WARN!("Config parse error: {}", e);
                    let g = STATE.lock().expect("poisoned");
                    if !g.loaded_once {
                        drop(g);
                        let s = Snapshot::default();
                        apply_manifest_provider(&s.manifest_provider);
                        return apply_snapshot(&s);
                    }
                    return LoadResult::default();
                }
            },
            Err(e) => {
                LOG_WARN!("Config read error: {}", e);
                let g = STATE.lock().expect("poisoned");
                if !g.loaded_once {
                    drop(g);
                    let s = Snapshot::default();
                    apply_manifest_provider(&s.manifest_provider);
                    return apply_snapshot(&s);
                }
                return LoadResult::default();
            }
        }
    } else {
        LOG_INFO!("Config file not found, using defaults");
        let s = Snapshot::default();
        apply_manifest_provider(&s.manifest_provider);
        let r = apply_snapshot(&s);
        log_loaded();
        return r;
    };

    let snapshot = snapshot_from_toml(&toml_cfg, path);
    apply_manifest_provider(&snapshot.manifest_provider);
    let r = apply_snapshot(&snapshot);
    log_loaded();
    r
}

fn log_loaded() {
    let g = STATE.lock().expect("poisoned");
    LOG_INFO!(
        "Config loaded: manifest.url={} log.level={} lua.paths={} stats.enable_api={} remote.url_templates={}",
        bridge::manifest_active_provider_name(),
        g.snapshot.log_level.as_str(),
        g.snapshot.lua_paths.len(),
        g.snapshot.stats_enable_api,
        g.snapshot.remote_url_templates.len(),
    );
}

macro_rules! getter {
    ($name:ident, $ty:ty, $field:ident) => {
        pub fn $name() -> $ty { STATE.lock().expect("poisoned").snapshot.$field.clone() }
    };
}
getter!(get_lua_paths, Vec<String>, lua_paths);
pub fn get_lua_dir(steam_root: &str) -> String {
    let g = STATE.lock().expect("poisoned");
    if let Some(dir) = &g.snapshot.lua_dir {
        if !dir.is_empty() {
            let p = std::path::Path::new(dir);
            if p.is_absolute() {
                return dir.clone();
            } else {
                return format!("{}\\{}", steam_root, dir);
            }
        }
    }
    format!("{}\\config\\stplug-in", steam_root)
}
getter!(get_remote_url_templates, Vec<String>, remote_url_templates);
getter!(get_log_dir, String, log_dir);
getter!(get_inject_dlls, Vec<InjectDll>, inject_dlls);

pub fn get_log_level() -> LogLevel { STATE.lock().expect("poisoned").snapshot.log_level }
pub fn get_stats_enable_api() -> bool { STATE.lock().expect("poisoned").snapshot.stats_enable_api }
pub fn get_update_enabled() -> bool { STATE.lock().expect("poisoned").snapshot.update_enabled }
pub fn get_manifest_timeouts() -> ManifestTimeouts { STATE.lock().expect("poisoned").snapshot.manifest_timeouts }
pub fn get_donate_settings() -> DonateSettings { STATE.lock().expect("poisoned").snapshot.donate.clone() }
pub fn get_cloud_settings() -> CloudSettings { STATE.lock().expect("poisoned").snapshot.cloud.clone() }
pub fn get_tokeer_register_scheme() -> bool { STATE.lock().expect("poisoned").snapshot.tokeer_register_scheme }

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn parse_log_level_covers_all() {
        assert_eq!(parse_log_level("trace"), Some(LogLevel::Trace));
        assert_eq!(parse_log_level("error"), Some(LogLevel::Error));
        assert_eq!(parse_log_level("nope"), None);
    }
    #[test] fn default_snapshot_matches_cpp() {
        let s = Snapshot::default();
        assert_eq!(s.manifest_provider, "opensteamtool");
        assert_eq!(s.manifest_timeouts.resolve, 5000);
        assert_eq!(s.manifest_timeouts.recv,    10000);
        assert!(s.stats_enable_api);
        assert!(s.update_enabled);
        assert!(s.donate.enabled);
        assert_eq!(s.donate.interval_secs, 30);
    }
    #[test] fn toml_parses_known_shape() {
        let text = r#"
[manifest]
url = "steamrun"
timeout_recv_ms = 20000
[log]
level = "info"
[lua]
paths = ["C:\\a", "C:\\b"]
[donate]
interval_secs = 60
max_mints_per_cycle = 10
[cloud]
enabled = true
library = "cloud_redirect.dll"
"#;
        let t: TomlConfig = toml::from_str(text).unwrap();
        let s = snapshot_from_toml(&t, Path::new("C:\\steam\\opensteamtool.toml"));
        assert_eq!(s.manifest_provider, "steamrun");
        assert_eq!(s.manifest_timeouts.recv, 20000);
        assert_eq!(s.log_level, LogLevel::Info);
        assert_eq!(s.lua_paths.len(), 2);
        assert_eq!(s.donate.interval_secs, 60);
        assert_eq!(s.donate.max_mints_per_cycle, 10);
        assert!(s.cloud.enabled);
    }
    #[test] fn remote_url_template_accepts_both_shapes() {
        let single: TomlConfig = toml::from_str(r#"[remote]
url_template = "https://x/{sha256}""#).unwrap();
        let s = snapshot_from_toml(&single, Path::new("a.toml"));
        assert_eq!(s.remote_url_templates, vec!["https://x/{sha256}"]);
        let multi: TomlConfig = toml::from_str(r#"[remote]
url_template = ["https://a", "https://b"]"#).unwrap();
        let s = snapshot_from_toml(&multi, Path::new("a.toml"));
        assert_eq!(s.remote_url_templates, vec!["https://a", "https://b"]);
    }
}
