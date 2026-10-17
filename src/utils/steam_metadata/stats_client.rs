//! Stats API client. Source: `StatsClient.cpp`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::platform::http;
use crate::platform::numbers;
use crate::utils::config::config;

/// How long a failed lookup is remembered. Prevents every achievement
/// query for an app whose stats API entry is missing from hammering the
/// endpoint in a loop.
const MISS_TTL: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Cache {
    hits: HashMap<u32, u64>,
    misses: HashMap<u32, Instant>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

pub fn fetch_stat_steam_id(app_id: u32) -> Option<u64> {
    if app_id == 0 { return None; }
    if !config::get_stats_enable_api() {
        LOG_ACHIEVEMENT_DEBUG!("Stats SteamID API disabled for appid={}", app_id);
        return None;
    }

    {
        let mut g = CACHE.lock().expect("stats cache poisoned");
        let c = g.get_or_insert_with(Cache::default);
        if let Some(v) = c.hits.get(&app_id).copied() {
            LOG_ACHIEVEMENT_DEBUG!("Stats SteamID API cache hit appid={} steamid={}", app_id, v);
            return Some(v);
        }
        if c.misses.get(&app_id).map_or(false, |t| t.elapsed() < MISS_TTL) {
            LOG_ACHIEVEMENT_DEBUG!("Stats SteamID API miss cached appid={}", app_id);
            return None;
        }
    }

    let url = format!("https://stats.opensteamtool.com/{}", app_id);
    let req = http::Request::new("GET", &url);
    let r = http::execute(&req);
    LOG_ACHIEVEMENT_INFO!("Stats SteamID API status={} appid={}", r.status, app_id);

    let parsed = if !r.ok || r.status != 200 {
        LOG_ACHIEVEMENT_WARN!("Stats SteamID API failed appid={} status={} ok={}", app_id, r.status, r.ok);
        None
    } else {
        std::str::from_utf8(&r.body).ok()
            .and_then(|s| numbers::parse_u64(s))
            .filter(|v| *v != 0)
    };

    let mut g = CACHE.lock().expect("stats cache poisoned");
    let c = g.as_mut().unwrap();
    if let Some(v) = parsed {
        c.hits.insert(app_id, v);
        c.misses.remove(&app_id);
        LOG_ACHIEVEMENT_INFO!("Stats SteamID API resolved appid={} steamid={}", app_id, v);
        Some(v)
    } else {
        c.misses.insert(app_id, Instant::now());
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_app_id_returns_none() {
        assert_eq!(fetch_stat_steam_id(0), None);
    }
}