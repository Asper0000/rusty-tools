//! Persists Lua-provided depot decryption keys into `<Steam>/config/config.vdf`
//! so external tools (like the Python blueprint extractor) can find them.
//!
//! ## Why per-app, not apps/0
//!
//! Steam re-serializes `config.vdf` from an in-memory model keyed by real
//! app IDs. Entries under an unknown app (like `apps/0`) are dropped on
//! every save, which is why the naive approach produces "18 keys added"
//! on every launch and never sticks.
//!
//! Instead, we scan the installed ACFs (`steamapps/appmanifest_*.acf`) to
//! build a `depot_id -> app_id` map, and write each key under
//! `apps/<real_app>/depots/<depot>/DecryptionKey`. That is exactly the
//! shape Steam uses for owned games, so Steam preserves it on save.
//!
//! Depots with no matching installed ACF fall back to `apps/0`. In
//! practice that only happens for depots of uninstalled apps, which the
//! extractor cannot use anyway.
//!
//! ## Keeper thread
//!
//! Even with real app IDs, Steam saves `config.vdf` throughout a session
//! (login timestamps, library bookkeeping). Any save rewrites the whole
//! file from memory, which can race our write. A keeper thread polls the
//! file's mtime every 250 ms and re-persists when it changes externally.
//! `DLL_PROCESS_DETACH` also runs one final persist to catch the shutdown
//! save.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use parking_lot::Mutex;

use super::lua_config;

// Ã¢â€â‚¬Ã¢â€â‚¬ VDF node Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

pub struct VdfNode {
    value: Option<String>,
    children: Vec<(String, VdfNode)>,
}

impl VdfNode {
    fn new_section() -> Self { Self { value: None, children: Vec::new() } }
    fn new_value(s: &str) -> Self { Self { value: Some(s.to_string()), children: Vec::new() } }

    fn child_index(&self, key: &str) -> Option<usize> {
        self.children.iter().position(|(k, _)| k.eq_ignore_ascii_case(key))
    }

    pub fn get(&self, key: &str) -> Option<&VdfNode> {
        self.child_index(key).map(|i| &self.children[i].1)
    }

    pub fn ensure_section(&mut self, key: &str) -> &mut VdfNode {
        if let Some(i) = self.child_index(key) {
            self.children[i].1.value = None;
            return &mut self.children[i].1;
        }
        self.children.push((key.to_string(), VdfNode::new_section()));
        let i = self.children.len() - 1;
        &mut self.children[i].1
    }

    fn set(&mut self, key: &str, value: &str) {
        if let Some(i) = self.child_index(key) {
            self.children[i].1.value = Some(value.to_string());
            self.children[i].1.children.clear();
        } else {
            self.children.push((key.to_string(), VdfNode::new_value(value)));
        }
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Tokenizer / parser Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

enum Tok { Str(String), OpenBrace, CloseBrace }

struct Lexer<'a> { text: &'a [u8], pos: usize }

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self { Self { text: text.as_bytes(), pos: 0 } }

