//! Lua configuration. Source: `Utils/Config/LuaConfig.cpp`, `LuaConfig.h`
//!
//! Uses `mlua` 0.9 (Lua 5.4 vendored). State is a global
//! `Mutex<Option<State>>`; every public query binds the result to a
//! local before returning, so the `MutexGuard` temporaries drop in the
//! right order.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use mlua::{Lua, Value};

use crate::bridge;

#[derive(Copy, Clone, Debug, Default)]
pub struct ManifestOverride { pub gid: u64, pub size: u64 }

#[derive(Default)]
struct Store {
    depot_key_set: HashMap<u32, String>,
    access_token_set: HashMap<u32, u64>,
    legacy_cd_key_set: HashMap<u32, String>,
    pinned_apps: HashSet<u32>,
    manifest_overrides: HashMap<u64, ManifestOverride>,
    stat_steam_id_set: HashMap<u32, u64>,
    owned_app_id_set: HashSet<u32>,
    process_name_app_id_map: HashMap<String, u32>,
    forced_denuvo_set: HashSet<u32>,
    eticket_url: String,
    current_file: String,
    file_depots: HashMap<String, HashSet<u32>>,
    file_manifest_overrides: HashMap<String, HashMap<u64, ManifestOverride>>,
    file_parse_sequence: HashMap<String, u64>,
    next_file_parse_sequence: u64,
    depot_ref_count: HashMap<u32, u32>,
    file_mtime: HashMap<String, u32>,
    purchase_time: HashMap<u32, u32>,
    pending_removals: Vec<u32>,
    pending_additions: Vec<u32>,
    has_manifest_code_func: bool,
    has_manifest_code_func_ex: bool,
}

struct State { lua: Lua, store: Arc<Mutex<Store>> }

static STATE: Mutex<Option<State>> = Mutex::new(None);

const DEFAULT_STAT_STEAM_ID: u64 = 76561198028121353;

fn parse_hex_byte(s: &str) -> u8 {
    crate::platform::numbers::parse_hex_u8(s).unwrap_or(0)
}

fn rebuild_manifest_override(store: &mut Store, depot_id: u64) {
    let mut best: Option<ManifestOverride> = None;
    let mut best_seq: u64 = 0;
    for (file, overrides) in &store.file_manifest_overrides {
        let Some(ov) = overrides.get(&depot_id) else { continue };
        let Some(&seq) = store.file_parse_sequence.get(file) else { continue };
        if best.is_none() || seq > best_seq { best = Some(*ov); best_seq = seq; }
    }
    match best {
        Some(ov) => { store.manifest_overrides.insert(depot_id, ov); }
        None     => { store.manifest_overrides.remove(&depot_id); }
    }
}


