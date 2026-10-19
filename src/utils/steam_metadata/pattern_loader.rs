//! Pattern loader. Source: `PatternLoader.cpp`.
//!
//! `ModuleHandle` does not implement `Hash`, so the maps are keyed on the
//! raw pointer value as `usize`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::platform::dynamic_library::ModuleHandle;
use crate::platform::memory;
use crate::platform::numbers;
use crate::utils::support::fnv::fnv1a_hash;

use super::remote_toml::{self, Request as RtRequest};
use super::steam_diagnostics;

#[derive(Clone, Debug, Default)]
struct PatternEntry {
    name: String,
    rva: usize,
    sig: String,
}

type PatternMap = HashMap<u32, PatternEntry>;

static MODULE_MAPS: Mutex<Option<HashMap<usize, PatternMap>>> = Mutex::new(None);
static FAILED_MODULES: Mutex<Option<HashSet<usize>>> = Mutex::new(None);
static MISSING: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn with_maps<R>(f: impl FnOnce(&mut HashMap<usize, PatternMap>) -> R) -> R {
    let mut g = MODULE_MAPS.lock().expect("poisoned");
    if g.is_none() { *g = Some(HashMap::new()); }
    f(g.as_mut().unwrap())
}
fn with_failed<R>(f: impl FnOnce(&mut HashSet<usize>) -> R) -> R {
    let mut g = FAILED_MODULES.lock().expect("poisoned");
    if g.is_none() { *g = Some(HashSet::new()); }
    f(g.as_mut().unwrap())
}
fn with_missing<R>(f: impl FnOnce(&mut Vec<String>) -> R) -> R {
    let mut g = MISSING.lock().expect("poisoned");
    if g.is_none() { *g = Some(Vec::new()); }
    f(g.as_mut().unwrap())
}

fn parse_sig(s: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut bytes = Vec::new();
    let mut mask = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c == ' ' || c == '\t' || c == ',' { i += 1; continue; }
        if i + 1 < b.len() && c == '?' && b[i + 1] == b'?' {
            bytes.push(0); mask.push(0); i += 2; continue;
        }
        if i + 1 >= b.len() { return None; }
        let nib = |ch: u8| -> Option<u8> {
            let c = ch as char;
            match c {
                '0'..='9' => Some(c as u8 - b'0'),
                'a'..='f' => Some(c as u8 - b'a' + 10),
                'A'..='F' => Some(c as u8 - b'A' + 10),
                _ => None,
            }
        };
        let h = nib(b[i])?;
        let l = nib(b[i + 1])?;
        bytes.push((h << 4) | l);
        mask.push(1);
        i += 2;
    }
    if bytes.is_empty() { None } else { Some((bytes, mask)) }
}

unsafe fn scan_module(module: ModuleHandle, bytes: &[u8], mask: &[u8]) -> *mut u8 {
    let Some(img) = memory::get_module_image(module) else { return std::ptr::null_mut() };
    if img.size < bytes.len() { return std::ptr::null_mut(); }
    let slice = std::slice::from_raw_parts(img.base as *const u8, img.size);
    let pat_len = bytes.len();
    'outer: for i in 0..=(slice.len() - pat_len) {
        for j in 0..pat_len {
            if mask[j] != 0 && slice[i + j] != bytes[j] { continue 'outer; }
        }
        return img.base.add(i);
    }
    std::ptr::null_mut()
}

fn parse_pattern_toml(body: &str) -> PatternMap {
    let mut map = PatternMap::new();
    let parsed: toml::Value = match toml::from_str(body) { Ok(v) => v, Err(_) => return map };
    let Some(tbl) = parsed.as_table() else { return map };
    for (k, v) in tbl {
        let Some(sub) = v.as_table() else { continue };
        let Some(hash_key) = numbers::parse_hex_u32(k) else { continue };
        let mut entry = PatternEntry::default();
        if let Some(s) = sub.get("name").and_then(|v| v.as_str()) { entry.name = s.into(); }
        if let Some(s) = sub.get("rva").and_then(|v| v.as_str()) {
            if let Some(rva) = numbers::parse_hex_u64(s) { entry.rva = rva as usize; }
        }
        if let Some(s) = sub.get("sig").and_then(|v| v.as_str()) { entry.sig = s.into(); }
        map.insert(hash_key, entry);
    }
    map
}

fn show_download_failed_popup(dll_name: &str, sha: &str, component: &str) {
    steam_diagnostics::show_warning(
        "rusty_tools - Unsupported Steam Version".to_string(),
        format!(
            "rusty_tools: signature file not found for {}.\n\nHooks that depend on {} are disabled for this session; other modules are unaffected.\n\nYou can:\n  1. Wait for the next signature update, then restart Steam.\n  2. Drop a matching TOML at:\n       <Steam>\\rustytools\\pattern\\{}\\{}.toml\n  3. Check upstream:\n       https://github.com/madoiscool/steam-monitor/tree/pattern/{}\n  4. Report the diagnostics below:\n       https://github.com/madoiscool/BetterSteamTools/issues",
            dll_name, dll_name, component, sha, component
        ),
    );
}