    fn skip_ws(&mut self) {
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            if c.is_ascii_whitespace() {
                self.pos += 1;
            } else if c == b'/' && self.pos + 1 < self.text.len()
                && self.text[self.pos + 1] == b'/'
            {
                while self.pos < self.text.len() && self.text[self.pos] != b'\n' {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Option<Tok> {
        self.skip_ws();
        if self.pos >= self.text.len() { return None; }
        let c = self.text[self.pos];
        if c == b'{' { self.pos += 1; return Some(Tok::OpenBrace); }
        if c == b'}' { self.pos += 1; return Some(Tok::CloseBrace); }
        if c == b'"' { return self.read_quoted().map(Tok::Str); }
        self.read_unquoted().map(Tok::Str)
    }

    fn read_quoted(&mut self) -> Option<String> {
        self.pos += 1;
        let mut out: Vec<u8> = Vec::new();
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            self.pos += 1;
            if c == b'"' { return String::from_utf8(out).ok(); }
            if c == b'\\' && self.pos < self.text.len() {
                let e = self.text[self.pos];
                self.pos += 1;
                out.push(match e {
                    b'n' => b'\n', b't' => b'\t', b'r' => b'\r',
                    b'\\' => b'\\', b'"' => b'"',
                    _ => e,
                });
            } else {
                out.push(c);
            }
        }
        None
    }

    fn read_unquoted(&mut self) -> Option<String> {
        let start = self.pos;
        while self.pos < self.text.len() {
            let c = self.text[self.pos];
            if c.is_ascii_whitespace() || c == b'{' || c == b'}' { break; }
            self.pos += 1;
        }
        if start == self.pos { return None; }
        String::from_utf8(self.text[start..self.pos].to_vec()).ok()
    }
}

fn parse_vdf(text: &str) -> Option<VdfNode> {
    let mut lex = Lexer::new(text);
    let mut stack: Vec<(String, VdfNode)> = vec![(String::new(), VdfNode::new_section())];
    while let Some(tok) = lex.next() {
        match tok {
            Tok::Str(key) => match lex.next()? {
                Tok::OpenBrace => stack.push((key, VdfNode::new_section())),
                Tok::Str(value) => { stack.last_mut()?.1.set(&key, &value); }
                Tok::CloseBrace => return None,
            },
            Tok::CloseBrace => {
                if stack.len() <= 1 { return None; }
                let (k, node) = stack.pop()?;
                stack.last_mut()?.1.children.push((k, node));
            }
            Tok::OpenBrace => return None,
        }
    }
    if stack.len() != 1 { return None; }
    Some(stack.into_iter().next()?.1)
}

fn serialize_vdf(node: &VdfNode) -> String {
    let mut out = String::new();
    write_node(&mut out, node, 0);
    out
}

fn write_node(out: &mut String, node: &VdfNode, indent: usize) {
    let pad = "\t".repeat(indent);
    for (k, v) in &node.children {
        if let Some(val) = &v.value {
            out.push_str(&format!("{}\"{}\" \"{}\"\n", pad, escape(k), escape(val)));
        } else {
            out.push_str(&format!("{}\"{}\"\n{}{{\n", pad, escape(k), pad));
            write_node(out, v, indent + 1);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Existing-key walk Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

fn collect_depot_ids(node: &VdfNode, out: &mut HashSet<u32>) {
    if let Some(depots) = node.get("depots") {
        for (k, v) in &depots.children {
            if let Ok(id) = k.parse::<u32>() {
                if let Some(key) = v.get("DecryptionKey") {
                    let has = key.value.as_ref().map(|s| !s.is_empty()).unwrap_or(false);
                    if has { out.insert(id); }
                }
            }
        }
    }
    for (_, child) in &node.children {
        collect_depot_ids(child, out);
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Library discovery + ACF scan Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

fn find_libraries(steam_root: &str) -> Vec<String> {
    let mut libs = vec![steam_root.to_string()];
    let mut seen: HashSet<String> = HashSet::new();
    seen.insert(steam_root.to_lowercase());

    let lf_path = Path::new(steam_root).join("steamapps").join("libraryfolders.vdf");
    let text = match std::fs::read_to_string(&lf_path) {
        Ok(t) => t,
        Err(_) => return libs,
    };
    let root = match parse_vdf(&text) {
        Some(r) => r,
        None => return libs,
    };
    let lf = match root.get("libraryfolders") {
        Some(l) => l,
        None => return libs,
    };
    for (k, v) in &lf.children {
        if k.parse::<u32>().is_err() { continue; }
        // Modern format: <n> { path "..." }. Legacy: <n> "...".
        let path = if v.value.is_some() {
            v.value.clone()
        } else {
            v.get("path").and_then(|n| n.value.clone())
        };
        let Some(p) = path else { continue };
        let lower = p.to_lowercase();
        if seen.contains(&lower) || !Path::new(&p).is_dir() { continue; }
        seen.insert(lower);
        libs.push(p);
    }
    libs
}

fn build_depot_to_app_map(steam_root: &str) -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    for lib in find_libraries(steam_root) {
        let steamapps = Path::new(&lib).join("steamapps");
        let entries = match std::fs::read_dir(&steamapps) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            let name = match path.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") { continue; }

            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let root = match parse_vdf(&text) {
                Some(r) => r,
                None => continue,
            };
            let app = match root.get("AppState") {
                Some(a) => a,
                None => continue,
            };
            let app_id = match app
                .get("appid")
                .and_then(|n| n.value.as_ref())
                .and_then(|s| s.parse::<u32>().ok())
            {
                Some(id) => id,
                None => continue,
            };
            let installed = match app.get("InstalledDepots") {
                Some(d) => d,
                None => continue,
            };
            for (k, _) in &installed.children {
                if let Ok(depot_id) = k.parse::<u32>() {
                    map.insert(depot_id, app_id);
                }
            }
        }
    }
    map
}

// Ã¢â€â‚¬Ã¢â€â‚¬ State + failure logging Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

static LAST_WRITE_MTIME: Mutex<Option<SystemTime>> = Mutex::new(None);
static FAILED_SINCE_SUCCESS: AtomicBool = AtomicBool::new(false);
static KEEPER_SHUTDOWN: AtomicBool = AtomicBool::new(false);
static KEEPER_HANDLE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Log a persist failure. The first failure since the last success is
/// WARN; repeats are silent. Reset by any successful return from
/// [`persist_lua_keys_to_config_vdf`].
fn log_failure(msg: &str) {
    if !FAILED_SINCE_SUCCESS.swap(true, Ordering::Relaxed) {
        LOG_WARN!("VdfPersist: {}", msg);
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Public entry point Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

/// Read `config.vdf`, inject every Lua-provided depot key that is not
/// already present, and write back atomically. Uses each depot's real
/// owning app when an installed ACF can be found; falls back to
/// `apps/0` otherwise.
///
/// Always updates [`LAST_WRITE_MTIME`] after the attempt so the keeper
/// thread does not retry in a tight loop on persistent failure. Retry
/// happens on the next external write.
pub fn persist_lua_keys_to_config_vdf(steam_root: &str) -> bool {
    let config_path = Path::new(steam_root).join("config").join("config.vdf");
    let result = inner_persist(steam_root, &config_path);

    if let Ok(mtime) = std::fs::metadata(&config_path).and_then(|m| m.modified()) {
        *LAST_WRITE_MTIME.lock() = Some(mtime);
    }
    if result {
        FAILED_SINCE_SUCCESS.store(false, Ordering::Relaxed);
    }
    result
}

fn inner_persist(steam_root: &str, config_path: &Path) -> bool {
    let text = match std::fs::read_to_string(config_path) {
        Ok(t) => t,
        Err(e) => {
            log_failure(&format!("cannot read {}: {}", config_path.display(), e));
            return false;
        }
    };
    let mut root = match parse_vdf(&text) {
        Some(r) => r,
        None => {
            log_failure(&format!("failed to parse {}", config_path.display()));
            return false;
        }
    };
    if root.children.iter().any(|(k, _)| k.eq_ignore_ascii_case("#base")) {
        log_failure("config.vdf contains #base directives, refusing to rewrite");
        return false;
    }
    let structure_ok = root
        .get("InstallConfigStore")
        .and_then(|n| n.get("Software"))
        .and_then(|n| n.get("Valve"))
        .and_then(|n| n.get("Steam"))
        .is_some();
    if !structure_ok {
        log_failure("unexpected config.vdf structure, refusing to write");
        return false;
    }

    let depot_to_app = build_depot_to_app_map(steam_root);

    let mut existing: HashSet<u32> = HashSet::new();
    collect_depot_ids(&root, &mut existing);

    // Group to_add by real app id. Depots with no ACF match go under 0.
    let mut by_app: HashMap<u32, Vec<(u32, String)>> = HashMap::new();
    for depot_id in lua_config::get_all_depot_ids() {
        if existing.contains(&depot_id) { continue; }
        let key = lua_config::get_decryption_key(depot_id);
        if key.is_empty() { continue; }
        let app_id = depot_to_app.get(&depot_id).copied().unwrap_or(0);
        by_app.entry(app_id).or_default().push((depot_id, hex::encode(&key)));
    }

    let total_new: usize = by_app.values().map(|v| v.len()).sum();
    if total_new == 0 {
        LOG_DEBUG!("VdfPersist: no new depot key(s) ({} already present)", existing.len());
        return true;
    }

    let apps = root
        .ensure_section("InstallConfigStore")
        .ensure_section("Software")
        .ensure_section("Valve")
        .ensure_section("Steam")
        .ensure_section("apps");

    for (app_id, depots) in &by_app {
        let app_node = apps.ensure_section(&app_id.to_string());
        let depot_section = app_node.ensure_section("depots");
        for (depot_id, key_hex) in depots {
            depot_section
                .ensure_section(&depot_id.to_string())
                .set("DecryptionKey", key_hex);
        }
    }

    let output = serialize_vdf(&root);
    let tmp = config_path.with_extension("vdf.rustytools.tmp");
    if let Err(e) = std::fs::write(&tmp, &output) {
        log_failure(&format!("write temp failed: {}", e));
        return false;
    }
    if let Err(e) = std::fs::rename(&tmp, config_path) {
        log_failure(&format!("rename failed: {}", e));
        let _ = std::fs::remove_file(&tmp);
        return false;
    }

    let real_apps = by_app.keys().filter(|&&a| a != 0).count();
    let unmatched = by_app.get(&0).map(|v| v.len()).unwrap_or(0);
    LOG_INFO!(
        "VdfPersist: wrote {} key(s) across {} real app(s){} ({} already present)",
        total_new,
        real_apps,
        if unmatched > 0 { format!(" + {} under apps/0", unmatched) } else { String::new() },
        existing.len(),
    );
    true
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Keeper Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

pub fn start_keeper(steam_root: &str) {
    if KEEPER_HANDLE.lock().is_some() {
        LOG_DEBUG!("VdfKeeper: already running");
        return;
    }
    KEEPER_SHUTDOWN.store(false, Ordering::SeqCst);
    let root = steam_root.to_string();
    match std::thread::Builder::new()
        .name("ost-vdf-keeper".into())
        .spawn(move || keeper_loop(&root))
    {
        Ok(h) => { *KEEPER_HANDLE.lock() = Some(h); }
        Err(e) => LOG_WARN!("VdfKeeper: failed to spawn thread: {}", e),
    }
}

pub fn stop_keeper() {
    KEEPER_SHUTDOWN.store(true, Ordering::SeqCst);
    if let Some(h) = KEEPER_HANDLE.lock().take() {
        let _ = h.join();
    }
}

fn keeper_loop(steam_root: &str) {
    let config_path = Path::new(steam_root).join("config").join("config.vdf");
    LOG_INFO!("VdfKeeper: watching {}", config_path.display());
    loop {
        // Sleep 250 ms total, in 10 ms chunks, so shutdown is detected
        // within 10 ms of being signaled.
        for _ in 0..25 {
            if KEEPER_SHUTDOWN.load(Ordering::SeqCst) {
                LOG_INFO!("VdfKeeper: stopped");
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let current = match std::fs::metadata(&config_path).and_then(|m| m.modified()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let recorded = *LAST_WRITE_MTIME.lock();
        if recorded.map(|t| t == current).unwrap_or(false) {
            continue; // no external write since our last attempt
        }
        LOG_DEBUG!("VdfKeeper: config.vdf changed externally, re-persisting");
        persist_lua_keys_to_config_vdf(steam_root);
    }
}

// Ã¢â€â‚¬Ã¢â€â‚¬ Tests Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_roundtrip() {
        let src = r#"
"InstallConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "730" { "depots" { "731" { "DecryptionKey" "abcd" } } }
                }
            }
        }
    }
}
"#;
        let root = parse_vdf(src).expect("parse");
        let mut out = HashSet::new();
        collect_depot_ids(&root, &mut out);
        assert!(out.contains(&731));

        let s = serialize_vdf(&root);
        let root2 = parse_vdf(&s).expect("reparse");
        let mut out2 = HashSet::new();
        collect_depot_ids(&root2, &mut out2);
        assert!(out2.contains(&731));
    }

    #[test]
    fn inject_new_depot_under_real_app() {
        let mut root = parse_vdf(r#"
"InstallConfigStore" { "Software" { "Valve" { "Steam" { "apps" { } } } } }
"#).unwrap();
        let apps = root
            .ensure_section("InstallConfigStore")
            .ensure_section("Software")
            .ensure_section("Valve")
            .ensure_section("Steam")
            .ensure_section("apps");
        // Simulate writing 209651 under app 209650.
        apps.ensure_section("209650")
            .ensure_section("depots")
            .ensure_section("209651")
            .set("DecryptionKey", "deadbeef");
        let s = serialize_vdf(&root);
        let reparsed = parse_vdf(&s).unwrap();
        let mut out = HashSet::new();
        collect_depot_ids(&reparsed, &mut out);
        assert!(out.contains(&209651));
    }

    #[test]
    fn skips_existing_depot() {
        let mut root = parse_vdf(r#"
"InstallConfigStore" { "Software" { "Valve" { "Steam" { "apps" {
    "730" { "depots" { "731" { "DecryptionKey" "abcd" } } }
} } } }
"#).unwrap();
        let mut existing = HashSet::new();
        collect_depot_ids(&root, &mut existing);
        assert!(existing.contains(&731));

        let apps = root
            .ensure_section("InstallConfigStore")
            .ensure_section("Software")
            .ensure_section("Valve")
            .ensure_section("Steam")
            .ensure_section("apps");
        for id in [731u32] {
            if !existing.contains(&id) {
                apps.ensure_section("0")
                    .ensure_section("depots")
                    .ensure_section(&id.to_string())
                    .set("DecryptionKey", "wrong");
            }
        }
        let s = serialize_vdf(&root);
        assert!(!s.contains("\"wrong\""));
    }
}