/// Serialise a Lua `{ key = value, ... }` table into the
/// `"Key: Value\r\n"` form that WinHTTP request headers take.
///
/// Matches the C++ `LuaHeadersToWstr`. Keys and values are
/// stringified with `to_string_lossy`; non-scalar entries (tables,
/// functions, userdata) are skipped, which mirrors `lua_tostring`
/// returning NULL for those types.
fn headers_table_to_string(tbl: mlua::Table<'_>) -> mlua::Result<String> {
    let mut out = String::new();
    for pair in tbl.pairs::<Value, Value>() {
        let (k, v) = pair?;
        let key = match k {
            Value::String(s) => s.to_string_lossy().into_owned(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => continue,
        };
        let val = match v {
            Value::String(s) => s.to_string_lossy().into_owned(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => continue,
        };
        out.push_str(&key);
        out.push_str(": ");
        out.push_str(&val);
        out.push_str("\r\n");
    }
    Ok(out)
}

/// Shared body for the Lua `http_get` / `http_post` functions.
///
/// The C++ implementation routes through `OSTPlatform::Http::Execute`
/// (WinHTTP); this does the same through `platform::http::execute`.
/// Timeouts are the platform defaults, matching the C++ call sites
/// that omit the timeout arguments.
fn do_http<'lua>(
    lua: &'lua Lua,
    method: &str,
    url: &str,
    body: Option<&[u8]>,
    headers: Option<mlua::Table<'lua>>,
) -> mlua::Result<(Value<'lua>, Value<'lua>)> {
    let header_string = match headers {
        Some(tbl) => headers_table_to_string(tbl)?,
        None => String::new(),
    };

    let mut req = crate::platform::http::Request::new(method, url);
    req.body = body;
    if !header_string.is_empty() {
        req.headers = Some(header_string.as_str());
    }

    let resp = crate::platform::http::execute(&req);
    if resp.ok {
        let body_val = lua.create_string(&resp.body)?;
        Ok((Value::String(body_val), Value::Integer(resp.status as i64)))
    } else {
        let msg = lua.create_string(b"HTTP request failed")?;
        Ok((Value::Nil, Value::String(msg)))
    }
}

/// Install a metatable on `_G` whose `__index` lowercases the
/// requested name and re-tries the raw lookup. Matches the C++
/// `case_insensitive_global_index`: `setAppTicket`, `SETAPPTICKET`,
/// `SetAppticket`, and `setappticket` all resolve to the same
/// function.
///
/// The metamethod only fires on a *miss*, so the common case (a
/// lowercase name present in the globals table) is not affected.
/// The `raw_get` inside the metamethod bypasses the metatable,
/// avoiding recursion into the same metamethod.
fn install_case_insensitive_index(lua: &Lua) {
    let globals = lua.globals();
    let mt = match lua.create_table() {
        Ok(t) => t,
        Err(_) => return,
    };
    let index = match lua.create_function(|lua, (_tbl, key): (mlua::Table, Value)| {
        // Only string keys can be case-folded.
        let Value::String(s) = key else { return Ok(Value::Nil); };
        let name = s.to_string_lossy();
        let lower = name.to_ascii_lowercase();
        if lower == name.as_ref() {
            // The raw lookup in `_G` already failed for this exact
            // spelling; there is no lowercased variant to try.
            return Ok(Value::Nil);
        }
        // `raw_get` avoids recursing back into this metamethod.
        let globals = lua.globals();
        globals.raw_get::<_, Value>(lower)
    }) {
        Ok(f) => f,
        Err(_) => return,
    };
    if mt.set("__index", index).is_err() { return; }
    let _ = globals.set_metatable(Some(mt));
}

fn initialize() -> bool {
    let mut guard = STATE.lock().expect("poisoned");
    if guard.is_some() { return true; }
    let lua = Lua::new();
    let store = Arc::new(Mutex::new(Store::default()));
    register_all(&lua, &store);
    install_case_insensitive_index(&lua);
    *guard = Some(State { lua, store });
    true
}

/// Reject negative or overflowing ids. Returns `Err` from the calling
/// Lua function so a typo in a user script fails at parse time rather
/// than silently poisoning the depot set.
fn validate_id(id: i64, fname: &str) -> mlua::Result<u32> {
    if id < 0 || id as u64 > u32::MAX as u64 {
        return Err(mlua::Error::RuntimeError(format!(
            "{}: id out of range ({})", fname, id
        )));
    }
    Ok(id as u32)
}

fn register_all(lua: &Lua, store: &Arc<Mutex<Store>>) {
    // addappid(id, [flag], [key_hex])
    let s = store.clone();
    let f = lua.create_function(move |_, (id, a, b): (i64, Option<Value>, Option<Value>)| {
        let depot_id = validate_id(id, "addappid")?;
        let mut st = s.lock().unwrap();
        let mut key = String::new();
        // Accept both (id, key) and (id, 0, key) shapes.
        let third_str = b.as_ref().and_then(|v| match v { Value::String(s) => s.to_str().ok().map(|c| c.to_owned()), _ => None });
        let second_str = a.as_ref().and_then(|v| match v { Value::String(s) => s.to_str().ok().map(|c| c.to_owned()), _ => None });
        if let Some(k) = third_str.or(second_str) { if k.len() == 64 { key = k; } }
        if !key.is_empty() || !st.depot_key_set.contains_key(&depot_id) {
            st.depot_key_set.insert(depot_id, key);
        }
        if !st.current_file.is_empty() {
            let cur = st.current_file.clone();
            let entry = st.file_depots.entry(cur).or_default();
            if entry.insert(depot_id) {
                let rc = st.depot_ref_count.entry(depot_id).or_insert(0);
                *rc += 1;
                if *rc == 1 { st.pending_additions.push(depot_id); }
                if let Some(&mt) = st.file_mtime.get(&st.current_file) {
                    let slot = st.purchase_time.entry(depot_id).or_insert(0);
                    if mt > *slot { *slot = mt; }
                }
            }
        }
        Ok(())
    }).unwrap();
    lua.globals().set("addappid", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, (id, token): (i64, String)| {
        let depot_id = validate_id(id, "addtoken")?;
        let mut st = s.lock().unwrap();
        if let Some(v) = crate::platform::numbers::parse_u64(&token) {
            st.access_token_set.insert(depot_id, v);
        }
        Ok(())
    }).unwrap();
    lua.globals().set("addtoken", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, (id, key): (i64, String)| {
        let depot_id = validate_id(id, "setlegacycdkey")?;
        let mut st = s.lock().unwrap();
        st.legacy_cd_key_set.insert(depot_id, key);
        Ok(())
    }).unwrap();
    lua.globals().set("setlegacycdkey", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, (id, name): (i64, String)| {
        let app_id = validate_id(id, "addprocess")?;
        let mut st = s.lock().unwrap();
        st.process_name_app_id_map.insert(name.to_ascii_lowercase(), app_id);
        Ok(())
    }).unwrap();
    lua.globals().set("addprocess", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, id: i64| {
        let app_id = validate_id(id, "forcedenuvo")?;
        let mut st = s.lock().unwrap();
        st.forced_denuvo_set.insert(app_id);
        Ok(())
    }).unwrap();
    lua.globals().set("forcedenuvo", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, url: String| {
        let mut st = s.lock().unwrap();
        st.eticket_url = url;
        Ok(())
    }).unwrap();
    lua.globals().set("seteticketurl", f).ok();

    let s = store.clone();
    let f = lua.create_function(move |_, (id, gid_str, size): (i64, String, Option<u64>)| {
        let depot_id = validate_id(id, "setManifestid")? as u64;
        let mut st = s.lock().unwrap();
        let Some(gid) = crate::platform::numbers::parse_u64(&gid_str) else {
            return Err(mlua::Error::RuntimeError("setManifestid: gid must be all digits".into()));
        };
        let ov = ManifestOverride { gid, size: size.unwrap_or(0) };
        if !st.current_file.is_empty() {
            let cur = st.current_file.clone();
            st.file_manifest_overrides.entry(cur).or_default().insert(depot_id, ov);
            rebuild_manifest_override(&mut st, depot_id);
        } else {
            st.manifest_overrides.insert(depot_id, ov);
        }
        Ok(())
    }).unwrap();
    lua.globals().set("setmanifestid", f).ok();


    let s = store.clone();
    let f = lua.create_function(move |_, (app_id, steam_id): (i64, String)| {
        let app_id = validate_id(app_id, "setStat")?;
        let mut st = s.lock().unwrap();
        let Some(v) = crate::platform::numbers::parse_u64(&steam_id) else {
            return Err(mlua::Error::RuntimeError("setStat: steamId must be all digits".into()));
        };
        st.stat_steam_id_set.insert(app_id, v);
        Ok(())
    }).unwrap();
    lua.globals().set("setstat", f).ok();

    // http_get / http_post. Real WinHTTP calls through
    // `platform::http::execute`. The return shape matches the C++
    // documented contract:
    //   success -> (body_string, status_code)
    //   failure -> (nil, "HTTP request failed")
    let f = lua.create_function(|lua, (url, headers): (String, Option<mlua::Table>)| {
        do_http(lua, "GET", &url, None, headers)
    }).unwrap();
    lua.globals().set("http_get", f).ok();

    let f = lua.create_function(|lua, (url, body, headers): (String, String, Option<mlua::Table>)| {
        do_http(lua, "POST", &url, Some(body.as_bytes()), headers)
    }).unwrap();
    lua.globals().set("http_post", f).ok();
}

// ── Public API ──────────────────────────────────────────────────────

pub fn has_depot(app_id: u32, exclude_owned: bool) -> bool {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return false };
    let store = state.store.lock().unwrap();
    store.depot_key_set.contains_key(&app_id)
        && (!exclude_owned || !store.owned_app_id_set.contains(&app_id))
}