pub const PATTERN_CHANNEL: &str = "pattern";

pub fn load(module: ModuleHandle, dll_path: &str, component: &str) -> bool {
    let r = remote_toml::fetch(&RtRequest {
        channel: PATTERN_CHANNEL.into(),
        component: component.into(),
        dll_path: dll_path.into(),
    });

    if r.ok {
        let map = parse_pattern_toml(&r.body);
        if !map.is_empty() {
            LOG_INFO!("PatternLoader: loaded {} patterns for {} ({})",
                      map.len(), component, if r.from_cache { "cache fallback" } else { "remote" });
            with_maps(|m| { m.insert(module.0 as usize, map); });
            return true;
        }
        LOG_WARN!("PatternLoader: TOML for {} parsed empty", component);
    }

    let dll_name = std::path::Path::new(dll_path)
        .file_name().and_then(|s| s.to_str()).unwrap_or("?");
    let sha = if r.sha256.is_empty() { "(hash failed)" } else { &r.sha256 };
    show_download_failed_popup(dll_name, sha, component);
    with_failed(|f| { f.insert(module.0 as usize); });
    false
}

pub fn find_pattern(module: ModuleHandle, func_name: &str) -> *mut std::ffi::c_void {
    let key = module.0 as usize;
    if with_failed(|f| f.contains(&key)) { return std::ptr::null_mut(); }

    let hash_key = fnv1a_hash(func_name);

    let entry = with_maps(|m| {
        let Some(map) = m.get(&key) else { return None };
        map.get(&hash_key).cloned()
    });

    let Some(entry) = entry else {
        // Either Load was never called or there's no entry.
        let loaded = with_maps(|m| m.contains_key(&key));
        if !loaded {
            LOG_WARN!("PatternLoader: FindPattern called for module that was never loaded ('{}')", func_name);
        } else {
            LOG_WARN!("PatternLoader: no entry for '{}' (key=0x{:08X})", func_name, hash_key);
        }
        with_missing(|v| v.push(func_name.to_string()));
        return std::ptr::null_mut();
    };

    if entry.rva != 0 {
        let addr = (module.0 as usize + entry.rva) as *mut std::ffi::c_void;
        LOG_DEBUG!("PatternLoader: {} resolved via RVA 0x{:X}", func_name, entry.rva);
        return addr;
    }

    if !entry.sig.is_empty() {
        if let Some((bytes, mask)) = parse_sig(&entry.sig) {
            let addr = unsafe { scan_module(module, &bytes, &mask) };
            if !addr.is_null() {
                let rva = addr as usize - module.0 as usize;
                LOG_DEBUG!("PatternLoader: {} resolved via sig @ RVA 0x{:X}", func_name, rva);
                return addr as *mut std::ffi::c_void;
            }
            LOG_WARN!("PatternLoader: sig scan miss for '{}'", func_name);
        } else {
            LOG_WARN!("PatternLoader: malformed sig for '{}'", func_name);
        }
    } else {
        LOG_WARN!("PatternLoader: entry for '{}' has neither rva nor sig", func_name);
    }

    with_missing(|v| v.push(func_name.to_string()));
    std::ptr::null_mut()
}

pub fn report_missing_functions() {
    let list = with_missing(|v| std::mem::take(v));
    if list.is_empty() { return; }
    let mut body = String::from("rusty_tools: some functions could not be located.\n\nThe following functions were not found in the signature file:\n");
    for name in &list { body.push_str("  - "); body.push_str(name); body.push('\n'); }
    body.push_str("\nHooks for these functions are disabled for this session.\n\nPlease report this at:\nhttps://github.com/madoiscool/BetterSteamTools/issues");
    steam_diagnostics::show_warning("rusty_tools - Missing Signatures".to_string(), body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sig_basic() {
        let (b, m) = parse_sig("48 8B ?? C0").unwrap();
        assert_eq!(b, vec![0x48, 0x8B, 0x00, 0xC0]);
        assert_eq!(m, vec![1, 1, 0, 1]);
    }

    #[test]
    fn parse_sig_rejects_empty() {
        assert!(parse_sig("").is_none());
    }

    #[test]
    fn parse_pattern_toml_extracts_entries() {
        let body = r#"
[0x82428E37]
name = "BBuildAndAsyncSendFrame"
rva = "0x1234"

[0xC37F2D8E]
name = "BuildDepotDependency"
sig = "48 8B C4"
"#;
        let m = parse_pattern_toml(body);
        assert_eq!(m.len(), 2);
        let e1 = m.get(&0x82428E37).unwrap();
        assert_eq!(e1.name, "BBuildAndAsyncSendFrame");
        assert_eq!(e1.rva, 0x1234);
    }
}