pub fn is_owned(app_id: u32) -> bool {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return false };
    let store = state.store.lock().unwrap();
    store.owned_app_id_set.contains(&app_id)
}

pub fn mark_owned(app_id: u32) {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return };
    let mut store = state.store.lock().unwrap();
    if store.owned_app_id_set.insert(app_id) {
        LOG_PACKAGE_INFO!("Marking app {} as owned", app_id);
    }
}

pub fn get_all_depot_ids() -> Vec<u32> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return Vec::new() };
    let store = state.store.lock().unwrap();
    store.depot_key_set.keys().copied().collect()
}

pub fn get_decryption_key(depot_id: u32) -> Vec<u8> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return Vec::new() };
    let store = state.store.lock().unwrap();
    let Some(key_str) = store.depot_key_set.get(&depot_id) else { return Vec::new() };
    let mut out = Vec::with_capacity(key_str.len() / 2);
    let b = key_str.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        let s: String = [b[i] as char, b[i + 1] as char].iter().collect();
        out.push(parse_hex_byte(&s));
        i += 2;
    }
    out
}

pub fn get_access_token(app_id: u32) -> u64 {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return 0 };
    let store = state.store.lock().unwrap();
    *store.access_token_set.get(&app_id).unwrap_or(&0)
}

pub fn get_legacy_cd_key(app_id: u32) -> Option<String> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return None };
    let store = state.store.lock().unwrap();
    store.legacy_cd_key_set.get(&app_id).cloned()
}

pub fn get_stat_steam_id(app_id: u32) -> u64 {
    let explicit: Option<u64> = {
        let guard = STATE.lock().unwrap();
        let s = guard.as_ref()
            .and_then(|s| s.store.lock().unwrap().stat_steam_id_set.get(&app_id).copied());
        s
    };
    if let Some(v) = explicit { return v; }
    if let Some(v) = bridge::stats_fetch_stat_steam_id(app_id) { return v; }
    DEFAULT_STAT_STEAM_ID
}

pub fn is_pinned(app_id: u32) -> bool {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return false };
    let store = state.store.lock().unwrap();
    store.pinned_apps.contains(&app_id)
}

pub fn get_purchase_time(app_id: u32) -> u32 {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return 0 };
    let store = state.store.lock().unwrap();
    *store.purchase_time.get(&app_id).unwrap_or(&0)
}

pub fn get_manifest_overrides() -> HashMap<u64, ManifestOverride> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return HashMap::new() };
    let store = state.store.lock().unwrap();
    store.manifest_overrides.clone()
}

pub fn get_app_id_for_process(image_name: &str) -> u32 {
    let lower = image_name.to_ascii_lowercase();
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return 0 };
    let store = state.store.lock().unwrap();
    *store.process_name_app_id_map.get(&lower).unwrap_or(&0)
}

pub fn is_forced_denuvo(app_id: u32) -> bool {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return false };
    let store = state.store.lock().unwrap();
    store.forced_denuvo_set.contains(&app_id)
}

pub fn get_eticket_url() -> String {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return String::new() };
    let store = state.store.lock().unwrap();
    store.eticket_url.clone()
}

pub fn take_pending_removals() -> Vec<u32> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return Vec::new() };
    let v = std::mem::take(&mut state.store.lock().unwrap().pending_removals);
    v
}

pub fn take_pending_additions() -> Vec<u32> {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return Vec::new() };
    let v = std::mem::take(&mut state.store.lock().unwrap().pending_additions);
    v
}

pub fn has_manifest_code_func() -> bool {
    let guard = STATE.lock().unwrap();
    guard.as_ref().map(|s| s.store.lock().unwrap().has_manifest_code_func).unwrap_or(false)
}

pub fn has_manifest_code_func_ex() -> bool {
    let guard = STATE.lock().unwrap();
    guard.as_ref().map(|s| s.store.lock().unwrap().has_manifest_code_func_ex).unwrap_or(false)
}

pub fn parse_file(file_path: &str) {
    if !initialize() { return; }
    unload_file(file_path);

    let contents = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(e) => { LOG_WARN!("ParseFile: failed to open {}: {}", file_path, e); return; }
    };
    let mtime = std::fs::metadata(file_path).and_then(|m| m.modified()).ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as u32).unwrap_or(0);

    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return };

    {
        let mut st = state.store.lock().unwrap();
        st.current_file = file_path.to_string();
        st.next_file_parse_sequence += 1;
        let seq = st.next_file_parse_sequence;
        st.file_parse_sequence.insert(file_path.to_string(), seq);
        st.file_mtime.insert(file_path.to_string(), mtime);
    }

    if let Err(e) = state.lua.load(&contents).set_name(file_path).exec() {
        LOG_WARN!("{}: {}", file_path, e);
    }

    let mut st = state.store.lock().unwrap();
    let g = state.lua.globals();
    let f1: Result<Value, _> = g.get("fetch_manifest_code");
    st.has_manifest_code_func = matches!(f1, Ok(Value::Function(_)));
    let f2: Result<Value, _> = g.get("fetch_manifest_code_ex");
    st.has_manifest_code_func_ex = matches!(f2, Ok(Value::Function(_)));
    st.current_file.clear();
}

pub fn unload_file(file_path: &str) {
    let guard = STATE.lock().unwrap();
    let Some(state) = guard.as_ref() else { return };
    let mut st = state.store.lock().unwrap();
    let Some(depots) = st.file_depots.remove(file_path) else {
        if let Some(overrides) = st.file_manifest_overrides.remove(file_path) {
            let affected: Vec<u64> = overrides.keys().copied().collect();
            for id in affected { rebuild_manifest_override(&mut st, id); }
        }
        st.file_parse_sequence.remove(file_path);
        st.file_mtime.remove(file_path);
        return;
    };

    for id in depots {
        if let Some(rc) = st.depot_ref_count.get_mut(&id) {
            *rc -= 1;
            if *rc == 0 {
                st.depot_ref_count.remove(&id);
                st.depot_key_set.remove(&id);
                st.purchase_time.remove(&id);
                st.pending_removals.push(id);
            }
        }
    }
    if let Some(overrides) = st.file_manifest_overrides.remove(file_path) {
        let affected: Vec<u64> = overrides.keys().copied().collect();
        for id in affected { rebuild_manifest_override(&mut st, id); }
    }
    st.file_parse_sequence.remove(file_path);
    st.file_mtime.remove(file_path);
}

fn collect_lua_files(directory: &str) -> Vec<String> {
    let dir = Path::new(directory);
    let _ = std::fs::create_dir_all(dir);
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return out };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("lua") {
            if let Some(s) = p.to_str() { out.push(s.to_string()); }
        }
    }
    out
}

pub fn parse_directory(directory: &str) {
    if !initialize() { return; }
    for path in collect_lua_files(directory) { parse_file(&path); }
    let guard = STATE.lock().unwrap();
    if let Some(state) = guard.as_ref() {
        state.store.lock().unwrap().pending_additions.clear();
    }
}

pub fn merge_watch_dirs(configured: &[String], default_dir: &str) -> Vec<String> {
    fn key(p: &str) -> String {
        let p = Path::new(p);
        let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        c.to_string_lossy().to_ascii_lowercase()
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let add = |dir: &str, out: &mut Vec<String>, seen: &mut HashSet<String>| {
        if dir.is_empty() { return; }
        let k = key(dir);
        if seen.insert(k) { out.push(dir.to_string()); }
    };
    for d in configured { add(d, &mut out, &mut seen); }
    add(default_dir, &mut out, &mut seen);
    out
}

pub fn reload_directories(directories: &[String], clear_pending: bool) {
    if !initialize() { return; }
    let mut active: HashSet<String> = HashSet::new();
    let mut ordered: Vec<String> = Vec::new();
    for d in directories {
        for path in collect_lua_files(d) {
            if active.insert(path.clone()) { ordered.push(path); }
        }
    }
    let tracked: Vec<String> = {
        let guard = STATE.lock().unwrap();
        let Some(state) = guard.as_ref() else { return };
        let st = state.store.lock().unwrap();
        let mut s: HashSet<String> = HashSet::new();
        for k in st.file_depots.keys() { s.insert(k.clone()); }
        for k in st.file_manifest_overrides.keys() { s.insert(k.clone()); }
        s.into_iter().collect()
    };
    for f in tracked { if !active.contains(&f) { unload_file(&f); } }
    for f in ordered { parse_file(&f); }
    if clear_pending {
        let guard = STATE.lock().unwrap();
        if let Some(state) = guard.as_ref() {
            state.store.lock().unwrap().pending_additions.clear();
        }
    }
}

// â”€â”€ Lua manifest-code callbacks â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
//
// Ported from `LuaConfig.cpp`'s `CallManifestFetchCode` and
// `CallManifestFetchCodeEx`. Each calls a user-defined Lua function and
// extracts a uint64 from the return value. Returns `None` on any of:
//   * the STATE has not been initialized (no Lua scripts loaded yet)
//   * the function is not defined
//   * the Lua call raised an error
//   * the function returned `nil`
//   * the return value was neither an integer nor a decimal string
//   * the decimal string did not parse as a u64
//
// The caller (`manifest_client`) uses `has_manifest_code_func*` to decide
// whether to try these at all, so the common path is skipped before it
// reaches here. That check and this call are intentionally separate: a
// script can define the function on some parses and not others, and the
// registry would still report the last observed state.

fn extract_u64(result: mlua::Result<Value>) -> Option<u64> {
    let v = result.ok()?;
    match v {
        Value::Integer(i) if i >= 0 => Some(i as u64),
        Value::String(s) => {
            let decoded = s.to_str().ok()?;
            crate::platform::numbers::parse_u64(&decoded)
        }
        _ => None,
    }
}

pub fn call_manifest_fetch_code(gid: u64) -> Option<u64> {
    let guard = STATE.lock().unwrap();
    let state = guard.as_ref()?;
    let globals = state.lua.globals();
    let f: mlua::Function = match globals.get("fetch_manifest_code") {
        Ok(f) => f,
        Err(_) => return None,
    };
    let result = f.call::<_, Value>(gid);
    extract_u64(Ok(result.unwrap_or(Value::Nil)))
}

pub fn call_manifest_fetch_code_ex(app_id: u64, depot_id: u64, gid: u64) -> Option<u64> {
    let guard = STATE.lock().unwrap();
    let state = guard.as_ref()?;
    let globals = state.lua.globals();
    let f: mlua::Function = match globals.get("fetch_manifest_code_ex") {
        Ok(f) => f,
        Err(_) => return None,
    };
    let result = f.call::<_, Value>((app_id, depot_id, gid));
    extract_u64(Ok(result.unwrap_or(Value::Nil)))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_hex_roundtrip() {
        assert_eq!(decode_hex("00ff10"), vec![0x00, 0xff, 0x10]);
        assert_eq!(decode_hex("A"), vec![0xa0]);
    }

    #[test]
    fn merge_watch_dirs_dedupes() {
        let got = merge_watch_dirs(&["a".into(), "a".into(), "b".into()], "a");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn headers_table_to_string_formats_correctly() {
        // Uses a fresh Lua state so the process-global STATE is
        // not involved; this is a pure formatting test.
        let lua = Lua::new();
        let t = lua.create_table().unwrap();
        t.set("Accept", "application/json").unwrap();
        t.set("X-Custom", "value").unwrap();
        let s = headers_table_to_string(t).unwrap();
        // Lua table iteration order is undefined; check both lines.
        assert!(s.contains("Accept: application/json\r\n"), "missing Accept: {s:?}");
        assert!(s.contains("X-Custom: value\r\n"), "missing X-Custom: {s:?}");
        assert!(s.ends_with("\r\n"), "must end with CRLF: {s:?}");
    }

    #[test]
    fn case_insensitive_lookup_reaches_registered_functions() {
        assert!(initialize());
        let guard = STATE.lock().unwrap();
        let state = guard.as_ref().expect("initialized above");
        let g = state.lua.globals();
        // Exact-case: raw hit, no metamethod fires.
        let exact: mlua::Result<Value> = g.get("addappid");
        assert!(matches!(exact, Ok(Value::Function(_))), "lowercase missing");
        // Mixed-case: raw miss, __index lowercases, raw_get finds it.
        for name in &["AddAppid", "ADDAPPID", "aDdApPiD", "addAppId"] {
            let v: mlua::Result<Value> = g.get(*name);
            assert!(
                matches!(v, Ok(Value::Function(_))),
                "case-insensitive lookup failed for {name}"
            );
        }
        // A genuinely unknown name still returns nil.
        let missing: mlua::Result<Value> = g.get("definitely_not_a_registered_function");
        assert!(matches!(missing, Ok(Value::Nil)));
    }

    #[test]
    fn http_get_invalid_url_returns_nil_and_error() {
        assert!(initialize());
        let guard = STATE.lock().unwrap();
        let state = guard.as_ref().expect("initialized above");
        let g = state.lua.globals();
        let get: mlua::Function = g.get("http_get").expect("http_get registered");
        // An invalid URL fails at WinHTTP parse time, so this
        // returns immediately without touching the network.
        let (body, status): (Value, Value) = get
            .call(("not-a-url", Value::Nil))
            .expect("http_get callable with 2 args");
        assert!(matches!(body, Value::Nil));
        assert!(matches!(status, Value::String(_)));
    }

}
